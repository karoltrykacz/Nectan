#[cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

use crate::devices::{handle_devices, update_devices};

use crate::explorer::handle_explorer;
// use crate::handlers::{
//     event_listener, handle_add_device, handle_cancel_walker,
//     handle_connection_offer, handle_drag_and_drop_files,
//     handle_incoming_transfer_offer, handle_open_transfers,
//     handle_outcoming_transfer, handle_paste, handle_scan_files,
//     handle_scan_folders, handle_send, handle_settings,
//     handle_username_change, handle_window_controls,
// };
use crate::keymap::handle_keys;
use anyhow::Result;
use nectan_core::common::get_signing_key;
use nectan_core::storage::Storage;
use std::path::PathBuf;
use std::time::{Duration, Instant};
// use search::handle_search;
use slint::{Model, ModelRc, VecModel, Weak};
use tracing::info;

use crate::sidebar::handle_sidebar;
use crate::state::{init_app_state, init_transfers};
use crate::transfers::{
    handle_lazyfiles_source, handle_refresh_items_list,
    handle_refresh_transfers_list, init_models, set_transfers,
};
use nectan_core::devices::Devices;
use nectan_core::devices::UserInfo;
use nectan_core::devices::Username;
use nectan_core::setup_core;
use std::any::type_name_of_val;
use std::sync::Arc;

mod devices;
mod explorer;
mod file_picker;
// mod handlers;
mod keymap;
mod search;
mod sidebar;
mod state;
mod transfers;
mod uri;
mod window;

#[cfg(target_os = "linux")]
use crate::drag_and_drop_handler::handle_drag_drop;
#[cfg(target_os = "linux")]
mod drag_and_drop_handler;
#[cfg(target_os = "linux")]
mod wayland;
#[cfg(target_os = "linux")]
mod x11;

fn stateless_handlers(w: &App) {
    // init_app_state(&w);
    // handle_sidebar(&w);
    // // handle_search(&w);
    // handle_paste(&w);
    // handle_scan_files(&w);
    // handle_scan_folders(&w);
    // // handle_drag_and_drop_files(&w);
    // handle_lazyfiles_source(&w);
    // handle_refresh_items_list(&w);
    // handle_window_controls(&w);
    //
    // #[cfg(target_os = "linux")]
    // handle_drag_drop(&w);
}

async fn init_state(weak: Weak<App>, storage: Arc<Storage>) {
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

    // tokio::spawn(event_listener(
    //     weak.clone(),
    //     rx,
    //     Arc::clone(&state),
    // ));
    //
    let _ = weak.upgrade_in_event_loop(move |w| {
        // handle_username_change(&w, state.clone());
        //
        // init_transfers(&w, Arc::clone(&state.transfers));
        // handle_settings(&w, Arc::clone(&state));
        // handle_send(&w, Arc::clone(&state));
        // handle_transfer_controls(&w, Arc::clone(&state));
        // handle_incoming_transfer_offer(&w, Arc::clone(&state));
        // handle_connection_offer(&w, Arc::clone(&state));
        // handle_add_device(&w, Arc::clone(&state));
        // handle_devices(&w, Arc::clone(&state));
        //
        // update_devices(&w.as_weak(), &state);
        // set_transfers(&w.as_weak(), &state);
        //
        // init_models(&w);
        // handle_cancel_walker(&w);
        // handle_outcoming_transfer(&w);
        // handle_refresh_transfers_list(&w);
        // handle_open_transfers(&w);
        //
        // handle_explorer(&w);
    });
}

#[tokio::main]
async fn main() -> Result<(), slint::PlatformError> {
    let w = App::new()?;
    tracing_subscriber::fmt::init();

    // let sidebar = Keys::from_parts(["Control", "T"]).unwrap();
    // w.set_toggle_sidebar(sidebar);

    // let weak = w.as_weak();
    // w.global::<Icons>().on_set_icon(move |id, path| {
    //     let path = PathBuf::from(path.to_string());
    //     info!("Icon {id} {}", path.display());
    //     if let Ok(img) = slint::Image::load_from_path(&path) {
    //         info!("Loaded image");
    //         let w = weak.upgrade().unwrap();
    //         let icons = w.global::<Icons>();
    //         icons.set_refresh(img);
    //     }
    // });

    println!("Test");
    stateless_handlers(&w);

    let storage = Arc::new(Storage::new(Storage::dir()));
    let is_setup = storage.username().is_none();

    // init_lang(&w, storage.clone());

    let theme = w.global::<Theme>();

    // theme.set_mode(theme_index(&theme, &storage.theme()));
    // theme.set_font(storage.font_family().into());
    // theme.set_font_scale(storage.font_scale());

    println!("Is setup? {is_setup}");
    w.global::<WindowBridge>().set_welcome_open(is_setup);

    let st = storage.clone();
    let weak = w.as_weak();
    w.global::<WindowBridge>()
        .on_close_welcome(move |username| {
            let username = Username::new(username).unwrap();
            let _ = st.update(|s| s.username = Some(username));
            tokio::spawn(init_state(weak.clone(), st.clone()));
        });

    //
    handle_keys(&w, storage.clone());

    if !is_setup {
        tokio::spawn(init_state(w.as_weak(), storage));
    }

    w.run()
}

// struct DragPayload {}
// let api = w.global::<Api>();
// api.on_make_data(|| {
//     let mut t = DataTransfer::default();
//     t.set_user_data(Rc::new(DragPayload {}));
//     t
// });
// api.on_can_drop(|| -> DragAction { DragAction::Copy });
// api.on_dropped(|| {
//     println!("Dropped.");
// });
//
// let tray = Tray::new()?;
