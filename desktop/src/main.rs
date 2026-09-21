#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

use anyhow::Result;
use nectan_core::common::get_signing_key;
use nectan_core::storage_utils::KvStore;
use std::path::PathBuf;

use crate::devices::update_devices;
use crate::handlers::{
    handle_add_device, handle_cancel_walker, handle_connection_offer,
    handle_incoming_transfer_offer, handle_scan_files, handle_scan_folders, handle_send,
    handle_tabs, handle_window_controls, start_event_listener,
};
use crate::state::init_app_state;
use crate::transfers::update_transfers_list;
use nectan_core::devices::Devices;
use nectan_core::devices::UserInfo;
use nectan_core::devices::Username;
use nectan_core::setup_core;
use std::sync::Arc;

mod devices;
mod handlers;
mod state;
mod transfers;

#[tokio::main]
async fn main() -> Result<(), slint::PlatformError> {
    let w = NectanWindow::new()?;

    init_app_state(&w);

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .without_time()
        .init();

    let userinfo = UserInfo::new(Username::new("Desktop User").unwrap());
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let devices = Devices::new(Some(data_dir)).expect("Failed to create devices pool.");
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| std::env::temp_dir())
        .join("Nectan");
    let store = KvStore::new(data_dir, "store").expect("Failed to create persistent storage.");

    let key = get_signing_key(&store);
    let device_id = key.verifying_key();

    let state = setup_core(tx, device_id, userinfo, key, devices, store)
        .await
        .unwrap();
    let state = Arc::new(state);

    start_event_listener(&w, rx, Arc::clone(&state));
    handle_window_controls(&w);
    handle_tabs(&w);
    handle_add_device(&w, Arc::clone(&state));

    handle_scan_files(&w);
    handle_scan_folders(&w);
    handle_cancel_walker(&w);
    handle_send(&w, Arc::clone(&state));

    handle_incoming_transfer_offer(&w, Arc::clone(&state));
    handle_connection_offer(&w, Arc::clone(&state));

    update_devices(&w.as_weak(), &state);
    update_transfers_list(&w.as_weak(), &state);

    w.run()
}
