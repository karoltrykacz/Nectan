use crate::{
    MB,
    walker::{self, Walker},
};
use core::hash;
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Instant, UNIX_EPOCH},
};

pub struct Vault {
    root: PathBuf,
    root_node: Node,
    total_size: u64,
    files: u64,
    folders: u64,
    symlinks: u64,
}
impl Vault {
    pub fn new(root: PathBuf) -> Self {
        let walker = Walker::new();
        let root_node = walker.walk(&root).unwrap();
        let total_size = walker.total_size();

        let folders = walker.folders();
        let files = walker.files();
        let symlinks = walker.symlinks();

        Vault {
            root,
            root_node,
            total_size,
            files,
            folders,
            symlinks,
        }
    }
}

pub enum ListCmd {
    Add(VerifyingKey),
    Remove(VerifyingKey),
}

/// Set of commands thay the containers use to sync each other
pub enum SyncCmd {
    /// Command sent after some file(s) changed
    IndexUpdate(Vec<FlatNode>),
    Request(PathBuf),
    Get(PathBuf),
    Dir(PathBuf),
    Rescan(PathBuf),
    Greenlist(ListCmd),
    Redlist(ListCmd),
}

/// Helper struct to hold root and meta without the children
pub struct FlatNode {
    path: PathBuf,
    meta: ItemMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    #[serde(with = "lossy_map")]
    children: HashMap<PathBuf, Node>,
    meta: ItemMeta,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum FileType {
    File,
    Dir,
    Symlink,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VersionCounter {
    user: VerifyingKey,
    value: u64,
}
impl Eq for VersionCounter {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemMeta {
    pub entry_type: FileType,
    pub size: u64,
    pub modified: u64,
    pub deleted: bool,
    pub versions: Vec<VersionCounter>,
    pub sequence: u64,
    pub blocks: Option<Vec<Block>>,
}

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub hash: blake3::Hash,
}

impl Block {
    pub fn block_size(file_size: u64) -> u64 {
        match file_size {
            n if n < 500 * MIB => 2 * MIB,
            n if n < 1_000 * MIB => 4 * MIB,
            n if n < 2_000 * MIB => 8 * MIB,
            n if n < 4_000 * MIB => 16 * MIB,
            _ => 16 * MIB,
        }
    }
}

// message BlockInfo {
//     int64 offset = 1;
//     int32 size   = 2;
//     bytes hash   = 3;
//     reserved 4;
// }

impl ItemMeta {
    fn differs(&self, o: &ItemMeta) -> bool {
        self.entry_type != o.entry_type
            || self.size != o.size
            || self.modified != o.modified
            || self.deleted != o.deleted
            // || self.hash != o.hash
            || self.versions != o.versions
    }

    pub fn dir() -> Self {
        ItemMeta {
            entry_type: FileType::Dir,
            size: 0,
            modified: 0,
            deleted: false,
            versions: Vec::new(),
            sequence: 0,
            blocks: None,
        }
    }

    // pub fn from_path(path: &Path, sequence: u64) -> Self {
    //     let md = std::fs::symlink_metadata(path).ok();
    //     let entry_type = match &md {
    //         Some(m) if m.is_dir() => FileType::Dir,
    //         Some(m) if m.file_type().is_symlink() => FileType::Symlink,
    //         _ => FileType::File,
    //     };
    //     let size = match (&md, entry_type) {
    //         (Some(m), FileType::File) => m.len(),
    //         _ => 0,
    //     };
    //     let modified = md
    //         .and_then(|m| m.modified().ok())
    //         .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
    //         .map(|d| d.as_secs())
    //         .unwrap_or(0);
    //
    //     ItemMeta {
    //         entry_type,
    //         size,
    //         modified,
    //         deleted: false,
    //         versions: Vec::new(),
    //         sequence,
    //         blocks: None,
    //     }
    // }

    pub fn is_file(&self) -> bool {
        self.entry_type == FileType::File
    }
}

mod lossy_map {
    use serde::{Deserializer, Serializer};

    use super::*;

    pub fn serialize<S>(map: &HashMap<PathBuf, Node>, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let r: HashMap<String, &Node> = map
            .iter()
            .map(|(k, v)| (k.to_string_lossy().into_owned(), v))
            .collect();
        r.serialize(ser)
    }

    pub fn deserialize<'de, D>(de: D) -> Result<HashMap<PathBuf, Node>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let r: HashMap<String, Node> = HashMap::deserialize(de)?;
        Ok(r.into_iter().map(|(k, v)| (PathBuf::from(k), v)).collect())
    }
}

impl Node {
    pub fn new(meta: ItemMeta) -> Self {
        Node {
            children: HashMap::new(),
            meta,
        }
    }
    pub fn diff(&self, other: &Node) -> Vec<PathBuf> {
        let mut out = Vec::new();
        self.diff_inner(other, Path::new(""), &mut out);
        out
    }
    fn diff_inner(&self, other: &Node, prefix: &Path, out: &mut Vec<PathBuf>) {
        if !prefix.as_os_str().is_empty() && self.meta.differs(&other.meta) {
            out.push(prefix.to_path_buf());
        }
        for (key, mine) in &self.children {
            let p = prefix.join(key);
            match other.children.get(key) {
                Some(theirs) => mine.diff_inner(theirs, &p, out),
                None => mine.collect_all(&p, out),
            }
        }
        for (key, theirs) in &other.children {
            if !self.children.contains_key(key) {
                theirs.collect_all(&prefix.join(key), out);
            }
        }
    }

