#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

use anyhow::Result;
use arboard::Clipboard;
use nectan_core::common::get_signing_key;
use nectan_core::storage_utils::KvStore;
use nectan_core::walker::Walker;
use slint::DataTransfer;
use slint::winit_030::winit::event::Event;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::Ordering::{self, Relaxed};
use std::time::{Duration, Instant};

use crate::devices::update_devices;
use crate::handlers::{
    event_listener, handle_add_device, handle_cancel_walker, handle_connection_offer,
    handle_drag_and_drop_files, handle_incoming_transfer_offer, handle_outcoming_transfer,
    handle_paste, handle_scan_files, handle_scan_folders, handle_send, handle_window_controls,
    open_send_modal,
};
use crate::state::{UiState, init_app_state, ui_state};
use crate::transfers::{handle_refresh_items_list, handle_refresh_transfers_list, set_transfers};
use nectan_core::devices::Devices;
use nectan_core::devices::UserInfo;
use nectan_core::devices::Username;
use nectan_core::setup_core;
use slint::language::DragAction;
use std::sync::Arc;

mod devices;
mod handlers;
mod state;
mod transfers;

struct DragPayload {}

#[tokio::main]
async fn main() -> Result<(), slint::PlatformError> {
    let w = NectanWindow::new()?;

    let api = w.global::<Api>();
    api.on_make_data(|| {
        let mut t = DataTransfer::default();
        t.set_user_data(Rc::new(DragPayload {}));
        t
    });
    api.on_can_drop(|| -> DragAction { DragAction::Copy });
    api.on_dropped(|| {
        println!("Dropped some shit");
    });

    // let tray = Tray::new()?;

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

    init_app_state(&w, state.transfers.clone());

    tokio::spawn(event_listener(w.as_weak(), rx, Arc::clone(&state)));

    handle_paste(&w);

    handle_window_controls(&w);
    handle_add_device(&w, Arc::clone(&state));

    handle_drag_and_drop_files(&w);
    handle_send(&w, Arc::clone(&state));

    handle_outcoming_transfer(&w);
    handle_incoming_transfer_offer(&w, Arc::clone(&state));
    handle_connection_offer(&w, Arc::clone(&state));

    update_devices(&w.as_weak(), &state);

    handle_scan_files(&w);
    handle_scan_folders(&w);
    handle_cancel_walker(&w);

    handle_refresh_items_list(&w);
    handle_refresh_transfers_list(&w);

    set_transfers(&w.as_weak(), &state);
    w.run()
}
