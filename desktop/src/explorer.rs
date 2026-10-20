use std::{
    borrow::BorrowMut,
    cell::{Cell, Ref, RefCell},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    rc::Rc,
};

use crate::{
    DirEntry, EntryType, ExplorerBridge, NectanWindow, handlers::show_error,
    sidebar::TileAction, state::ui_state,
};
use nectan_core::format::BinaryBytes;
use slint::{
    ComponentHandle, Model, ModelNotify, ModelRc, SharedString, ToSharedString,
    Weak,
};
use tracing::{Instrument, info, warn};

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

        Some(DirEntry {
            name: SharedString::from(
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .as_ref(),
            ),
            size: BinaryBytes(meta.size()).to_string().to_shared_string(),
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
    let weak = w.as_weak();
    b.on_open(move |entry, id| {
        let e = ui_state().explorer();
        if entry.r#type == EntryType::Directory {
            let path = e.paths().get(id as usize).unwrap().to_owned();
            e.open_dir(weak.clone(), path);
        }
    });

    let weak = w.as_weak();
    b.on_back(move || {
        let e = ui_state().explorer();
        if let Some(p) = e.history.borrow_mut().back() {
            e.open_dir(weak.clone(), p);
        }
    });

    let weak = w.as_weak();
    b.on_forward(move || {
        let e = ui_state().explorer();
        if let Some(p) = e.history.borrow_mut().forward() {
            e.open_dir(weak.clone(), p);
        }
    });

    let weak = w.as_weak();
    b.on_up(move || {
        let e = ui_state().explorer();
        let path = {
            let mut current = e.current.borrow_mut();
            current.pop();
            current.clone()
        };
        e.open_dir(weak.clone(), path);
    });

    let weak = w.as_weak();
    b.on_builtin(move |builtin| {
        let Some(w) = weak.upgrade() else { return };
        TileAction::Builtin(builtin.into()).execute(&w);
    });

    let weak = w.as_weak();
    b.on_rename(move |idx, new_name| {
        let w = weak.clone();
        let e = ui_state().explorer();
        let paths = e.paths();
        let path = paths.get(idx as usize).unwrap();
        let new_path = path.with_file_name(new_name.as_str());

        if new_path == *path {
            return;
        }

        if new_path.exists() {
            show_error(
                &w,
                "Failed to rename",
                format!("File {} already exists.", new_path.display()),
            );
            return;
        }

        match std::fs::rename(path, &new_path) {
            Ok(()) => {
                println!("Renamed {} to {}", path.display(), new_path.display())
            }
            Err(e) => {
                show_error(&w, "Failed to rename", format!("{}", e));
            }
        }

        // Todo update only one path?
        e.open_dir(w, new_path.parent().unwrap());
    });

    let weak = w.as_weak();
    b.on_refresh(move || {
        let e = ui_state().explorer();
        let path = e.current.borrow().clone();
        e.open_dir(weak.clone(), path);
    });

    b.on_get_dot_pos(move |idx| {
        let e = ui_state().explorer();
        let paths = e.paths();
        let filename = paths
            .get(idx as usize)
            .unwrap()
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        filename.rfind('.').unwrap_or(filename.len()) as i32
    });
}

async fn read_dir(p: impl AsRef<Path>) -> std::io::Result<Vec<PathBuf>> {
    let mut dir = tokio::fs::read_dir(p).await?;
    let mut items = Vec::new();
    while let Some(e) = dir.next_entry().await? {
        items.push(e.path());
    }
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
