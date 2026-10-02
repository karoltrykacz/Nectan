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

pub struct OutcomingTransfer {
    pub walker: Mutex<Option<Walker>>,
    pub total: AtomicU64,
    pub ready: AtomicBool,
    pub match_builder: Mutex<Option<WalkBuilder>>,
}

pub struct UiState {
    // paths: Arc<Mutex<Vec<PathBuf>>>,
    paths: RefCell<HashSet<PathBuf>>,
    devices_model: Rc<DevicesModel>,
    transfers_model: Rc<TransfersModel>,
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
