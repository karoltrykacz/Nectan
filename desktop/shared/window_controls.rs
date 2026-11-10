use crate::App;
use crate::WindowBridge;
use slint::{ComponentHandle, Global, Weak};

pub fn handle_window_controls(w: &App) {
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

pub fn handle_links(w: &App) {
    let b = w.global::<WindowBridge>();

    b.on_open_github(move || {
        let _ = open::that("https://github.com/karoltrykacz/Nectan");
    });

    b.on_join_discord(move || {
        let _ = open::that("https://discord.gg/asljlkjlkjlk");
    });

    b.on_open_privacy(move || {
        let _ = open::that("https://nectan.co/privacy");
    });
}
