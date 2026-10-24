use std::io::Write;
use std::{
    borrow::BorrowMut,
    cell::{Cell, Ref, RefCell},
    io::Cursor,
    path::{Path, PathBuf},
    rc::Rc,
    time::Instant,
};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt as WindowsMetadataExt;

use crate::{
    DirEntry, EntryType, ExplorerBridge, NectanWindow, handlers::show_error,
    sidebar::TileAction, state::ui_state,
};
use clipboard_rs::Clipboard;
use nectan_core::format::BinaryBytes;
use slint::{
    ComponentHandle, Model, ModelNotify, ModelRc, SharedString, ToSharedString,
    Weak,
};
use tracing::{Instrument, info, warn};
use url::Url;
use zip::{AesMode, ZipArchive, ZipWriter, write::SimpleFileOptions};

pub struct Items {
    paths: RefCell<Vec<PathBuf>>,
    notify: ModelNotify,
}

impl Items {
    pub fn new() -> Self {
        Items {
            paths: RefCell::new(Vec::new()),
            notify: ModelNotify::default(),
        }
    }

    /// Replace the contents and tell Slint the whole model changed.
    pub fn set(&self, items: Vec<PathBuf>) {
        *self.paths.borrow_mut() = items;
        self.notify.reset();
    }
}

impl Model for Items {
    type Data = DirEntry;

    fn row_count(&self) -> usize {
        self.paths.borrow().len()
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        let items = self.paths.borrow();
        let path = items.get(row)?;
        let meta = std::fs::metadata(path).ok()?;
        let etype = meta.file_type();
        let r#type = if etype.is_dir() {
            EntryType::Directory
        } else if etype.is_file() {
            EntryType::File
        } else {
            EntryType::Symlink
        };

        #[cfg(target_family = "unix")]
        let size = meta.size();

        #[cfg(not(target_family = "unix"))]
        let size = meta.len();

        Some(DirEntry {
            name: SharedString::from(
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .as_ref(),
            ),
            size: BinaryBytes(size).to_string().to_shared_string(),
            r#type,
            ..Default::default()
        })
    }

    fn model_tracker(&self) -> &dyn slint::ModelTracker {
        &self.notify
    }
}

pub struct Explorer {
    current: RefCell<PathBuf>,
    items: Rc<Items>,
    token: Cell<u64>,
    history: RefCell<History>,
}

impl Explorer {
    pub fn new() -> Rc<Self> {
        Rc::new(Explorer {
            current: RefCell::new(dirs::home_dir().unwrap()),
            items: Rc::new(Items::new()),
            token: Cell::new(0),
            history: RefCell::new(History::default()),
        })
    }

    pub fn model(&self) -> ModelRc<DirEntry> {
        ModelRc::from(Rc::clone(&self.items))
    }

    pub fn paths(&self) -> Ref<'_, Vec<PathBuf>> {
        self.items.paths.borrow()
    }

    pub fn open_dir<P>(self: &Rc<Self>, w: Weak<NectanWindow>, p: P)
    where
        P: Into<PathBuf>,
    {
        let p = p.into();
        let this = Rc::clone(self);
        let token = this.token.get() + 1;

        w.upgrade()
            .unwrap()
            .global::<ExplorerBridge>()
            .set_current_dir(p.display().to_string().into());

        this.token.set(token);
        *this.current.borrow_mut() = p.clone();

        slint::spawn_local(async move {
            let result = tokio::spawn(read_dir(p.clone())).await;

            // let result = tokio::spawn(async move {
            //     let mut dir = tokio::fs::read_dir(p).await?;
            //     let mut items = Vec::new();
            //
            //     while let Some(e) = dir.next_entry().await? {
            //         items.push(e.path());
            //         // if items.len()
            //     }
            //     Ok(items)
            // });

            if this.token.get() != token {
                warn!("Token changed");
                return;
            }

            match result {
                Ok(Ok(items)) => {
                    println!("Opened {}", items.len());
                    this.items.set(items);
                    this.history.borrow_mut().push(p);
                }
                Ok(Err(e)) => show_error(
                    &w,
                    "Error",
                    format!("Failed to open {}. {}", p.display(), e),
                ),
                Err(e) => warn!("read_dir task failed: {e}"),
            }
        })
        .unwrap();
    }

    pub fn add_path(&mut self, p: PathBuf) {
        self.history.borrow_mut().push(p);
    }

    pub fn last_path(&mut self) -> Option<PathBuf> {
        self.history.borrow_mut().back()
    }

    pub fn next_path(&mut self) -> Option<PathBuf> {
        self.history.borrow_mut().forward()
    }
}

