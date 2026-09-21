use crate::{
    DevicesBridge, NectanWindow, TransfersBridge, devices::DevicesModel, transfers::TransfersModel,
};
use slint::{Global, ModelRc};
use std::{cell::OnceCell, rc::Rc, sync::Arc};

pub struct UiState {
    devices_model: Rc<DevicesModel>,
    transfers_model: Rc<TransfersModel>,
}

impl UiState {
    pub fn devices(&self) -> Rc<DevicesModel> {
        self.devices_model.clone()
    }

    pub fn transfers(&self) -> Rc<TransfersModel> {
        self.transfers_model.clone()
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
        devices_model,
        transfers_model,
    });

    UI.with(|c| c.set(ui).ok().expect("UiState already initialized."));

    println!("NectanAppState Initialized.");
}
