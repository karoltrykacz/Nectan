#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

use anyhow::Result;

use crate::handlers::{
    handle_add_device, handle_incoming_transfer_offer, handle_tabs, handle_window_controls,
    start_event_listener,
};
use nectan_core::devices::Devices;
use nectan_core::devices::UserInfo;
use nectan_core::devices::Username;
use nectan_core::protocol::{
    NectanState, TransferOffer, TransferOfferInner, build_offer, gen_device_id,
};
use nectan_core::setup_core;
use std::sync::Arc;
use uuid::Uuid;

mod handlers;

#[tokio::main]
async fn main() -> Result<(), slint::PlatformError> {
    let w = NectanWindow::new()?;

    let (device_id, key) = gen_device_id();
    let userinfo = UserInfo::new(Username::new("Default User").unwrap());
    let tmp = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let devices = Devices::new(Some(tmp)).expect("Failed to create devices pool.");
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let state = setup_core(tx, device_id, userinfo, key, devices)
        .await
        .unwrap();
    let state = Arc::new(state);

    start_event_listener(&w, rx);
    handle_window_controls(&w);
    handle_tabs(&w);
    handle_add_device(&w, Arc::clone(&state));

    w.run()
}
