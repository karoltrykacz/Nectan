use std::{rc::Rc, sync::Arc};

use nectan_core::{
    code_lookup::{CodeLookupError, gen_code, issue_code, lookup_code},
    format::DecimalBytes,
    messages::{
        AppEvent::{self},
        UiResponse,
    },
    protocol::{NectanState, TransferOffer, connect},
};
use slint::{ComponentHandle, Model, ModelRc, VecModel, Weak, winit_030::WinitWindowAccessor};
use tokio::sync::mpsc::Receiver;
use tracing::{error, info, trace};

use crate::{
    AddDeviceBridge, IncomingTransferOffer, IncomingTransferOfferBridge, LookupState, NectanTab,
    NectanWindow, WindowBridge, selected_window,
};

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

pub fn start_event_listener(w: &NectanWindow, mut rx: Receiver<AppEvent>) {
    let w = w.as_weak();
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            match msg {
                AppEvent::IncomingTransferOffer { offer } => {
                    show_transfer_offer(&w, offer);
                }
                _ => {}
            }
        }
    });
}

pub fn handle_incoming_transfer_offer(w: &NectanWindow, state: Arc<NectanState>) {
    let b = w.global::<IncomingTransferOfferBridge>();
    let weak = w.as_weak();
    let s = state.clone();
    b.on_accept(move || {
        info!("Accepted");
        if let Some(w) = weak.upgrade() {
            let b = w.global::<IncomingTransferOfferBridge>();
            b.set_is_open(false);
            let s = s.clone();

            tokio::spawn(async move {
                s.respond_to_offer(UiResponse::Reject {
                    reason: Some("User rejected the offer.".to_string()),
                });
            });
        }
    });

    let weak = w.as_weak();
    let s = state.clone();
    b.on_reject(move || {
        info!("Rejected");
        if let Some(w) = weak.upgrade() {
            let b = w.global::<IncomingTransferOfferBridge>();
            b.set_is_open(false);

            let s = s.clone();
            tokio::spawn(async move {
                s.respond_to_offer(UiResponse::Reject {
                    reason: Some("User rejected the offer.".to_string()),
                });
            });
        }
    });

    // let weak = w.as_weak();
    // b.on_toggle_node(move |id, path| {
    //     let Some(w) = weak.upgrade() else { return };
    //
    //     let bridge = w.global::<IncomingTransferOfferBridge>();
    //     let nodes = bridge.get_nodes();
    //     let mut nodes: Vec<TreeNode> = nodes.iter().map(|node| node.clone()).collect();
    //     if nodes[id as usize].is_file {
    //         return;
    //     }
    //     let parent_depth = nodes[id as usize].depth;
    //
    //     if nodes[id as usize].expanded {
    //         nodes[id as usize].expanded = false;
    //         // Close the nodes
    //         let mut end = id as usize + 1;
    //         while end < nodes.len() && nodes[end].depth > parent_depth {
    //             end += 1;
    //         }
    //         nodes.drain(id as usize + 1..end);
    //
    //         let model = VecModel::from(nodes);
    //         bridge.set_nodes(ModelRc::from(Rc::new(model)));
    //         return;
    //     }
    //
    //     nodes[id as usize].expanded = true;
    //
    //     let offer = app_state().transfer_offer();
    //     let weak = w.as_weak();
    //
    //     // Read from the tree the sender sent us
    //     tokio::spawn(async move {
    //         let path = Path::new(&path);
    //         let lock = offer.read().await;
    //         let info = lock.as_ref().unwrap();
    //
    //         let children: Vec<TreeNode> = info
    //             .info
    //             .tree
    //             .children_of(path)
    //             .iter()
    //             .map(|(p, is_file)| TreeNode {
    //                 depth,
    //                 expanded: false,
    //                 is_file: *is_file,
    //                 filename: p.file_name().unwrap().to_string_lossy().to_string().into(),
    //                 path: p.to_string_lossy().to_string().into(),
    //             })
    //             .collect();
    //         nodes.splice((id + 1) as usize..(id + 1) as usize, children);
    //
    //         let _ = weak.upgrade_in_event_loop(move |w| {
    //             let bridge = w.global::<IncomingTransferOfferBridge>();
    //             let model = VecModel::from(nodes);
    //             bridge.set_nodes(ModelRc::from(Rc::new(model)));
    //         });
    //     });
    // });
}

