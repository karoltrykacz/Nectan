use crate::{
    MB,
    vault::{Block, FileType, ItemMeta, Node},
};
use ignore::{DirEntry, Error, WalkBuilder, WalkState};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{
            AtomicBool, AtomicU64,
            Ordering::{self, Relaxed},
        },
        mpsc::{Sender, channel},
    },
    thread::JoinHandle,
    time::{Duration, Instant, UNIX_EPOCH},
};
use tracing::{error, info, warn};

pub struct WalkerInner {
    total_size: AtomicU64,
    files: AtomicU64,
    folders: AtomicU64,
    symlinks: AtomicU64,
    pub stop_token: AtomicBool,
    pub finished: AtomicBool,
}

#[derive(Clone)]
pub struct Walker(Arc<WalkerInner>);

impl Walker {
    pub fn new() -> Self {
        Walker(Arc::new(WalkerInner {
            files: AtomicU64::new(0),
            folders: AtomicU64::new(0),
            symlinks: AtomicU64::new(0),
            total_size: AtomicU64::new(0),
            stop_token: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        }))
    }

    fn hash_file(path: &Path, file_size: u64) -> std::io::Result<Vec<Block>> {
        let mut file = std::fs::File::open(&path)?;
        let bs = Block::block_size(file_size);
        let mut blocks: Vec<Block> = Vec::with_capacity(file_size.div_ceil(bs) as usize);

        loop {
            let mut hasher = blake3::Hasher::new();
            let n = std::io::copy(&mut (&mut file).take(bs), &mut hasher)?;
            if n == 0 {
                break;
            }
            blocks.push(Block {
                hash: hasher.finalize(),
            });
        }
        Ok(blocks)
    }

    pub fn walk(&self, root: &Path) -> Option<Node> {
        let start = Instant::now();
        info!("Walking {}", root.display());

        let c = self.clone();
        std::thread::spawn(move || {
            loop {
                if c.finished() || c.should_abort() {
                    return;
                }
                let files = c.files();
                let folders = c.folders();
                let symlinks = c.symlinks();
                let elapsed = start.elapsed();

                info!(
                    "Scanning [{elapsed:?}]. [{files} files] [{folders} folders] [{symlinks} symlinks]",
                );
                std::thread::sleep(Duration::from_millis(15));
            }
        });

        let c = self.clone();
        let (tx, rx) = channel::<Node>();
        let walker = WalkBuilder::new(root).build_parallel();

        let mut node = Node::root();

        walker.run(|| {
            let c = c.clone();
            let mut flush = Flush {
                tx: tx.clone(),
                model: Node::root(),
            };
            let f = move |r: Result<DirEntry, Error>| {
                if let Ok(entry) = r
                    && let Ok(md) = entry.metadata()
                {
                    let path = entry.into_path();
                    let size = md.len();

                    let Some(modified) = md
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                    else {
                        error!("Failed to read entry metadata. {}", path.display());
                        return WalkState::Continue;
                    };

                    let (entry_type, blocks) = if md.is_dir() {
                        (FileType::Dir, None)
                    } else if md.file_type().is_symlink() {
                        c.folders.fetch_add(1, Relaxed);
                        (FileType::Symlink, None)
                    } else {
                        let Ok(blocks) = Walker::hash_file(&path, size) else {
                            error!("Failed to read file");
                            return WalkState::Continue;
                        };

                        c.files.fetch_add(1, Relaxed);
                        (FileType::File, Some(blocks))
                    };

                    let meta = ItemMeta {
                        entry_type,
                        size,
                        modified,
                        deleted: false,
                        versions: Vec::new(),
                        sequence: 0,
                        blocks,
                    };

                    flush.model.insert(&path, meta);

                    // Update live progress
                    c.total_size.fetch_add(size, Relaxed);
                };

                if c.should_abort() {
                    warn!("Walker quit early");
                    return ignore::WalkState::Quit;
                }

                WalkState::Continue
            };

            Box::new(f)
        });

        if c.should_abort() {
            return None;
        }

        drop(tx);
        node.merge(Node::merge_all(rx.iter()));

        let files = c.files();
        let folders = c.folders();
        let symlinks = c.symlinks();
        let elapsed = start.elapsed();

        info!(
            "Walker finished [{elapsed:?}]. [{files} files] [{folders} folders] [{symlinks} symlinks]",
        );

        c.finished.store(true, Ordering::Release);
        Some(node)
    }

    pub fn finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.stop_token.store(true, Ordering::Relaxed)
    }

    pub fn should_abort(&self) -> bool {
        self.stop_token.load(Ordering::Relaxed)
    }

    pub fn files(&self) -> u64 {
        self.files.load(Ordering::Relaxed)
    }

    pub fn folders(&self) -> u64 {
        self.folders.load(Ordering::Relaxed)
    }

    pub fn symlinks(&self) -> u64 {
        self.symlinks.load(Ordering::Relaxed)
    }

    pub fn total_size(&self) -> u64 {
        self.total_size.load(Ordering::Relaxed)
    }
}

impl std::ops::Deref for Walker {
    type Target = WalkerInner;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

struct Flush {
    tx: Sender<Node>,
    model: Node,
}
impl Drop for Flush {
    fn drop(&mut self) {
        let model = std::mem::replace(&mut self.model, Node::root());
        let _ = self.tx.send(model);
    }
}
// How to efficiently exchange the blocks
