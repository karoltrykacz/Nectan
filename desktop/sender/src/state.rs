use std::{
    cell::{OnceCell, Ref, RefCell},
    collections::HashSet,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, MutexGuard},
};

use clipboard_rs::ClipboardContext;
use nectan_core::{transfers::Transfers, walker::Walker};
use slint::{Global, ModelRc, Timer};

use crate::{
    DevicesBridge, NectanSender,
    devices::DevicesModel,
    transfers::{OutcomingTransfer, TransfersModel},
};

pub struct SenderState {
    paths: RefCell<HashSet<PathBuf>>,
    devices_model: Rc<DevicesModel>,
    transfers_model: OnceCell<Rc<TransfersModel>>,
    outcoming_transfer: Arc<OutcomingTransfer>,
    clipboard: ClipboardContext,
    save_destination: RefCell<PathBuf>,
    code_timer: RefCell<Timer>,
    pub dnd_init_timer: RefCell<Timer>,
    pub dnd_pump_timer: RefCell<Timer>,
}

impl SenderState {
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

    pub fn clipboard(&self) -> &ClipboardContext {
        &self.clipboard
    }
}

thread_local! {
    static UI: OnceCell<Rc<SenderState>> = const { OnceCell::new() };
}

pub fn ui_state() -> Rc<SenderState> {
    UI.with(|c| {
        c.get()
            .expect("State not initialized on this thread")
            .clone()
    })
}

pub fn init_app_state(w: &NectanSender) {
    let devices_model = Rc::new(DevicesModel::new());

    DevicesBridge::get(w)
        .set_devices(ModelRc::from(devices_model.clone()));

    let save_destination = dirs::download_dir()
        .or_else(dirs::home_dir)
        .or_else(|| std::env::current_dir().ok())
        .expect("Failed to select download path.");

    let ui = Rc::new(SenderState {
        paths: RefCell::new(HashSet::new()),
        devices_model,
        transfers_model: OnceCell::new(),
        outcoming_transfer: Arc::new(OutcomingTransfer::default()),
        clipboard: ClipboardContext::new().expect("Clipboard error."),
        save_destination: RefCell::new(save_destination),
        code_timer: RefCell::new(Timer::default()),
        dnd_init_timer: RefCell::new(Timer::default()),
        dnd_pump_timer: RefCell::new(Timer::default()),
    });

    UI.with(|c| {
        c.set(ui).ok().expect("UiState already initialized.")
    });
}
// pub fn init_transfers(w: &NectanSender, transfers: Arc<Transfers>) {
//     let model = Rc::new(TransfersModel::new(transfers));
//     TransfersBridge::get(w)
//         .set_transfers(ModelRc::from(model.clone()));
//     ui_state()
//         .transfers_model
//         .set(model)
//         .ok()
//         .expect("TransfersModel already initialized.");
// }
