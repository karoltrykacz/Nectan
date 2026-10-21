#[cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

use crate::devices::{handle_devices, update_devices};
use crate::explorer::handle_explorer;
use crate::handlers::{
    event_listener, handle_add_device, handle_cancel_walker,
    handle_connection_offer, handle_drag_and_drop_files,
    handle_incoming_transfer_offer, handle_open_transfers,
    handle_outcoming_transfer, handle_paste, handle_scan_files,
    handle_scan_folders, handle_send, handle_settings, handle_username_change,
    handle_window_controls,
};
use crate::keymap::handle_keys;
use anyhow::Result;
use nectan_core::common::get_signing_key;
use nectan_core::storage::Storage;
// use search::handle_search;
use slint::{Model, ModelRc, VecModel, Weak};
use tracing::info;

use crate::sidebar::handle_sidebar;
use crate::state::{init_app_state, init_transfers};
use crate::transfers::{
    handle_lazyfiles_source, handle_refresh_items_list,
    handle_refresh_transfers_list, handle_transfer_controls, init_models,
    set_transfers,
};
use nectan_core::devices::Devices;
use nectan_core::devices::UserInfo;
use nectan_core::devices::Username;
use nectan_core::setup_core;
use std::any::type_name_of_val;
use std::sync::Arc;

mod devices;
mod explorer;
mod handlers;
mod keymap;
mod search;
mod sidebar;
mod state;
mod transfers;

pub fn theme_code(theme: &Theme<'_>, idx: i32) -> String {
    theme
        .get_themes()
        .row_data(idx as usize)
        .map(|t| t.code.to_string())
        .unwrap_or_else(|| "light".to_string())
}

pub fn theme_index(theme: &Theme<'_>, code: &str) -> i32 {
    theme
        .get_themes()
        .iter()
        .position(|t| t.code.as_str() == code)
        .unwrap_or(0) as i32
}

fn stateless_handlers(w: &NectanWindow) {
    init_app_state(&w);
    // handle_search(&w);
    handle_paste(&w);
    handle_scan_files(&w);
    handle_scan_folders(&w);
    handle_drag_and_drop_files(&w);
    handle_lazyfiles_source(&w);
    handle_refresh_items_list(&w);
    handle_window_controls(&w);
}

pub const LANGUAGES: [(&str, &str); 2] = [("English", "en"), ("Polish", "pl")];

pub fn apply_language(code: &str) {
    let lang = if code == "en" { "" } else { code };
    if let Err(e) = slint::select_bundled_translation(lang) {
        println!("Failed to select language [{code}] {e}");
    }
}

pub fn language_options() -> Vec<DropDownOption> {
    LANGUAGES
        .iter()
        .map(|(name, code)| DropDownOption {
            name: (*name).into(),
            code: (*code).into(),
        })
        .collect()
}
fn init_lang(w: &NectanWindow, storage: Arc<Storage>) {
    let b = w.global::<WindowBridge>();

    let saved = storage.language();
    let idx = LANGUAGES
        .iter()
        .position(|(_, code)| *code == saved)
        .unwrap_or(0);
    b.set_selected_language_idx(idx as i32);
    apply_language(&saved);

    b.set_languages(ModelRc::new(VecModel::from(language_options())));

    b.on_select_language({
        let storage = storage.clone();
        move |code| {
            apply_language(code.as_str());
            if let Err(e) = storage.update(|s| s.language = code.to_string()) {
                eprintln!("Failed to save language: {e}");
            }
        }
    });
}
use slint::Keys as SlintKeys;
fn init_state(weak: Weak<NectanWindow>, storage: Arc<Storage>) {
    tokio::spawn(async move {
        let userinfo = UserInfo::new(Username::new("Desktop User").unwrap());
        let devices =
            Devices::new(None).expect("Failed to create devices pool.");
        let key = get_signing_key(&storage);
        let device_id = key.verifying_key();

        let (tx, rx) = tokio::sync::mpsc::channel(16);

        let state =
            match setup_core(tx, device_id, userinfo, key, devices, storage)
                .await
            {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("setup_core failed: {e:?}");
                    return;
                }
            };

        tokio::spawn(event_listener(weak.clone(), rx, Arc::clone(&state)));

        let _ = weak.upgrade_in_event_loop(move |w| {
            handle_username_change(&w, state.clone());

            // Theme
            let theme = w.global::<Theme>();
            let s = state.clone();
            let weak = w.as_weak();
            theme.on_mode_changed(move |idx| {
                let ui = weak.unwrap();
                let code = theme_code(&ui.global::<Theme>(), idx);
                let _ = s.storage.update(|s| s.theme = code);
            });

            w.global::<SettingsBridge>().set_default_download_dir(
                state
                    .storage
                    .default_dir()
                    .to_string_lossy()
                    .to_string()
                    .into(),
            );

            init_transfers(&w, Arc::clone(&state.transfers));
            handle_settings(&w, Arc::clone(&state));
            handle_send(&w, Arc::clone(&state));
            handle_transfer_controls(&w, Arc::clone(&state));
            handle_incoming_transfer_offer(&w, Arc::clone(&state));
            handle_connection_offer(&w, Arc::clone(&state));
            handle_add_device(&w, Arc::clone(&state));
            handle_devices(&w, Arc::clone(&state));

            update_devices(&w.as_weak(), &state);
            set_transfers(&w.as_weak(), &state);

            init_models(&w);
            handle_cancel_walker(&w);
            handle_outcoming_transfer(&w);
            handle_refresh_transfers_list(&w);
            handle_open_transfers(&w);

            handle_explorer(&w);
        });
    });
}

#[tokio::main]
async fn main() -> Result<(), slint::PlatformError> {
    tracing_subscriber::fmt::init();

    let w = NectanWindow::new()?;

    // let sidebar = Keys::from_parts(["Control", "T"]).unwrap();
    // w.set_toggle_sidebar(sidebar);

    stateless_handlers(&w);

    let storage = Arc::new(Storage::new(Storage::dir()));
    let is_setup = storage.username().is_none();

    init_lang(&w, storage.clone());

    let theme = w.global::<Theme>();
    theme.set_mode(theme_index(&theme, &storage.theme()));

    println!("Is setup? {is_setup}");
    w.global::<WindowBridge>().set_welcome_open(is_setup);

    let st = storage.clone();
    let weak = w.as_weak();
    w.global::<WindowBridge>()
        .on_close_welcome(move |username| {
            info!("Welcome done. {username}");
            // ok to unwrap beacuse input check is on slint side
            let username = Username::new(username).expect("Invalid username.");
            let _ = st.update(|s| s.username = Some(username));
            init_state(weak.clone(), st.clone());
            info!("Welcome done.");
        });

    handle_keys(&w, storage.clone());
    handle_sidebar(&w);

    if !is_setup {
        init_state(w.as_weak(), storage);
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
