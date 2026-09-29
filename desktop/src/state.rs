use crate::{
    DevicesBridge, NectanWindow, TransfersBridge, devices::DevicesModel, transfers::TransfersModel,
};
use nectan_core::walker::Walker;
use slint::{Global, ModelRc};
use std::{
    cell::{Cell, OnceCell, RefCell},
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

pub struct UiState {
    total: Arc<AtomicU64>,
    ready: Arc<AtomicBool>,
    // paths: Arc<Mutex<Vec<PathBuf>>>,
    paths: Rc<RefCell<HashSet<PathBuf>>>,
    devices_model: Rc<DevicesModel>,
    transfers_model: Rc<TransfersModel>,
    walker: Arc<Mutex<Option<Walker>>>,
}

impl UiState {
    pub fn devices(&self) -> Rc<DevicesModel> {
        self.devices_model.clone()
    }

    pub fn transfers(&self) -> Rc<TransfersModel> {
        self.transfers_model.clone()
    }

    pub fn walker(&self) -> MutexGuard<'_, Option<Walker>> {
        self.walker.lock().unwrap()
    }

    pub fn get_walker(&self) -> Arc<Mutex<Option<Walker>>> {
        self.walker.clone()
    }

    pub fn get_total(&self) -> Arc<AtomicU64> {
        self.total.clone()
    }

    pub fn insert_path(&self, p: PathBuf) {
        self.paths.borrow_mut().insert(p);
    }

    pub fn take_paths(&self) -> Vec<PathBuf> {
        self.paths.replace(HashSet::new()).drain().collect()
    }

    pub fn get_ready(&self) -> Arc<AtomicBool> {
        self.ready.clone()
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

pub fn init_app_state(w: &NectanWindow) {
    let devices_model = Rc::new(DevicesModel::new());
    let transfers_model = Rc::new(TransfersModel::new());

    DevicesBridge::get(w).set_devices(ModelRc::from(devices_model.clone()));
    TransfersBridge::get(w).set_transfers(ModelRc::from(transfers_model.clone()));

    let ui = Rc::new(UiState {
        total: Arc::new(AtomicU64::new(0)),
        ready: Arc::new(AtomicBool::new(false)),
        paths: Rc::new(RefCell::new(HashSet::new())),
        devices_model,
        transfers_model,
        walker: Arc::new(Mutex::new(None)),
    });

    UI.with(|c| c.set(ui).ok().expect("UiState already initialized."));

    println!("NectanAppState Initialized.");
}
