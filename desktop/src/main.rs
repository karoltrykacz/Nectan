#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

use anyhow::Result;
use nectan_core::common::get_signing_key;
use nectan_core::protocol::start_registration_loop;
use nectan_core::storage_utils::KvStore;

use crate::handlers::{
    handle_add_device, handle_incoming_transfer_offer, handle_tabs, handle_window_controls,
    start_event_listener,
};
use nectan_core::devices::Devices;
use nectan_core::devices::UserInfo;
use nectan_core::devices::Username;
use nectan_core::protocol::gen_device_id;
use nectan_core::setup_core;
use std::sync::Arc;
use uuid::Uuid;

mod handlers;

#[tokio::main]
async fn main() -> Result<(), slint::PlatformError> {
    let w = NectanWindow::new()?;
    tracing_subscriber::fmt().init();

    let userinfo = UserInfo::new(Username::new("Default User").unwrap());
    let tmp = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let devices = Devices::new(Some(tmp)).expect("Failed to create devices pool.");
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| std::env::temp_dir())
        .join("Nectan");
    let store = KvStore::new(data_dir, "store.json").expect("Failed to create persistent storage.");

    let key = get_signing_key(&store);
    let device_id = key.verifying_key();

    let state = setup_core(tx, device_id, userinfo, key, devices, store)
        .await
        .unwrap();
    let state = Arc::new(state);

    start_event_listener(&w, rx);
    handle_window_controls(&w);
    handle_tabs(&w);
    handle_add_device(&w, Arc::clone(&state));

    w.run()
}
