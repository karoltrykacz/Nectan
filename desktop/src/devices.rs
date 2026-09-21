use nectan_core::{
    devices::device_id_to_base64, format::RoundedDecimalBytes, protocol::NectanState,
};
use slint::{Model, ModelNotify, ModelTracker, Weak};
use std::cell::RefCell;

use crate::{DeviceItem, DeviceStatus, NectanWindow, state::ui_state};

pub struct DevicesModel {
    devices: RefCell<Vec<DeviceItem>>,
    notify: ModelNotify,
}

impl DevicesModel {
    pub fn new() -> Self {
        Self {
            devices: RefCell::new(Vec::new()),
            notify: ModelNotify::default(),
        }
    }
    pub fn update(&self, new_devices: Vec<DeviceItem>) {
        *self.devices.borrow_mut() = new_devices;
        self.notify.reset();
    }
}

impl Model for DevicesModel {
    type Data = DeviceItem;

    fn row_count(&self) -> usize {
        self.devices.borrow().len()
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        self.devices.borrow().get(row).cloned()
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
}

impl From<nectan_core::devices::DeviceStatus> for DeviceStatus {
    fn from(value: nectan_core::devices::DeviceStatus) -> Self {
        match value {
            nectan_core::devices::DeviceStatus::Online => DeviceStatus::Online,
            nectan_core::devices::DeviceStatus::Offline => DeviceStatus::Offline,
            nectan_core::devices::DeviceStatus::Nearby => DeviceStatus::Nearby,
        }
    }
}

pub fn update_devices(w: &Weak<NectanWindow>, state: &NectanState) {
    let state = state.clone();
    let _ = w.upgrade_in_event_loop(move |_w| {
        let mut devices: Vec<DeviceItem> = state
            .devices
            .get_all()
            .iter()
            .map(|d| DeviceItem {
                id: device_id_to_base64(&d.id).into(),
                name: d.username.to_string().into(),
                status: DeviceStatus::from(d.status),
                favourite: d.fav,
                total_exchanged_data: RoundedDecimalBytes(d.total_exchanged_data)
                    .to_string()
                    .into(),
                completed_transfers: d.completed_transfers as i32,
            })
            .collect();

        devices.sort_unstable_by_key(|d| {
            let status_rank = match d.status {
                DeviceStatus::Nearby => 0,
                DeviceStatus::Online => 1,
                DeviceStatus::Offline => 2,
            };
            (status_rank, std::cmp::Reverse(d.favourite))
        });

        ui_state().devices().update(devices);
    });
}
