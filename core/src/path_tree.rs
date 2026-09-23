use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

mod lossy_map {
    use serde::{Deserializer, Serializer};

    use super::*;

    pub fn serialize<S>(map: &HashMap<PathBuf, PathTree>, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let r: HashMap<String, &PathTree> = map
            .iter()
            .map(|(k, v)| (k.to_string_lossy().into_owned(), v))
            .collect();
        r.serialize(ser)
    }

    pub fn deserialize<'de, D>(de: D) -> Result<HashMap<PathBuf, PathTree>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let r: HashMap<String, PathTree> = HashMap::deserialize(de)?;
        Ok(r.into_iter().map(|(k, v)| (PathBuf::from(k), v)).collect())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PathTree {
    #[serde(with = "lossy_map")]
    children: HashMap<PathBuf, PathTree>,
    is_file: bool,
    /// File size in bytes. 0 for directories or unknown.
    size: u64,
    /// Stable id, assigned at insert time, independent of HashMap iteration order.
    id: u32,
}

pub struct PathTreeIter<'a> {
    stack: Vec<(
        PathBuf,
        std::collections::hash_map::Iter<'a, PathBuf, PathTree>,
    )>,
}

impl<'a> Iterator for PathTreeIter<'a> {
    type Item = (PathBuf, bool, u64, u32); // path, is_file, size, id

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((prefix, iter)) = self.stack.last_mut() {
            match iter.next() {
                Some((segment, child)) => {
                    let full_path = prefix.join(segment);
                    let item = (full_path.clone(), child.is_file, child.size, child.id);
                    self.stack.push((full_path, child.children.iter()));
                    return Some(item);
                }
                None => {
                    self.stack.pop();
                }
            }
        }
        None
    }
}

impl<'a> IntoIterator for &'a PathTree {
    type Item = (PathBuf, bool, u64, u32);
    type IntoIter = PathTreeIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        PathTreeIter {
            stack: vec![(PathBuf::new(), self.children.iter())],
        }
    }
}

impl PathTree {
    pub fn new() -> Self {
        PathTree {
            children: HashMap::new(),
            is_file: false,
            size: 0,
            id: 0,
        }
    }

