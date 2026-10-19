use crate::{
    DevicesBridge, NectanWindow, TransfersBridge, devices::DevicesModel,
    explorer::Explorer, sidebar::EssentialsModel, transfers::TransfersModel,
};
use ignore::WalkBuilder;
use nectan_core::{transfers::Transfers, utils::CancelToken, walker::Walker};
use slint::{Global, ModelRc, Timer};
use std::{
    cell::{OnceCell, Ref, RefCell, RefMut},
    collections::HashSet,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU64},
    },
};

pub struct OutcomingTransfer {
    pub walker: Mutex<Option<Walker>>,
    pub total: AtomicU64,
    pub ready: AtomicBool,
    pub match_builder: Mutex<Option<WalkBuilder>>,
}

pub struct UiState {
    paths: RefCell<HashSet<PathBuf>>,
    essentials: Rc<EssentialsModel>,
    explorer: Explorer,
    devices_model: Rc<DevicesModel>,
    transfers_model: OnceCell<Rc<TransfersModel>>,
    outcoming_transfer: Arc<OutcomingTransfer>,
    // clipboard: RefCell<Clipboard>,
    save_destination: RefCell<PathBuf>,
    code_timer: RefCell<Timer>,
}

impl UiState {
    pub fn save_to(&self) -> PathBuf {
        self.save_destination.borrow().clone()
    }

    pub fn set_save_to<P: AsRef<Path>>(&self, path: P) {
        let path = path.as_ref().to_path_buf();
        *self.save_destination.borrow_mut() = path;
    }

    pub fn devices(&self) -> Rc<DevicesModel> {
        self.devices_model.clone()
    }

    pub fn transfers(&self) -> Rc<TransfersModel> {
        self.transfers_model.get().unwrap().clone()
    }

    pub fn timer(&self) -> Ref<'_, Timer> {
        self.code_timer.borrow()
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

    // pub fn get_clipboard(&self) -> RefMut<'_, Clipboard> {
    //     self.clipboard.borrow_mut()
    // }
}

thread_local! {
    static UI: OnceCell<Rc<UiState>> = const { OnceCell::new() };
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

    DevicesBridge::get(w).set_devices(ModelRc::from(devices_model.clone()));

    let save_destination = dirs::download_dir()
        .or_else(dirs::home_dir)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let out_t = OutcomingTransfer {
        ready: AtomicBool::default(),
        total: AtomicU64::default(),
        walker: Mutex::default(),
        match_builder: Mutex::default(),
    };
    let essentials = Rc::new(EssentialsModel::new());

    let ui = Rc::new(UiState {
        paths: RefCell::new(HashSet::new()),
        essentials,
        devices_model,
        explorer: Explorer::new(),
        transfers_model: OnceCell::new(),
        // clipboard: RefCell::new(Clipboard::new().expect("Clipboard error.")),
        save_destination: RefCell::new(save_destination),
        outcoming_transfer: Arc::new(out_t),
        code_timer: RefCell::new(Timer::default()),
    });

    UI.with(|c| c.set(ui).ok().expect("UiState already initialized."));
}

pub fn init_transfers(w: &NectanWindow, transfers: Arc<Transfers>) {
    let model = Rc::new(TransfersModel::new(transfers));
    TransfersBridge::get(w).set_transfers(ModelRc::from(model.clone()));
    ui_state()
        .transfers_model
        .set(model)
        .ok()
        .expect("TransfersModel already initialized.");
}