pub fn show_transfer_offer(w: &Weak<NectanWindow>, offer: TransferOffer) {
    // TODO show the contents
    let _ = w.upgrade_in_event_loop(move |w| {
        let b = w.global::<IncomingTransferOfferBridge>();
        let nodes = Vec::new();

        let offer = IncomingTransferOffer {
            from: offer.sender_name.into(),
            total_size: format!("{}", DecimalBytes(1_500)).into(),
            transfer_name: offer.inner.transfer_name.into(),
            total_entries: offer.inner.entries_num as i32,
            // offer,
        };
        b.set_nodes(ModelRc::new(VecModel::from(nodes)));
        b.set_offer(offer);
        b.set_is_open(true);
    });
}

pub fn handle_tabs(w: &NectanWindow) {
    let bridge = w.global::<WindowBridge>();

    let initial_tabs = vec![
        NectanTab {
            display: "Transfers".into(),
            id: 0,
            kind: selected_window::Transfers,
            ref_id: 0,
        },
        // NectanTab {
        //     display: "Home".into(),
        //     id: 1,
        //     kind: selected_window::Transfers,
        //     ref_id: 1,
        // },
    ];

    let tabs_model = Rc::new(VecModel::from(initial_tabs));
    bridge.set_tabs(ModelRc::from(tabs_model.clone()));

    let ui_weak = w.as_weak();

    bridge.on_close_requested({
        let tabs_model = tabs_model.clone();
        let ui_weak = ui_weak.clone();
        move |id| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let bridge = ui.global::<WindowBridge>();

            let Some(idx) = (0..tabs_model.row_count())
                .find(|&i| tabs_model.row_data(i).map(|t| t.id) == Some(id.clone()))
            else {
                return;
            };

            tabs_model.remove(idx);

            if tabs_model.row_count() == 0 {
                let fresh = NectanTab {
                    display: "Transfers".into(),
                    id: 2,
                    kind: selected_window::Transfers,
                    ref_id: 2,
                };

                tabs_model.push(fresh);
                bridge.set_current_tab(0);
                return;
            }

            let current = bridge.get_current_tab();
            if current == idx as i32 {
                let new_current = idx.min(tabs_model.row_count() - 1);
                bridge.set_current_tab(new_current as i32);
            } else if current > idx as i32 {
                bridge.set_current_tab(current - 1);
            }
        }
    });

    bridge.on_reorder_requested({
        let tabs_model = tabs_model.clone();
        let ui_weak = ui_weak.clone();
        move |from_idx, to_idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let bridge = ui.global::<WindowBridge>();

            let from = from_idx as usize;
            let to = to_idx as usize;

            if from >= tabs_model.row_count() || to >= tabs_model.row_count() || from == to {
                return;
            }

            if let Some(item) = tabs_model.row_data(from) {
                tabs_model.remove(from);
                tabs_model.insert(to, item);
                bridge.set_current_tab(to_idx);
            }
        }
    });
    bridge.on_open_tab({
        let ui_weak = ui_weak.clone();
        move |kind, ref_id, display_n| {
            trace!("Opening new tab. {kind:?} {ref_id:?} {display_n:?}",);

            let Some(ui) = ui_weak.upgrade() else { return };
            let bridge = ui.global::<WindowBridge>();
            let tabs = bridge.get_tabs();
            let mut found_idx: Option<usize> = None;

            for i in 0..tabs.row_count() {
                let tab = tabs.row_data(i).unwrap();
                let same_kind = tab.kind == kind;
                let same_ref = match kind {
                    selected_window::Transfers | selected_window::Vpn => true,
                    selected_window::Device => tab.ref_id == ref_id,
                };
                if same_kind && same_ref {
                    found_idx = Some(i);

                    break;
                }
            }
            if let Some(idx) = found_idx {
                bridge.set_current_tab(idx as i32);
                bridge.set_current_window(kind);
                bridge.set_current_ref_id(ref_id.clone());
            } else {
                let new_id = (0..tabs.row_count())
                    .filter_map(|i| tabs.row_data(i))
                    .map(|t| t.id)
                    .max()
                    .unwrap_or(-1)
                    + 1;

                let new_idx = tabs.row_count() as i32;

                let new_tab = NectanTab {
                    display: display_n.into(),
                    id: new_id,
                    kind,
                    ref_id: ref_id.clone().into(),
                };

                if let Some(model) = tabs.as_any().downcast_ref::<VecModel<NectanTab>>() {
                    model.push(new_tab);
                }

                bridge.set_current_tab(new_idx);
                bridge.set_current_window(kind);
                bridge.set_current_ref_id(ref_id);
            }
        }
    });
}

