use std::{
    cell::{Cell, Ref, RefCell},
    io::Cursor,
    path::{Path, PathBuf},
    rc::Rc,
    time::Instant,
};

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
use tracing::{error, info, warn};
use url::Url;
use zip::{ZipWriter, write::SimpleFileOptions};

pub struct PathModel<D> {
    paths: RefCell<Vec<PathBuf>>,
    notify: ModelNotify,
    map: fn(&Path) -> Option<D>,
}

impl<D> PathModel<D> {
    pub fn new(map: fn(&Path) -> Option<D>) -> Self {
        Self {
            paths: RefCell::new(Vec::new()),
            notify: ModelNotify::default(),
            map,
        }
    }

    pub fn set(&self, items: Vec<PathBuf>) {
        *self.paths.borrow_mut() = items;
        self.notify.reset();
    }

    pub fn paths(&self) -> Ref<'_, Vec<PathBuf>> {
        self.paths.borrow()
    }
}

impl<D: 'static> Model for PathModel<D> {
    type Data = D;

    fn row_count(&self) -> usize {
        self.paths.borrow().len()
    }

    fn row_data(&self, row: usize) -> Option<D> {
        let paths = self.paths.borrow();
        (self.map)(paths.get(row)?)
    }

    fn model_tracker(&self) -> &dyn slint::ModelTracker {
        &self.notify
    }
}

fn dir_entry(path: &Path) -> Option<DirEntry> {
    let meta = std::fs::metadata(path).ok()?;
    let etype = meta.file_type();
    let r#type = if etype.is_dir() {
        EntryType::Directory
    } else if etype.is_file() {
        EntryType::File
    } else {
        EntryType::Symlink
    };

    Some(DirEntry {
        name: SharedString::from(
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
        ),
        size: BinaryBytes(meta.len()).to_string().to_shared_string(),
        r#type,
        ..Default::default()
    })
}

fn crumb_label(p: &Path) -> Option<SharedString> {
    Some(match p.file_name() {
        Some(name) => name.to_string_lossy().as_ref().into(),
        None => p.display().to_string().into(),
    })
}

fn crumb_paths(p: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = p.ancestors().map(Path::to_path_buf).collect();
    v.reverse();
    v
}

// Search results should not copy the items, instead create a bit set that will indicate which
// results fit the query???

pub struct Explorer {
    current: RefCell<PathBuf>,
    search_results: RefCell<Vec<bool>>,
    items: Rc<PathModel<DirEntry>>,
    crumbs: Rc<PathModel<SharedString>>,
    token: Cell<u64>,
    history: RefCell<History>,
}

impl Explorer {
    pub fn new() -> Rc<Self> {
        Rc::new(Explorer {
            current: RefCell::new(dirs::home_dir().unwrap()),
            search_results: RefCell::new(Vec::new()),
            items: Rc::new(PathModel::new(dir_entry)),
            crumbs: Rc::new(PathModel::new(crumb_label)),
            token: Cell::new(0),
            history: RefCell::new(History::default()),
        })
    }

    pub fn model(&self) -> ModelRc<DirEntry> {
        ModelRc::from(Rc::clone(&self.items))
    }

    pub fn crumb_model(&self) -> ModelRc<SharedString> {
        ModelRc::from(Rc::clone(&self.crumbs))
    }

    pub fn paths(&self) -> Ref<'_, Vec<PathBuf>> {
        self.items.paths()
    }

    pub fn open_dir<P>(self: &Rc<Self>, w: Weak<NectanWindow>, p: P)
    where
        P: Into<PathBuf>,
    {
        let p = p.into();
        info!("Opening {}", p.display());
        let this = Rc::clone(self);
        let token = this.token.get() + 1;
        this.token.set(token);
        this.crumbs.set(crumb_paths(&p));

        slint::spawn_local(async move {
            let result = tokio::spawn(read_dir(p.clone())).await;

            if this.token.get() != token {
                warn!("Token changed");
                return;
            }

            match result {
                Ok(Ok(items)) => {
                    info!("Opened {} ({} entries)", p.display(), items.len());
                    *this.current.borrow_mut() = p.clone();
                    this.items.set(items);
                    this.history.borrow_mut().push(p);
                }
                Ok(Err(e)) => {
                    let cur = this.current.borrow().clone();
                    this.crumbs.set(crumb_paths(&cur));
                    show_error(
                        &w,
                        "Error",
                        format!("Failed to open {}. {}", p.display(), e),
                    );
                }
                Err(e) => {
                    let cur = this.current.borrow().clone();
                    this.crumbs.set(crumb_paths(&cur));
                    warn!("read_dir task failed: {e}");
                }
            }
        })
        .unwrap();
    }
}

