use crate::{
    DevicesBridge, NectanWindow, TransfersBridge, devices::DevicesModel, transfers::TransfersModel,
};
use arboard::{Clipboard, Get};
use ignore::{IncrementalMatch, WalkBuilder};
use nectan_core::{transfers::Transfers, walker::Walker};
use slint::{Global, ModelRc};
use std::{
    cell::{Cell, OnceCell, RefCell, RefMut},
    cmp::Ordering,
    collections::HashSet,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
    },
    time::{Duration, Instant},
};
use tracing::info;

pub struct OutcomingTransfer {
    pub walker: Mutex<Option<Walker>>,
    pub total: AtomicU64,
    pub ready: AtomicBool,
    pub match_builder: Mutex<Option<WalkBuilder>>,
}

const MAX_HISTORY: usize = 256;

#[derive(Default)]
pub struct ExplorerHistory {
    cursor: usize,
    history: Vec<PathBuf>,
}
impl ExplorerHistory {
    fn push(&mut self, p: PathBuf) {
        println!("Adding to history {p:?}");
        if self.history.get(self.cursor) == Some(&p) {
            println!("Skipped");
            return;
        }
        if !self.history.is_empty() {
            info!("Truncuated history.");
            self.history.truncate(self.cursor + 1);
        }
        self.history.push(p);
        if self.history.len() > MAX_HISTORY {
            info!("Truncuated history.");
            self.history.remove(0);
        }
        self.cursor = self.history.len();
    }

    fn back(&mut self) -> Option<PathBuf> {
        println!("Self cursor: {}, len {}", self.cursor, self.history.len());
        if self.cursor > 0 {
            self.cursor -= 1;
            return self.history.get(self.cursor).cloned();
        }
        None
    }

    fn forward(&mut self) -> Option<PathBuf> {
        println!("Self cursor: {}, len {}", self.cursor, self.history.len());
        if self.cursor + 1 < self.history.len() {
            self.cursor += 1;
            return self.history.get(self.cursor).cloned();
        }
        None
    }
}

pub struct UiState {
    paths: RefCell<HashSet<PathBuf>>,
    devices_model: Rc<DevicesModel>,
    transfers_model: Rc<TransfersModel>,
    history: RefCell<ExplorerHistory>,
    outcoming_transfer: Arc<OutcomingTransfer>,
    clipboard: RefCell<Clipboard>,
}

impl UiState {
    pub fn devices(&self) -> Rc<DevicesModel> {
        self.devices_model.clone()
    }

    pub fn transfers(&self) -> Rc<TransfersModel> {
        self.transfers_model.clone()
    }

    pub fn walker(&self) -> MutexGuard<'_, Option<Walker>> {
        self.outcoming_transfer.walker.lock().unwrap()
    }

    pub fn outcoming_transfer(&self) -> Arc<OutcomingTransfer> {
        self.outcoming_transfer.clone()
    }

    pub fn add_path(&self, p: PathBuf) {
        self.history.borrow_mut().push(p);
    }

    pub fn last_path(&self) -> Option<PathBuf> {
        self.history.borrow_mut().back()
    }

    pub fn next_path(&self) -> Option<PathBuf> {
        self.history.borrow_mut().forward()
    }

    pub fn insert_path(&self, p: PathBuf) {
        self.paths.borrow_mut().insert(p);
    }

    pub fn take_paths(&self) -> Vec<PathBuf> {
        self.paths.replace(HashSet::new()).drain().collect()
    }
    pub fn get_clipboard(&self) -> RefMut<'_, Clipboard> {
        self.clipboard.borrow_mut()
    }
}

thread_local! {
    static UI: OnceCell<Rc<UiState>> = OnceCell::new();
}

pub fn ui_state() -> Rc<UiState> {
    UI.with(|c| {
        c.get()
            .expect("UiState not initialized on this thread")
            .clone()
    })
}

pub fn init_app_state(w: &NectanWindow, transfers: Arc<Transfers>) {
    let devices_model = Rc::new(DevicesModel::new());
    let transfers_model = Rc::new(TransfersModel::new(transfers));

    DevicesBridge::get(w).set_devices(ModelRc::from(devices_model.clone()));
    TransfersBridge::get(w).set_transfers(ModelRc::from(transfers_model.clone()));

    let ui = Rc::new(UiState {
        history: RefCell::new(ExplorerHistory::default()),
        paths: RefCell::new(HashSet::new()),
        devices_model,
        transfers_model,
        clipboard: RefCell::new(
            Clipboard::new().expect("Failed to initialize clipboard instance."),
        ),
        outcoming_transfer: Arc::new(OutcomingTransfer {
            ready: AtomicBool::default(),
            total: AtomicU64::default(),
            walker: Mutex::default(),
            match_builder: Mutex::default(),
        }),
    });

    UI.with(|c| c.set(ui).ok().expect("UiState already initialized."));

    println!("NectanAppState Initialized.");
}