pub fn handle_explorer(w: &NectanWindow) {
    let b = w.global::<ExplorerBridge>();
    let explorer = ui_state().explorer();
    let weak = w.as_weak();

    fn sel_range(start: i32, end: i32) -> std::ops::RangeInclusive<usize> {
        let start = if start == -1 { end } else { start };
        start as usize..=end as usize
    }

    b.on_open({
        let (e, weak) = (explorer.clone(), weak.clone());
        move |entry, id| {
            if entry.r#type == EntryType::Directory {
                let path = e.paths().get(id as usize).unwrap().to_owned();
                e.open_dir(weak.clone(), path);
            }
        }
    });

    b.on_back({
        let (e, weak) = (explorer.clone(), weak.clone());
        move || {
            let target = e.history.borrow_mut().back();
            if let Some(p) = target {
                e.open_dir(weak.clone(), p);
            }
        }
    });

    b.on_forward({
        let (e, weak) = (explorer.clone(), weak.clone());
        move || {
            let target = e.history.borrow_mut().forward();
            if let Some(p) = target {
                e.open_dir(weak.clone(), p);
            }
        }
    });

    b.on_up({
        let (e, weak) = (explorer.clone(), weak.clone());
        move || {
            let path = {
                let mut current = e.current.borrow_mut();
                current.pop();
                current.clone()
            };
            e.open_dir(weak.clone(), path);
        }
    });

    b.on_builtin({
        let weak = weak.clone();
        move |builtin| {
            let Some(w) = weak.upgrade() else { return };
            TileAction::Builtin(builtin.into()).execute(&w);
        }
    });

    b.on_rename({
        let (e, weak) = (explorer.clone(), weak.clone());
        move |idx, new_name| {
            let path = e.paths().get(idx as usize).unwrap().to_owned();
            let new_path = path.with_file_name(new_name.as_str());

            if new_path == path {
                return;
            }
            if new_path.exists() {
                show_error(
                    &weak,
                    "Failed to rename",
                    format!("File {} already exists.", new_path.display()),
                );
                return;
            }

            match std::fs::rename(&path, &new_path) {
                Ok(()) => println!(
                    "Renamed {} to {}",
                    path.display(),
                    new_path.display()
                ),
                Err(err) => {
                    show_error(&weak, "Failed to rename", err.to_string())
                }
            }
            e.open_dir(weak.clone(), new_path.parent().unwrap());
        }
    });

    b.on_refresh({
        let (e, weak) = (explorer.clone(), weak.clone());
        move || {
            let path = e.current.borrow().clone();
            e.open_dir(weak.clone(), path);
        }
    });

    b.on_get_dot_pos({
        let e = explorer.clone();
        move |idx| {
            let paths = e.paths();
            let filename = paths
                .get(idx as usize)
                .unwrap()
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            filename.rfind('.').unwrap_or(filename.len()) as i32
        }
    });

    b.on_delete_items({
        let (e, weak) = (explorer.clone(), weak.clone());
        move |start, end| {
            // TODO: make this an async operation
            for path in &e.paths()[sel_range(start, end)] {
                let result = match std::fs::metadata(path) {
                    Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
                    Ok(_) => std::fs::remove_file(path),
                    Err(err) => Err(err),
                };
                match result {
                    Ok(()) => info!("Deleted {path:?}"),
                    Err(err) => warn!("Failed to delete {path:?}: {err}"),
                }
            }
            let curr = e.current.borrow().clone();
            e.open_dir(weak.clone(), curr);
        }
    });

    b.on_trash_items({
        let (e, weak) = (explorer.clone(), weak.clone());
        move |start, end| {
            if let Err(err) =
                trash::delete_all(&e.paths()[sel_range(start, end)])
            {
                warn!("Failed to trash items: {err}");
            }
            let curr = e.current.borrow().clone();
            e.open_dir(weak.clone(), curr);
        }
    });

    // TODO: move to clipboard and delete on paste?
    b.on_cut(move |_start, _end| {});

    b.on_paste({
        let (e, weak) = (explorer.clone(), weak.clone());
        move || {
            let dir = e.current.borrow().clone();
            info!("Pasting into {dir:?}");

            let files = match ui_state().clipboard().get_files() {
                Ok(files) => files
                    .iter()
                    .filter_map(|f| uri_to_path(f))
                    .collect::<Vec<_>>(),
                Err(err) => {
                    show_error(
                        &weak,
                        "Clipboard error",
                        format!("Failed to paste: {err}"),
                    );
                    return;
                }
            };
            if files.is_empty() {
                return;
            }

            let mut errors = Vec::new();
            for src in &files {
                let Some(dst) = unique_dest(&dir, src) else {
                    errors.push(format!("{}: invalid path", src.display()));
                    continue;
                };
                match copy_recursive(src, &dst) {
                    Ok(()) => info!("Pasted {src:?} -> {dst:?}"),
                    Err(err) => {
                        errors.push(format!("{}: {err}", src.display()))
                    }
                }
            }

            if !errors.is_empty() {
                show_error(
                    &weak,
                    "Failed to paste some items",
                    errors.join("\n"),
                );
            }

            e.open_dir(weak.clone(), dir);
        }
    });

    b.on_copy({
        let e = explorer.clone();
        move |start, end| {
            info!("Copy from {start} to {end}");
            let files: Vec<String> = e.paths()[sel_range(start, end)]
                .iter()
                .filter_map(|p| p.canonicalize().ok())
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            info!("Copying {} item(s)", files.len());
            let _ = ui_state().clipboard().set_files(files);
        }
    });

    b.on_compress_items({
        let (e, weak) = (explorer.clone(), weak.clone());
        move |start, end, name, password| {
            let dir = e.current.borrow().clone();

            let buf = Cursor::new(Vec::new());
            let mut zip = ZipWriter::new(buf);
            let options = SimpleFileOptions::default()
                .with_aes_encryption(AesMode::Aes256, "Luj");
            let options = SimpleFileOptions::default();
            zip.start_file(name, options).unwrap();
            zip.write_all(b"hello\n").unwrap();

            // let names: Vec<std::ffi::OsString> = e.paths()
            //     [sel_range(start, end)]
            // .iter()
            // .filter_map(|p| p.file_name())
            // .map(|n| {
            //     let mut s = std::ffi::OsString::from("./");
            //     s.push(n);
            //     s
            // })
            // .collect();
            //
            // if names.is_empty() {
            //     return;
            // }
            //
            // // Archive name: "<item>.zip" for one item, "<current dir>.zip" otherwise.
            // let base = if names.len() == 1 {
            //     e.paths()[sel_range(start, end)][0]
            //         .file_name()
            //         .map(|n| n.to_string_lossy().into_owned())
            // } else {
            //     dir.file_name().map(|n| n.to_string_lossy().into_owned())
            // }
            // .unwrap_or_else(|| "archive".to_string());
            //
            // // Don't overwrite (zip would otherwise *update* an existing archive).
            // let mut archive = dir.join(format!("{base}.zip"));
            // let mut n = 1;
            // while archive.exists() {
            //     archive = dir.join(format!("{base} ({n}).zip"));
            //     n += 1;
            // }
            //
            // let result = std::process::Command::new("zip")
            //     .current_dir(&dir)
            //     .arg("-r")
            //     .arg("-q")
            //     .arg(&archive)
            //     .args(&names)
            //     .output();
            //
            // match result {
            //     Ok(out) if out.status.success() => {
            //         info!(
            //             "Compressed {} item(s) into {archive:?}",
            //             names.len()
            //         );
            //     }
            //     Ok(out) => {
            //         let stderr = String::from_utf8_lossy(&out.stderr);
            //         let stdout = String::from_utf8_lossy(&out.stdout);
            //         let msg = if stderr.trim().is_empty() {
            //             stdout
            //         } else {
            //             stderr
            //         };
            //         // Remove a partial archive if zip left one behind.
            //         let _ = std::fs::remove_file(&archive);
            //         show_error(
            //             &weak,
            //             "Failed to compress",
            //             msg.trim().to_string(),
            //         );
            //     }
            //     Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            //         show_error(
            //             &weak,
            //             "zip not installed",
            //             "zip not found. Install it and try again.".to_string(),
            //         );
            //     }
            //     Err(err) => {
            //         show_error(&weak, "Failed to compress", err.to_string());
            //     }
            // }

            e.open_dir(weak.clone(), dir);
        }
    });
}

