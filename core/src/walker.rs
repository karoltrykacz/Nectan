use crate::path_tree::{CompressedPathTree, PathTree};
use ignore::WalkBuilder;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{Sender, channel},
    },
    time::Instant,
};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

pub struct GitIgnoreSettings {}

pub struct WalkControllerInner {
    pub ignore_hidden_files: bool,
    pub total_entries: AtomicU64,
    pub total_size: AtomicU64,
    pub stop_token: AtomicBool,
    pub finished: AtomicBool,
    /// The initial selected paths. May contain both paths and files
    pub paths: Vec<PathBuf>,
    /// Processed path tree / to be sent to remote device
    pub tree: Mutex<Option<CompressedPathTree>>,
}

#[derive(Clone)]
pub struct WalkController(Arc<WalkControllerInner>);

impl WalkController {
    pub fn finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.stop_token.store(true, Ordering::Relaxed)
    }

    pub fn should_abort(&self) -> bool {
        self.stop_token.load(Ordering::Relaxed)
    }

    pub fn take_tree(&self) -> Option<CompressedPathTree> {
        self.tree.lock().unwrap().take()
    }

    pub fn total_entries(&self) -> u64 {
        self.total_entries.load(Ordering::Relaxed)
    }

    pub fn total_size(&self) -> u64 {
        self.total_size.load(Ordering::Relaxed)
    }

    pub fn new(paths: Vec<PathBuf>, ignore_hidden_files: bool, respect_gitignore: bool) -> Self {
        WalkController(Arc::new(WalkControllerInner {
            ignore_hidden_files,
            // respect_gitignore,
            total_entries: AtomicU64::new(0),
            total_size: AtomicU64::new(0),
            stop_token: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            tree: Mutex::new(None),
            paths,
        }))
    }
}

impl std::ops::Deref for WalkController {
    type Target = WalkControllerInner;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

struct Flush {
    tx: Sender<PathTree>,
    tree: PathTree,
}
impl Drop for Flush {
    fn drop(&mut self) {
        let _ = self.tx.send(std::mem::take(&mut self.tree));
    }
}

pub fn start_walker(c: WalkController) {
    std::thread::spawn(move || {
        let start = Instant::now();
        let mut tree = PathTree::new();
        for p in c.paths.iter() {
            let (tx, rx) = channel::<PathTree>();
            let walker = WalkBuilder::new(p)
                .hidden(c.ignore_hidden_files)
                // .git_ignore(c.respect_gitignore)
                // .git_global(c.respect_gitignore)
                // .git_exclude(c.respect_gitignore)
                .build_parallel();

            walker.run(|| {
                let c = c.clone();
                let mut flush = Flush {
                    tx: tx.clone(),
                    tree: PathTree::new(),
                };
                Box::new(move |r| {
                    if let Ok(entry) = r
                        && let Ok(meta) = entry.metadata()
                    {
                        let path = entry.into_path();
                        let is_file = meta.is_file();

                        flush.tree.insert(&path, is_file);
                        c.total_entries.fetch_add(1, Ordering::Relaxed);

                        #[cfg(unix)]
                        {
                            c.total_size.fetch_add(meta.size(), Ordering::Relaxed);
                        }

                        #[cfg(not(unix))]
                        {
                            c.total_size.fetch_add(meta.len(), Ordering::Relaxed);
                        }
                    }
                    if c.should_abort() {
                        return ignore::WalkState::Quit;
                    }
                    ignore::WalkState::Continue
                })
            });
            if c.should_abort() {
                return;
            }
            drop(tx);
            tree.merge(PathTree::merge_all(rx.iter()));
        }

        let compress = Instant::now();
        let compressed = tree.compress();
        *c.tree.lock().unwrap() = Some(compressed);
        let compress_time = compress.elapsed();

        tracing::trace!(
            "Walker finished [{:?}]. Compressed in [{:?}]",
            start.elapsed(),
            compress_time
        );
        c.finished.store(true, Ordering::Release);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_walker() {
        let root = "/home/karol";
        let mut tree = PathTree::new();

        let (tx, rx) = channel::<PathTree>();
        let walker = WalkBuilder::new(root).build_parallel();

        walker.run(|| {
            let mut flush = Flush {
                tx: tx.clone(),
                tree: PathTree::new(),
            };
            Box::new(move |r| {
                if let Ok(entry) = r
                    && let Ok(meta) = entry.metadata()
                {
                    let path = entry.into_path();
                    let is_file = meta.is_file();
                    flush.tree.insert(&path, is_file);
                }
                ignore::WalkState::Continue
            })
        });
        drop(tx);
        tree.merge(PathTree::merge_all(rx.iter()));

        let count_start = Instant::now();
        let x = tree.count();
        println!("Count took [{:?}] Result [{x}]", count_start.elapsed(),);

        let serialized = tree.serialize();
        let raw: usize = tree
            .to_vec()
            .iter()
            .map(|t| t.0.to_string_lossy().len())
            .sum();
        let vectree = tree.to_vec();
        println!("PATHS NUM [{}]", vectree.len());
        let mut bytes = String::new();
        for p in vectree {
            bytes += &p.0.to_string_lossy();
        }

        let comp_tree = tree.compress();

        let comp_vec_size = zstd::encode_all(bytes.as_bytes(), 0).unwrap().len();
        let comp_tree_size = comp_tree.len();

        println!(
            "SERIALIZED {} RAW {} COMPRESSED SERIALIZED {} COMPRESSED RAW {}",
            serialized.len(),
            raw,
            comp_tree_size,
            comp_vec_size,
        );
    }
}
