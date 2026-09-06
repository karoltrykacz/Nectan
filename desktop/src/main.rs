#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

use anyhow::Result;
use slint::winit_030::WinitWindowAccessor;

fn main() -> Result<(), slint::PlatformError> {
    let w = NectanWindow::new()?;

    handle_window_controls(&w);

    w.run()
}

pub fn handle_window_controls(w: &NectanWindow) {
    let w_weak = w.as_weak();
    let bridge = w.global::<WindowBridge>();

    // Titlebar drag
    bridge.on_drag_window(move || {
        if let Some(w) = w_weak.upgrade() {
            w.window().with_winit_window(|winit_win| {
                let _ = winit_win.drag_window();
            });
        }
    });
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
