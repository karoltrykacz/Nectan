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
}

impl PathTree {
    pub fn new() -> Self {
        PathTree {
            children: HashMap::new(),
            is_file: false,
        }
    }

    pub fn to_vec(&self) -> Vec<(PathBuf, bool)> {
        let mut paths = Vec::new();
        self.collect_paths(&PathBuf::new(), &mut paths);
        paths
    }

    fn collect_paths(&self, current_prefix: &PathBuf, acc: &mut Vec<(PathBuf, bool)>) {
        for (segment, child) in &self.children {
            let full_path = current_prefix.join(segment);
            acc.push((full_path.clone(), child.is_file));
            child.collect_paths(&full_path, acc);
        }
    }

    pub fn count(&self) -> u64 {
        let mut acc = 0u64;
        self.count_inner(&PathBuf::new(), &mut acc);
        acc
    }
    fn count_inner(&self, current_prefix: &PathBuf, acc: &mut u64) {
        for (segment, child) in &self.children {
            let full_path = current_prefix.join(segment);
            *acc += 1;
            child.count_inner(&full_path, acc);
        }
    }

    pub fn insert(&mut self, path: &Path, is_file: bool) {
        let mut node = self;
        let mut comps = path.components().peekable();
        while let Some(comp) = comps.next() {
            let key = PathBuf::from(comp.as_os_str());
            node = node.children.entry(key).or_insert_with(PathTree::new);
            if comps.peek().is_none() {
                node.is_file = is_file;
            }
        }
    }

    pub fn get_depth_0(&self) -> Vec<(PathBuf, bool)> {
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
            .map(|(p, t)| (root.join(p), t.is_file))
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

    pub fn children_of(&self, root: &Path) -> Vec<(PathBuf, bool)> {
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
            .map(|(c, t)| (root.join(c), t.is_file))
            .collect()
    }

    pub fn build(paths: &[PathBuf]) -> Self {
        let mut root = PathTree::new();
        for p in paths {
            root.insert(p, p.is_file());
        }
        root
    }

    pub fn merge(&mut self, other: PathTree) {
        for (key, other_child) in other.children {
            match self.children.get_mut(&key) {
                Some(existing) => existing.merge(other_child),
                None => {
                    self.children.insert(key, other_child);
                }
            }
        }
    }

    pub fn merged(mut self, other: PathTree) -> PathTree {
        self.merge(other);
        self
    }

    pub fn merge_all(trees: impl IntoIterator<Item = PathTree>) -> PathTree {
        let mut root = PathTree::new();
        for t in trees {
            root.merge(t);
        }
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
}
