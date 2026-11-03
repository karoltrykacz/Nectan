use crate::add_remote::handle_add_device;
use crate::devices::handle_devices;
use crate::dnd::handle_drag_drop;
use crate::events::event_listener;
use crate::offers::handle_offers;
use crate::sidebar::handle_sidebar;
use crate::state::init_app_state;
use crate::transfers::handle_transfers;
use crate::window::handle_settings;
use crate::window::handle_window;
use nectan_core::common::get_signing_key;
use nectan_core::devices::{Devices, UserInfo, Username};
use nectan_core::setup_core;
use nectan_core::storage::Storage;
use slint::ModelRc;
use slint::SharedString;
use slint::VecModel;
use slint::Weak;
use slint::platform::WindowEvent;
use slint::winit_030::EventResult;
use slint::winit_030::WinitWindowAccessor;
use slint::winit_030::winit::window::WindowLevel;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::Ordering::SeqCst;
use std::time::Duration;
use std::time::Instant;
use std::time::UNIX_EPOCH;
use tokio::sync::Mutex;

slint::include_modules!();

mod add_remote;
mod devices;
mod dnd;
mod events;
mod offers;
mod sidebar;
mod state;
mod transfers;
mod uri;
mod wayland;
mod window;

use slint::{Model, ModelTracker};

pub struct LargeDeviceModel {
    pub total_count: usize,
    // ModelTracker allows Slint to react if items are dynamically added/removed
    notify: RefCell<Option<Box<dyn ModelTracker>>>,
}

impl LargeDeviceModel {
    pub fn new(count: usize) -> Self {
        Self {
            total_count: count,
            notify: RefCell::new(None),
        }
    }
}

impl Model for LargeDeviceModel {
    type Data = DeviceItem;

    fn row_count(&self) -> usize {
        self.total_count
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        if row >= self.total_count {
            return None;
        }

        Some(DeviceItem {
            completed_transfers: 0,
            status: DeviceStatus::Offline,
            total_exchanged_data: SharedString::new(),
            id: SharedString::from(format!("dev-{}", row)),
            name: SharedString::from(format!("Device #{:06}", row)),
            favourite: false,
        })
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &()
    }
}

#[cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#[tokio::main]
async fn main() -> Result<(), slint::PlatformError> {
    tracing_subscriber::fmt::init();
    let w = App::new()?;
    let _ = slint::set_xdg_app_id("nectan");

    init_app_state(&w);
    handle_drag_drop(&w);

    // println!("Generating 100,000 elements...");
    // let start_gen = Instant::now();
    // let devices_vec: Vec<DeviceItem> = (0..100_000)
    //     .map(|i| DeviceItem {
    //         completed_transfers: 0,
    //         status: DeviceStatus::Offline,
    //         total_exchanged_data: SharedString::new(),
    //         id: SharedString::from(format!("dev-{}", i)),
    //         name: SharedString::from(format!("Device #{:06}", i)),
    //         favourite: i % 5 == 0,
    //     })
    //     .collect();
    //
    // let model = Rc::new(VecModel::from(devices_vec));
    // w.set_devices(ModelRc::from(model));

    let storage = Arc::new(Storage::new(Storage::dir()));
    handle_window(&w, Arc::clone(&storage));
    tokio::spawn(init(w.as_weak(), storage));

    w.run()
}

async fn init(weak: Weak<App>, storage: Arc<Storage>) {
    let userinfo =
        UserInfo::new(Username::new("Desktop User").unwrap());
    let devices = Devices::new(None);
    let key = get_signing_key(&storage);
    let device_id = key.verifying_key();
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let state = match setup_core(
        tx, device_id, userinfo, key, devices, storage, false,
    )
    .await
    {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("setup_core failed: {e:?}");
            return;
        }
    };
    tokio::spawn(event_listener(
        weak.clone(),
        rx,
        Arc::clone(&state),
    ));

    let _ = weak.upgrade_in_event_loop(move |w| {
        handle_devices(&w, Arc::clone(&state));
        handle_sidebar(&w);
        handle_add_device(&w, Arc::clone(&state));
        handle_transfers(&w, Arc::clone(&state));
        handle_settings(&w, Arc::clone(&state));
        handle_offers(&w, state);
    });
}