    fn collect_all(&self, path: &Path, out: &mut Vec<PathBuf>) {
        out.push(path.to_path_buf());
        for (key, child) in &self.children {
            child.collect_all(&path.join(key), out);
        }
    }

    pub fn root() -> Self {
        Node::new(ItemMeta::dir())
    }

    pub fn meta(&self) -> &ItemMeta {
        &self.meta
    }

    pub fn total_size(&self) -> u64 {
        self.children
            .values()
            .map(|c| c.meta.size + c.total_size())
            .sum()
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

    /// Insert a path. Missing parents become `ItemMeta::dir()`.
    /// The last component gets `meta` (overwrites if it already existed,
    /// e.g. was made earlier as an implicit parent).
    pub fn insert(&mut self, path: &Path, meta: ItemMeta) {
        let mut node = self;
        let mut comps = path.components().peekable();
        while let Some(comp) = comps.next() {
            let key = PathBuf::from(comp.as_os_str());
            node = node
                .children
                .entry(key)
                .or_insert_with(|| Node::new(ItemMeta::dir()));
            if comps.peek().is_none() {
                node.meta = meta;
                break;
            }
        }
    }

    pub fn common_parent(&self) -> PathBuf {
        let mut node = self;
        let mut parts = Vec::new();
        while node.children.len() == 1 {
            let (k, v) = node.children.iter().next().unwrap();
            parts.push(k.clone());
            node = v;
        }
        parts.into_iter().collect()
    }

    /// Direct children of `root`: (full path, meta).
    pub fn children_of(&self, root: &Path) -> Option<Vec<(PathBuf, ItemMeta)>> {
        let mut node = self;
        for comp in root.components() {
            let key = PathBuf::from(comp.as_os_str());
            match node.children.get(&key) {
                Some(n) => node = n,
                None => return None,
            }
        }
        Some(
            node.children
                .iter()
                .map(|(c, t)| (root.join(c), t.meta.clone()))
                .collect(),
        )
    }

    /// Structural merge. On a node in both trees, children are merged and
    /// the meta with the higher `sequence` wins.
    fn merge_inner(&mut self, other: Node) {
        if other.meta.modified > self.meta.modified
            || (other.meta.modified == self.meta.modified
                && other.meta.sequence > self.meta.sequence)
        {
            self.meta = other.meta;
        }

        for (key, other_child) in other.children {
            match self.children.get_mut(&key) {
                Some(existing) => existing.merge_inner(other_child),
                None => {
                    self.children.insert(key, other_child);
                }
            }
        }
    }

    pub fn merge(&mut self, other: Node) {
        self.merge_inner(other);
    }

    pub fn merged(mut self, other: Node) -> Node {
        self.merge(other);
        self
    }

    pub fn merge_all(trees: impl IntoIterator<Item = Node>) -> Node {
        let mut root = Node::root();
        for t in trees {
            root.merge_inner(t);
        }
        root
    }

    pub fn serialize(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap()
    }

    pub fn compress(&self) {
        let start = Instant::now();
        let encoded = postcard::to_allocvec(self).unwrap();
        let ser = start.elapsed();
        let original = encoded.len() as f32 / MB;
        let start = Instant::now();
        let result = zstd::encode_all(&*encoded, 0).unwrap().len() as f32 / MB;
        let com = start.elapsed();

        println!(
            "Serialization {ser:?}. Original [{original:.2}] Compression {com:.2?} [{result:.2} MB]"
        );
    }
    pub fn get(&self, path: &Path) -> Option<&Node> {
        let mut node = self;
        for comp in path.components() {
            let key = PathBuf::from(comp.as_os_str());
            node = node.children.get(&key)?;
        }
        Some(node)
    }
}
