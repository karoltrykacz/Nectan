use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{
            AtomicBool, AtomicU32, AtomicU64,
            Ordering::{self, SeqCst},
        },
        mpsc::{Sender, channel},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use tracing::info;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

use crate::common::common_parent;

pub struct GitIgnoreSettings {}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransferEntry {
    pub path: PathBuf,
    pub size: u64,
    pub is_dir: bool,
}

pub struct WalkControllerInner {
    pub total_entries: AtomicU32,
    pub total_size: AtomicU64,
    pub stop_token: AtomicBool,
    pub finished: AtomicBool,
    /// The initial selected paths. May contain mix of files, folders and symlinks
    pub paths: Vec<PathBuf>,
    /// Common parent of provided paths
    pub root_path: PathBuf,
    /// Result paths
    pub entries: Mutex<Option<Vec<TransferEntry>>>,
}

pub fn merge_all<T>(s: &mut Vec<T>, vecs: impl IntoIterator<Item = Vec<T>>) {
    for v in vecs {
        s.extend(v);
    }
}

#[derive(Clone)]
pub struct Walker(Arc<WalkControllerInner>);

impl Walker {
    pub fn finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }

    pub fn should_abort(&self) -> bool {
        self.stop_token.load(Ordering::SeqCst)
    }

    pub fn take_entries(&self) -> Option<Vec<TransferEntry>> {
        self.entries.lock().unwrap().take()
    }

    pub fn total_entries(&self) -> u32 {
        self.total_entries.load(Ordering::Relaxed)
    }

    pub fn total_size(&self) -> u64 {
        self.total_size.load(Ordering::Relaxed)
    }

    pub fn new(paths: Vec<PathBuf>) -> Self {
        Walker(Arc::new(WalkControllerInner {
            total_entries: AtomicU32::new(0),
            total_size: AtomicU64::new(0),
            stop_token: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            entries: Mutex::new(None),
            root_path: common_parent(&paths),
            paths,
        }))
    }

    pub fn rescan(&self) {
        if self.finished.load(SeqCst) {
            // Walker still not finished
            // Stop it and wait until it rearms itself so we can start
            self.stop();

            loop {
                if self.should_abort() {
                    std::thread::sleep(Duration::from_micros(1));
                    continue;
                } else {
                    break;
                }
            }
        }

        self.rearm();
        self.walk();
    }

    pub fn stop(&self) {
        self.stop_token.store(true, SeqCst)
    }

    fn rearm(&self) {
        self.total_entries.store(0, SeqCst);
        self.total_size.store(0, SeqCst);
        self.finished.store(false, SeqCst);
        self.stop_token.store(false, SeqCst);
    }

    pub fn walk(&self) -> JoinHandle<()> {
        let c = self.clone();
        std::thread::spawn(move || {
            let start = Instant::now();
            let mut entries = Vec::new();
            for p in c.paths.iter() {
                let (tx, rx) = channel::<Vec<TransferEntry>>();
                let walker = WalkBuilder::new(p).build_parallel();

                walker.run(|| {
                    let c = c.clone();
                    let mut flush = Flush {
                        tx: tx.clone(),
                        inner: Vec::new(),
                        _next_id: 0,
                    };
                    Box::new(move |r| {
                        if let Ok(entry) = r
                            && let Ok(meta) = entry.metadata()
                        {
                            let is_dir = meta.is_dir();
                            let path = entry.into_path();

                            #[cfg(unix)]
                            let size = meta.size();

                            #[cfg(not(unix))]
                            let size = meta.len();

                            let entry = TransferEntry { size, path, is_dir };
                            flush.inner.push(entry);

                            c.total_entries.fetch_add(1, Ordering::Relaxed);
                            c.total_size.fetch_add(size, Ordering::Relaxed);
                        }
                        if c.should_abort() {
                            c.rearm();
                            return ignore::WalkState::Quit;
                        }
                        ignore::WalkState::Continue
                    })
                });
                if c.should_abort() {
                    c.rearm();
                    return;
                }
                drop(tx);
                merge_all(&mut entries, rx.iter());
            }

            *c.entries.lock().unwrap() = Some(entries);

            info!("Walker finished [{:?}]. ", start.elapsed(),);

            if c.should_abort() {
                c.rearm();
                return;
            }
            c.finished.store(true, SeqCst);
        })
    }
}

impl std::ops::Deref for Walker {
    type Target = WalkControllerInner;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

struct Flush<T>
where
    T: Default,
{
    tx: Sender<T>,
    inner: T,
    _next_id: u32,
}
impl<T: Default> Drop for Flush<T> {
    fn drop(&mut self) {
        let _ = self.tx.send(std::mem::take(&mut self.inner));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_walker() {
        let root = "/home/karol";
        let mut entries = Vec::new();

        let (tx, rx) = channel::<Vec<PathBuf>>();
        let walker = WalkBuilder::new(root).build_parallel();

        walker.run(|| {
            let mut flush = Flush {
                tx: tx.clone(),
                inner: Vec::new(),
                _next_id: 0,
            };
            Box::new(move |r| {
                if let Ok(entry) = r {
                    let path = entry.into_path();
                    flush.inner.push(path);
                }
                ignore::WalkState::Continue
            })
        });
        drop(tx);

        merge_all(&mut entries, rx.iter());

        let count_start = Instant::now();
        let x = entries.len();

        println!("Count took [{:?}] Result [{x}]", count_start.elapsed(),);
    }
}