pub fn handle_explorer(w: &NectanWindow) {
    let b = w.global::<ExplorerBridge>();
    let explorer = ui_state().explorer();
    let weak = w.as_weak();

    b.set_current_dir(explorer.crumb_model());

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
            } else {
            }
        }
    });

    b.on_navigate_to_in_current({
        let (e, weak) = (explorer.clone(), weak.clone());
        move |index| {
            let target = e.crumbs.paths().get(index as usize).cloned();
            if let Some(p) = target {
                e.open_dir(weak.clone(), p);
            } else {
                warn!("Target not found");
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
            let parent = e.current.borrow().parent().map(Path::to_path_buf);
            if let Some(parent) = parent {
                e.open_dir(weak.clone(), parent);
            }
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
                Ok(()) => info!(
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

        move |start, end, name, encrypt, password| {
            let paths: Vec<PathBuf> = e.paths()[sel_range(start, end)].to_vec();
            let dir = e.current.borrow().clone();

            let password_str = password.to_string();
            let password_static: &'static str =
                Box::leak(password_str.into_boxed_str());

            info!("Zipping {start} = {end}");
            info!("Zipping {paths:?}");

            let result: Result<(), Box<dyn std::error::Error>> = (|| {
                let buf = Cursor::new(Vec::new());
                let mut zip = ZipWriter::new(buf);
                let mut options = SimpleFileOptions::default();

                if encrypt {
                    options = options.with_aes_encryption(
                        zip::AesMode::Aes256,
                        password_static,
                    );
                }

                // Recursive helper function for files and folders
                fn add_path<W: std::io::Write + std::io::Seek>(
                    zip: &mut ZipWriter<W>,
                    path: &Path,
                    base_path: &Path,
                    options: SimpleFileOptions,
                ) -> Result<(), Box<dyn std::error::Error>> {
                    let relative = match path.strip_prefix(base_path) {
                        Ok(r) => r,
                        Err(_) => return Ok(()),
                    };

                    let name_str =
                        relative.to_str().ok_or("Invalid path encoding")?;
                    if name_str.is_empty() {
                        return Ok(());
                    }

                    if path.is_dir() {
                        zip.add_directory(name_str, options)?;
                        for entry in std::fs::read_dir(path)? {
                            let entry = entry?;
                            add_path(zip, &entry.path(), base_path, options)?;
                        }
                    } else if path.is_file() {
                        zip.start_file(name_str, options)?;
                        let mut file = std::fs::File::open(path)?;
                        std::io::copy(&mut file, zip)?;
                    }

                    Ok(())
                }

                for path in &paths {
                    info!("Zipping recursive: {path:?}");
                    if let Some(base_path) = path.parent() {
                        add_path(&mut zip, path, base_path, options)?;
                    }
                }

                let cursor = zip.finish()?;
                let data = cursor.into_inner();

                let output_path = dir.join(name);
                std::fs::write(output_path, data)?;

                Ok(())
            })(
            );

            if let Err(err) = result {
                error!("Failed to compress items: {err}");
            }

            e.open_dir(weak.clone(), dir);
        }
    });

    b.on_search_current({
        let (e, weak) = (explorer.clone(), weak.clone());
        move |query| {
            info!("Searching current");
            // let target = e.history.borrow_mut().forward();
            // if let Some(p) = target {
            //     e.open_dir(weak.clone(), p);
            // }
        }
    });

    b.on_clear_search({
        let (e, weak) = (explorer.clone(), weak.clone());
        move || {
            info!("Cleared search results");
        }
    });

    b.on_terminal({
        let (e, weak) = (explorer.clone(), weak.clone());
        move || {
            let dir = e.current.borrow().clone();
            use std::process::Command;
            Command::new("gnome-terminal")
                .current_dir(dir)
                .spawn()
                .expect("Failed to start terminal");
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
    info!("read_dir took {:?}", start.elapsed());
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
        if self.history.get(self.cursor) == Some(&p) {
            return;
        }

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
        Ok(url) if url.scheme().len() == 1 => Some(PathBuf::from(s)),
        Ok(_) => None,
        Err(_) => Some(PathBuf::from(s)),
    }
}
