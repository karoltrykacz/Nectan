#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

use anyhow::Result;
use nectan_core::path_tree::PathTree;
use nectan_core::protocol::{Message, NectanState, build_offer};
use slint::winit_030::WinitWindowAccessor;
use slint::{ModelRc, ToSharedString, VecModel, Weak};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), slint::PlatformError> {
    let w = NectanWindow::new()?;

    let state = NectanState::new();
    start_event_listener(&w, state.clone());

    handle_window_controls(&w);

    let tree = build_offer();

    state
        .sender()
        .send(Message::TransferOffer {
            transfer_id: Uuid::default(),
            tree,
        })
        .unwrap();

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

pub fn start_event_listener(w: &NectanWindow, state: NectanState) {
    let mut event_rx = state.subscribe_to_events();
    let w = w.as_weak();
    tokio::spawn(async move {
        while let Ok(msg) = event_rx.recv().await {
            tracing::info!("New app event: {msg:#?}");
            match msg {
                Message::TransferOffer { transfer_id, tree } => {
                    show_transfer_offer(&w, transfer_id, tree);
                }
                _ => {
                    panic!("Invalid app event. {msg:#?}")
                }
            }
        }
    });
}

pub fn show_transfer_offer(w: &Weak<NectanWindow>, _transfer_id: Uuid, tree: PathTree) {
    let _ = w.upgrade_in_event_loop(move |w| {
        let b = w.global::<IncomingTransferOfferBridge>();
        b.set_open(true);

        let files: Vec<_> = tree
            .to_vec()
            .iter()
            .map(|(path, is_file)| path.to_string_lossy().to_shared_string())
            .collect();
        b.set_files(ModelRc::new(VecModel::from(files)));
    });
}