pub fn handle_add_device(w: &NectanWindow, state: Arc<NectanState>) {
    // Handlecode request
    let bridge = w.global::<AddDeviceBridge>();
    let ui_weak = w.as_weak();
    let s = state.clone();

    bridge.on_request_code(move || {
        let code = gen_code();
        let code_split = format!("{} {}", &code[0..3], &code[3..6]);

        let state = s.clone();
        let ui_weak = ui_weak.clone();

        if let Some(ui) = ui_weak.upgrade() {
            let bridge = ui.global::<AddDeviceBridge>();
            bridge.set_our_code(code_split.into());
        }

        tokio::spawn(async move {
            let result = issue_code(&state, code).await;
            slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    let bridge = ui.global::<AddDeviceBridge>();
                    match result {
                        Ok(code) => {
                            let code_split = format!("{} {}", &code[0..3], &code[3..6]);
                            bridge.set_our_code(code_split.into());
                        }
                        Err(e) => {
                            ui.invoke_show_error("Error".into(), e.to_string().into());
                            bridge.invoke_reset_add_remote();
                            bridge.set_add_device_open(false);
                            error!("Failed to issue code {e:?}");
                        }
                    }
                }
            })
            .unwrap();
        });
    });

    // Handle code lookup
    let s = state.clone();
    let ui_weak = w.as_weak();
    bridge.on_code_submitted(move |code| {
        let state = s.clone();
        let ui_weak = ui_weak.clone();
        tokio::spawn(async move {
            if let Some(ui) = ui_weak.upgrade() {
                let bridge = ui.global::<AddDeviceBridge>();
                bridge.set_lookup_state(LookupState::Checking);
            }
            let result = lookup_code(code.to_string(), &state).await;
            slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    let bridge = ui.global::<AddDeviceBridge>();
                    match result {
                        // Server may replace code to new one if it collides
                        Ok(addr) => {
                            bridge.set_lookup_state(LookupState::Connecting);
                            tokio::spawn(async move {
                                let result = connect(&state, addr).await;

                                if let Some(ui) = ui_weak.upgrade() {
                                    let bridge = ui.global::<AddDeviceBridge>();
                                    match result {
                                        Ok(device) => {
                                            bridge.set_lookup_state(LookupState::Connected);
                                            // And some info???
                                        }
                                        Err(_e) => {
                                            // TODO COULDE BE MORE VERBOSE
                                            bridge.set_lookup_state(LookupState::Rejected);
                                        }
                                    }
                                }
                            });
                        }
                        Err(e) => match e {
                            CodeLookupError::NotFound => {
                                bridge.set_lookup_state(LookupState::CodeNotFound);
                            }
                            CodeLookupError::NectanService => {
                                ui.invoke_show_error(
                                    "Nectan failed".into(),
                                    "Unexpected error.".into(),
                                );
                                bridge.invoke_reset_add_remote();
                            }
                            CodeLookupError::ConnectionFailed => {
                                ui.invoke_show_error(
                                    "Connection failed".into(),
                                    "Check your connection and try again.".into(),
                                );
                                bridge.invoke_reset_add_remote();
                            }
                        },
                    }
                }
            })
            .unwrap();
        });
    });
}