async fn read_dir(p: impl AsRef<Path>) -> std::io::Result<Vec<PathBuf>> {
    let start = Instant::now();
    let mut dir = tokio::fs::read_dir(p).await?;
    let mut items = Vec::new();

    while let Some(e) = dir.next_entry().await? {
        items.push(e.path());
        if items.len() > 1024 {
            break;
        }
    }
    println!("Took {:?}", start.elapsed());
    Ok(items)
}
const MAX_HISTORY: usize = 256;

#[derive(Default)]
pub struct History {
    cursor: usize,
    history: Vec<PathBuf>,
}

impl History {
    pub fn push(&mut self, p: PathBuf) {
        println!("Current history {:#?}", self.history);
        // Already at this entry (e.g. re-opening the current dir).
        if self.history.get(self.cursor) == Some(&p) {
            return;
        }

        // Drop the forward branch, but only if there is one.
        if self.cursor + 1 < self.history.len() {
            self.history.truncate(self.cursor + 1);
        }

        self.history.push(p);

        if self.history.len() > MAX_HISTORY {
            self.history.remove(0);
        }
        self.cursor = self.history.len() - 1;
    }

    pub fn can_back(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_forward(&self) -> bool {
        self.cursor + 1 < self.history.len()
    }

    pub fn back(&mut self) -> Option<PathBuf> {
        if !self.can_back() {
            return None;
        }
        self.cursor -= 1;
        self.history.get(self.cursor).cloned()
    }

    pub fn forward(&mut self) -> Option<PathBuf> {
        if !self.can_forward() {
            return None;
        }
        self.cursor += 1;
        self.history.get(self.cursor).cloned()
    }
}

fn copy_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        // Copying a directory into itself would recurse forever.
        if dst.starts_with(src) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cannot copy a directory into itself",
            ));
        }
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else {
        std::fs::copy(src, dst)?;
    }
    Ok(())
}

fn unique_dest(dir: &Path, src: &Path) -> Option<PathBuf> {
    let name = src.file_name()?;
    let candidate = dir.join(name);
    if !candidate.exists() {
        return Some(candidate);
    }

    let stem = src.file_stem()?.to_string_lossy();
    let ext = src.extension().map(|e| e.to_string_lossy());
    for n in 1.. {
        let suffix = if n == 1 {
            " (Copy)".to_string()
        } else {
            format!(" (Copy {n})")
        };
        let new_name = match &ext {
            Some(ext) => format!("{stem}{suffix}.{ext}"),
            None => format!("{stem}{suffix}"),
        };
        let candidate = dir.join(new_name);
        if !candidate.exists() {
            return Some(candidate);
        }
    }
    unreachable!()
}
fn uri_to_path(s: &str) -> Option<PathBuf> {
    match Url::parse(s) {
        Ok(url) if url.scheme() == "file" => url.to_file_path().ok(),
        Ok(_) => None,
        Err(_) => Some(PathBuf::from(s)),
    }
}
