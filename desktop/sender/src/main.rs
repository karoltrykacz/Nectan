use nectan_core::common::get_signing_key;
use nectan_core::devices::{Devices, UserInfo, Username};
use nectan_core::protocol::NectanState;
use nectan_core::setup_core;
use nectan_core::storage::Storage;
use notify_rust::Notification;

use crate::add_remote::handle_add_device;
use crate::devices::handle_devices;
use crate::events::event_listener;
use crate::events::notify;
use crate::state::init_app_state;
use slint::Weak;
use std::sync::Arc;

slint::include_modules!();

mod add_remote;
mod devices;
mod events;
mod offers;
mod state;
mod transfers;

#[cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#[tokio::main]
async fn main() -> Result<(), slint::PlatformError> {
    tracing_subscriber::fmt::init();
    let w = NectanSender::new()?;
    slint::set_xdg_app_id("nectan")?;

    init_app_state(&w);

    let storage = Arc::new(Storage::new(Storage::dir()));
    tokio::spawn(init(w.as_weak(), storage));

    w.run()
}

async fn init(weak: Weak<NectanSender>, storage: Arc<Storage>) {
    let userinfo =
        UserInfo::new(Username::new("Desktop User").unwrap());
    let devices = Devices::new(None);
    let key = get_signing_key(&storage);
    let device_id = key.verifying_key();
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let state = match setup_core(
        tx, device_id, userinfo, key, devices, storage,
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
        handle_add_device(&w, Arc::clone(&state));
    });

    // init_transfers(&w, Arc::clone(&state.transfers));
    // handle_settings(&w, Arc::clone(&state));
    // handle_send(&w, Arc::clone(&state));
    // handle_transfer_controls(&w, Arc::clone(&state));
    // handle_incoming_transfer_offer(&w, Arc::clone(&state));
    // handle_connection_offer(&w, Arc::clone(&state));
    // handle_devices(&w, Arc::clone(&state));
    // update_devices(&w.as_weak(), &state);
    // set_transfers(&w.as_weak(), &state);
    // init_models(&w);
    // handle_cancel_walker(&w);
    // handle_outcoming_transfer(&w);
    // handle_refresh_transfers_list(&w);
    // handle_open_transfers(&w);
}