    pub fn to_vec(&self) -> Vec<(PathBuf, bool, u64, u32)> {
        self.into_iter().collect()
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    /// Total size of all files under this node (recursive).
    pub fn total_size(&self) -> u64 {
        let mut acc = 0u64;
        for child in self.children.values() {
            acc += child.size;
            acc += child.total_size();
        }
        acc
    }

    pub fn count(&self) -> u64 {
        let mut acc = 0u64;
        self.count_inner(&mut acc);
        acc
    }
    fn count_inner(&self, acc: &mut u64) {
        for child in self.children.values() {
            *acc += 1;
            child.count_inner(acc);
        }
    }

    /// Insert a path. `size` is ignored (left 0) when `is_file` is false.
    /// `next_id` is a shared counter across a whole build/insert session so
    /// every node gets a unique, deterministic id.
    pub fn insert(&mut self, path: &Path, is_file: bool, size: u64, next_id: &mut u32) {
        let mut node = self;
        let mut comps = path.components().peekable();
        while let Some(comp) = comps.next() {
            let key = PathBuf::from(comp.as_os_str());
            node = node.children.entry(key).or_insert_with(|| {
                let id = *next_id;
                *next_id += 1;
                let mut n = PathTree::new();
                n.id = id;
                n
            });
            if comps.peek().is_none() {
                node.is_file = is_file;
                node.size = if is_file { size } else { 0 };
            }
        }
    }

    pub fn get_depth_0(&self) -> Vec<(PathBuf, bool, u64)> {
        let mut root = self.common_parent();
        root.pop();
        let mut node = self;
        for comp in root.components() {
            let key = PathBuf::from(comp.as_os_str());
            match node.children.get(&key) {
                Some(n) => node = n,
                None => return Vec::new(),
            }
        }
        node.children
            .iter()
            .map(|(p, t)| (root.join(p), t.is_file, t.size))
            .collect()
    }

    pub fn common_parent(&self) -> PathBuf {
        let mut node = self;
        let mut parts = Vec::new();
        loop {
            if node.children.len() != 1 {
                break;
            }
            let (k, v) = node.children.iter().next().unwrap();
            parts.push(k.clone());
            node = v;
        }
        parts.into_iter().collect()
    }

    pub fn children_of(&self, root: &Path) -> Vec<(PathBuf, bool, u64)> {
        let mut node = self;
        for comp in root.components() {
            let key = PathBuf::from(comp.as_os_str());
            match node.children.get(&key) {
                Some(n) => node = n,
                None => return Vec::new(),
            }
        }
        node.children
            .iter()
            .map(|(c, t)| (root.join(c), t.is_file, t.size))
            .collect()
    }

    /// Sort input paths first so id assignment is deterministic regardless of
    /// the order the caller happened to collect them in.
    pub fn build(paths: &[PathBuf]) -> Self {
        let mut sorted: Vec<&PathBuf> = paths.iter().collect();
        sorted.sort();
        let mut root = PathTree::new();
        let mut next_id = 0u32;
        for p in sorted {
            let is_file = p.is_file();
            let size = if is_file {
                std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
            } else {
                0
            };
            root.insert(p, is_file, size, &mut next_id);
        }
        root
    }

    /// Raw structural merge, ids left as-is (may collide between the two
    /// trees since each was built starting its own counter at 0). Callers
    /// should follow with `renumber()` — `merge`/`merge_all` do this for you.
    fn merge_inner(&mut self, other: PathTree) {
        for (key, other_child) in other.children {
            match self.children.get_mut(&key) {
                Some(existing) => existing.merge_inner(other_child),
                None => {
                    self.children.insert(key, other_child);
                }
            }
        }
    }

    fn renumber(&mut self) {
        let mut next_id = 0u32;
        self.renumber_inner(&mut next_id);
    }
    fn renumber_inner(&mut self, next_id: &mut u32) {
        for child in self.children.values_mut() {
            child.id = *next_id;
            *next_id += 1;
            child.renumber_inner(next_id);
        }
    }

    pub fn merge(&mut self, other: PathTree) {
        self.merge_inner(other);
        self.renumber();
    }

    pub fn merged(mut self, other: PathTree) -> PathTree {
        self.merge(other);
        self
    }

    pub fn merge_all(trees: impl IntoIterator<Item = PathTree>) -> PathTree {
        let mut root = PathTree::new();
        for t in trees {
            root.merge_inner(t);
        }
        root.renumber();
        root
    }

    pub fn serialize(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap()
    }

    pub fn compress(&self) -> CompressedPathTree {
        let encoded = postcard::to_allocvec(self).unwrap();
        CompressedPathTree(zstd::encode_all(&*encoded, 0).unwrap())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CompressedPathTree(Vec<u8>);

impl CompressedPathTree {
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn decompress(&self) -> anyhow::Result<PathTree> {
        let x = zstd::decode_all(self.0.as_slice())?;
        Ok(postcard::from_bytes(&x)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_single_path() {
        let paths = vec![PathBuf::from("/a/b/c")];
        let t = PathTree::build(&paths);
        let root = PathBuf::from("/a");
        let kids: Vec<PathBuf> = t.children_of(&root).drain(..).map(|t| t.0).collect();
        assert_eq!(kids, vec![PathBuf::from("/a/b")]);
    }

    #[test]
    fn tree_multi_children() {
        let paths = vec![
            PathBuf::from("/a/b"),
            PathBuf::from("/a/c"),
            PathBuf::from("/a/b/d"),
        ];
        let t = PathTree::build(&paths);
        let mut children: Vec<PathBuf> = t
            .children_of(&PathBuf::from("/a"))
            .drain(..)
            .map(|t| t.0)
            .collect();
        children.sort();
        let mut expect = vec![PathBuf::from("/a/b"), PathBuf::from("/a/c")];
        expect.sort();
        assert_eq!(children, expect);
    }

    #[test]
    fn children_of_missing_root() {
        let paths = vec![PathBuf::from("/a/b")];
        let t = PathTree::build(&paths);
        let kids = t.children_of(&PathBuf::from("/x/y"));
        assert!(kids.is_empty());
    }

    #[test]
    fn children_of_leaf_returns_empty() {
        let paths = vec![PathBuf::from("/a/b")];
        let t = PathTree::build(&paths);
        let kids = t.children_of(&PathBuf::from("/a/b"));
        assert!(kids.is_empty());
    }

    #[test]
    fn roundtrip_compress_decompress() {
        let paths = vec![
            PathBuf::from("/a/b/c"),
            PathBuf::from("/a/b/d"),
            PathBuf::from("/a/e"),
        ];

        let t = PathTree::build(&paths).compress();
        let restored = t.decompress().unwrap();

        let mut children: Vec<PathBuf> = restored
            .children_of(&PathBuf::from("/a/b"))
            .drain(..)
            .map(|t| t.0)
            .collect();
        children.sort();
        let mut expect = vec![PathBuf::from("/a/b/c"), PathBuf::from("/a/b/d")];
        expect.sort();
        assert_eq!(children, expect);
    }

    #[test]
    fn decompress_garbage_errs() {
        let bad = CompressedPathTree(vec![0u8, 1, 2, 3]);
        assert!(bad.decompress().is_err());
    }

    #[test]
    fn common_parent() {
        let paths = vec![PathBuf::from("/a/b/c"), PathBuf::from("/a/b/d")];
        let t = PathTree::build(&paths);
        assert_eq!(t.common_parent(), PathBuf::from("/a/b"));
    }

    #[test]
    fn common_parent_2() {
        let paths = vec![PathBuf::from("/a/b"), PathBuf::from("/x/y")];
        let t = PathTree::build(&paths);
        assert_eq!(t.common_parent(), PathBuf::from("/"));
    }

    #[test]
    fn common_parent_3() {
        let paths = vec![PathBuf::from("/a/a/a/a"), PathBuf::from("/a/a/a/a")];
        let t = PathTree::build(&paths);
        assert_eq!(t.common_parent(), PathBuf::from("/a/a/a/a"));
    }

    #[test]
    fn common_parent_4() {
        let paths = vec![PathBuf::from("/a/a/a/a"), PathBuf::from("/a/a/a/a/")];
        let t = PathTree::build(&paths);
        assert_eq!(t.common_parent(), PathBuf::from("/a/a/a/a"));
    }
    #[test]
    fn merge_two_trees() {
        let t1 = PathTree::build(&[PathBuf::from("/a/b/c")]);
        let t2 = PathTree::build(&[PathBuf::from("/a/b/d"), PathBuf::from("/a/e")]);

        let merged = t1.merged(t2);

        let mut children: Vec<PathBuf> = merged
            .children_of(&PathBuf::from("/a"))
            .drain(..)
            .map(|t| t.0)
            .collect();
        children.sort();
        let mut expect = vec![PathBuf::from("/a/b"), PathBuf::from("/a/e")];
        expect.sort();
        assert_eq!(children, expect);

        let mut bc: Vec<PathBuf> = merged
            .children_of(&PathBuf::from("/a/b"))
            .drain(..)
            .map(|t| t.0)
            .collect();
        bc.sort();
        let mut expect_bc = vec![PathBuf::from("/a/b/c"), PathBuf::from("/a/b/d")];
        expect_bc.sort();
        assert_eq!(bc, expect_bc);
    }

    #[test]
    fn merge_overlapping_paths_no_dup() {
        let t1 = PathTree::build(&[PathBuf::from("/a/b")]);
        let t2 = PathTree::build(&[PathBuf::from("/a/b")]);
        let merged = t1.merged(t2);
        let children: Vec<PathBuf> = merged
            .children_of(&PathBuf::from("/a"))
            .drain(..)
            .map(|t| t.0)
            .collect();
        assert_eq!(children, vec![PathBuf::from("/a/b")]);
    }

    #[test]
    fn insert_tracks_file_size() {
        let mut t = PathTree::new();
        let mut next_id = 0u32;
        t.insert(&PathBuf::from("/a/b/c"), true, 1234, &mut next_id);
        let kids = t.children_of(&PathBuf::from("/a/b"));
        assert_eq!(kids, vec![(PathBuf::from("/a/b/c"), true, 1234)]);
        assert_eq!(t.total_size(), 1234);
    }

    #[test]
    fn ids_unique_and_deterministic() {
        let paths = vec![
            PathBuf::from("/a/b/c"),
            PathBuf::from("/a/b/d"),
            PathBuf::from("/a/e"),
        ];
        let t1 = PathTree::build(&paths);

        // Same paths, different collection order -> same ids, since build() sorts.
        let mut reordered = paths.clone();
        reordered.reverse();
        let t2 = PathTree::build(&reordered);

        let mut ids1: Vec<u32> = t1.to_vec().iter().map(|(_, _, _, id)| *id).collect();
        let mut ids2: Vec<u32> = t2.to_vec().iter().map(|(_, _, _, id)| *id).collect();
        ids1.sort();
        ids2.sort();

        // All unique.
        let unique: std::collections::HashSet<u32> = ids1.iter().cloned().collect();
        assert_eq!(unique.len(), ids1.len());

        assert_eq!(ids1, ids2);
    }

    #[cfg(unix)]
    #[test]
    /// Invalid utf8 paths
    fn invalid_utf8() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        // Invalid utf8
        let bad = OsString::from_vec(vec![0x66, 0x6f, 0xff, 0x6f]);
        let bad_path = PathBuf::from("/a").join(&bad);

        let t = PathTree::build(&[bad_path.clone()]);

        assert!(bad_path.to_str().is_none());

        let bytes = t.serialize();
        assert!(!bytes.is_empty());

        let compressed = t.compress();
        let restored = compressed.decompress().unwrap();

        let kids = restored.children_of(&PathBuf::from("/a"));
        assert_eq!(kids.len(), 1);
        let restored_path = &kids[0].0;

        // Lost data after lossy conversion. Not equal after round trip
        assert_ne!(restored_path, &bad_path);

        let s = restored_path.to_string_lossy();
        assert!(s.contains('\u{FFFD}'));
    }
}
