use std::sync::Arc;

use nectan_core::{
    format::{map_speed, unmap_speed},
    protocol::NectanState,
    storage::{Appearance, SpeedLimits, Storage},
};

use slint::{ComponentHandle, Global, Model, Weak};
use slint::{ModelRc, ToSharedString, VecModel};
use tracing::info;

use crate::{
    App, DropDownOption, SettingsBridge, Theme, ThemeConfig,
    WindowBridge, transfers::format_speed,
};

pub const LANGUAGES: [(&str, &str); 2] =
    [("English", "en"), ("Polski", "pl")];

pub fn handle_window(w: &App, storage: Arc<Storage>) {
    handle_window_controls(w);
    init_lang(w, storage.clone());
    handle_appearance(w, storage);
}

fn handle_appearance(w: &App, storage: Arc<Storage>) {
    fn theme_code(theme: &Theme<'_>, idx: i32) -> String {
        theme
            .get_themes()
            .row_data(idx as usize)
            .map(|t| t.code.to_string())
            .unwrap_or_else(|| "light".to_string())
    }

    {
        let theme = w.global::<Theme>();
        let a = storage.appearance();

        theme.set_config(ThemeConfig {
            edges_shape: a.edges_shape,
            round_edges: a.round_window_edges,
            font: a.font.into(),
            font_scale: a.font_scale,
            topbar_always_on: a.topbar_always_on,
            native_topbar: a.native_topbar,
        });

        let idx = theme
            .get_themes()
            .iter()
            .position(|t| t.code == a.theme.as_str())
            .unwrap_or(0) as i32;

        theme.set_mode(idx);
    }

    let theme = w.global::<Theme>();
    let s = storage.clone();
    let weak = w.as_weak();

    theme.on_config_changed(move || {
        let ui = weak.unwrap();
        let theme = ui.global::<Theme>();
        let cfg = theme.get_config();

        let _ = s.set_appearance(Appearance {
            theme: theme_code(&theme, theme.get_mode()),
            font: cfg.font.to_string(),
            font_scale: cfg.font_scale,
            edges_shape: cfg.edges_shape,
            round_window_edges: cfg.round_edges,
            native_topbar: cfg.native_topbar,
            topbar_always_on: cfg.topbar_always_on,
        });
    });
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

fn init_lang(w: &App, storage: Arc<Storage>) {
    let b = w.global::<WindowBridge>();

    let saved = storage.language();
    let idx = LANGUAGES
        .iter()
        .position(|(_, code)| *code == saved)
        .unwrap_or(0);
    b.set_selected_language_idx(idx as i32);
    apply_language(&saved);

    b.set_languages(ModelRc::new(VecModel::from(language_options())));

    let w = WindowBridge::get(w);

    let idx = LANGUAGES
        .iter()
        .position(|(_, code)| *code == saved)
        .unwrap_or(0);

    w.set_selected_language_idx(idx as i32);
    apply_language(&saved);

    b.on_select_language({
        move |code| {
            apply_language(code.as_str());
            if let Err(e) =
                storage.update(|s| s.language = code.to_string())
            {
                eprintln!("Failed to save language: {e}");
            }
        }
    });
}

fn handle_window_controls(w: &App) {
    let bridge = WindowBridge::get(w);

    let w_weak = w.as_weak();
    bridge.on_minimize_window(move || {
        if let Some(w) = w_weak.upgrade() {
            w.window().set_minimized(true);
        }
    });

    let w_weak = w.as_weak();
    bridge.on_maximize_window(move || {
        if let Some(w) = w_weak.upgrade() {
            let win = w.window();
            win.set_maximized(!win.is_maximized());
        }
    });

    let w_weak = w.as_weak();
    bridge.on_close_window(move || {
        if let Some(w) = w_weak.upgrade() {
            let _ = w.hide();
        }
    });
}

pub fn handle_settings(w: &App, s: Arc<NectanState>) {
    let b = w.global::<SettingsBridge>();
    let st = &s.storage;

    b.set_self_accept_nearby(st.read(|s| s.autoaccept_from_nearby));
    b.set_self_accept_favourites(
        st.read(|s| s.autoaccept_from_nearby),
    );

    let n = st.read(|s| s.notifications.clone());

    b.set_devices_notifications(n.device);
    b.set_transfers_notifications(n.transfer);
    b.set_connection_notifications(n.connection);

    b.set_default_download_dir(st.read(|s| {
        s.default_destination.to_string_lossy().to_string().into()
    }));

    let state = s.clone();
    let weak = w.as_weak();
    b.on_change_default_dir(move || {
        let weak = weak.clone();
        let s = state.clone();
        tokio::spawn(async move {
            let Some(h) =
                rfd::AsyncFileDialog::new().pick_folder().await
            else {
                return;
            };
            let path = h.path();
            // Ensure if has writing permission
            if let Err(e) = std::fs::write(
                path.join("test.txt"),
                b"Permission check",
            ) {
                show_error(&weak, "Error", e.to_string());
                return;
            }
            let _ = std::fs::remove_file(path);
            let _ = s
                .storage
                .update(|s| s.default_destination = path.into());
            let path = path.to_string_lossy().to_shared_string();
            let _ = weak.upgrade_in_event_loop(move |w| {
                w.global::<SettingsBridge>()
                    .set_default_download_dir(path);
            });
        });
    });

    let state = s.clone();
    b.on_toggle_accept_nearby(move || {
        let _ = state.storage.update(|s| {
            s.autoaccept_from_nearby = !s.autoaccept_from_nearby
        });
    });

    let state = s.clone();
    b.on_toggle_accept_favourites(move || {
        let _ = state.storage.update(|s| {
            s.autoaccept_from_favourites =
                !s.autoaccept_from_favourites
        });
    });

    b.on_open_github(move || {
        let _ = open::that("https://github.com/karoltrykacz/Nectan");
    });

    b.on_join_discord(move || {
        let _ = open::that("https://discord.gg/asljlkjlkjlk");
    });

    b.on_open_privacy(move || {
        let _ = open::that("https://nectan.co/privacy");
    });

    b.on_format_speed(move |speed| {
        format_speed(map_speed(speed)).into()
    });

    let l = s.storage.speed_limits();
    if let Some(l) = l {
        b.set_speed_limits_on(true);
        b.set_upload_limit(unmap_speed(l.upload as f32 * 8.0));
        b.set_download_limit(unmap_speed(l.download as f32 * 8.0));
    } else {
        b.set_speed_limits_on(false);
        b.set_upload_limit(0.5);
        b.set_download_limit(0.5);
    }

    let state = s.clone();
    b.on_confirm_limits(move |enabled, upload, download| {
        let speed_limits = if enabled {
            // Map to bytes
            let upload = (map_speed(upload) / 8.0) as u64;
            let download = (map_speed(download) / 8.0) as u64;
            info!("New limits {upload} {download}");
            Some(SpeedLimits { upload, download })
        } else {
            info!("Disabled limits");
            None
        };
        let _ =
            state.storage.update(|s| s.speed_limits = speed_limits);
        // TODO update transfers
    });

    // b.on_
}

fn apply_language(code: &str) {
    let lang = if code == "en" { "" } else { code };
    if let Err(e) = slint::select_bundled_translation(lang) {
        println!("Failed to select language [{code}] {e}");
    }
}

pub fn show_error(
    weak: &Weak<App>,
    title: impl Into<slint::SharedString>,
    msg: impl Into<slint::SharedString>,
) {
    let (title, msg) = (title.into(), msg.into());
    let _ = weak.upgrade_in_event_loop(move |w| {
        w.global::<crate::Error>().invoke_show_error(title, msg);
    });
}
