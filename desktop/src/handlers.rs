use ignore::WalkBuilder;
use nectan_core::{
    code_lookup::{CodeLookupError, gen_code, issue_code, lookup_code},
    common::{common_parent, gen_transfer_name},
    devices::{Username, device_id_from_base64},
    format::{DecimalBytes, RoundedDecimalBytes},
    messages::{
        AppEvent::{self},
        ConnectionOffer, UiResponse,
    },
    protocol::{NectanState, TransferOffer, TransferOfferRequest, connect},
    storage,
    transfers::{TransferOfferError, send_contents},
    walker::Walker,
};
use nucleo::{
    Config, Matcher,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};
use rfd::FileHandle;
use slint::{ComponentHandle, Model, ModelRc, VecModel, Weak, invoke_from_event_loop};
use slint::{
    ToSharedString,
    winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent},
};
use std::{
    cell::RefCell,
    fs::DirEntry,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, atomic::Ordering::Relaxed},
    time::Duration,
};
use tokio::sync::{mpsc::Receiver, oneshot};
use tracing::{error, info, trace};
use uuid::Uuid;

use crate::{
    AddDeviceBridge, ConnectionOfferBridge, DeviceItem, IncomingTransferOffer,
    IncomingTransferOfferBridge, LookupState, NectanWindow, OutcomingTransferModalBridge,
    SearchBridge, SendModalState, SettingsBridge, TreeNode, WindowBridge, devices::update_devices,
    state::ui_state, transfers::update_transfers,
};

fn show_error(
    weak: &slint::Weak<NectanWindow>,
    title: impl Into<slint::SharedString>,
    msg: impl Into<slint::SharedString>,
) {
    let (title, msg) = (title.into(), msg.into());
    let _ = weak.upgrade_in_event_loop(move |w| {
        w.invoke_show_error(title, msg);
    });
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

pub async fn event_listener(
    w: Weak<NectanWindow>,
    mut rx: Receiver<AppEvent>,
    state: Arc<NectanState>,
) {
    while let Some(msg) = rx.recv().await {
        match msg {
            AppEvent::TransfersUpdated => {
                update_transfers(&w, &state);
            }
            AppEvent::IncomingTransferOffer { offer } => {
                show_transfer_offer(&w, offer);
            }
            AppEvent::ConnectionOffer { offer } => {
                show_connection_offer(&w, offer);
            }
            AppEvent::FoundNearby { .. } => {
                update_devices(&w, &state);
            }
            AppEvent::Connected { .. } => {
                update_devices(&w, &state);
            }
            AppEvent::TransferOfferDelivered => {
                transfer_offer_delivered(&w);
            }
            AppEvent::DeviceWentOffline { .. } => {
                update_devices(&w, &state);
            }
        }
    }
}
fn transfer_offer_delivered(w: &Weak<NectanWindow>) {
    let _ = w.upgrade_in_event_loop(move |w| {
        let bridge = w.global::<OutcomingTransferModalBridge>();
        bridge.set_send_state(SendModalState::WaitingForResponse);
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
                s.respond_transfer_offer(UiResponse::Reject {
                    reason: Some("User rejected.".to_string()),
                })
                .await;
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
                s.respond_transfer_offer(UiResponse::Reject {
                    reason: Some("User rejected.".to_string()),
                })
                .await;
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
    //     let offer = ui_state().transfer_offer();
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

pub fn show_transfer_offer(w: &Weak<NectanWindow>, offer: TransferOfferRequest) {
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

pub fn show_connection_offer(w: &Weak<NectanWindow>, offer: ConnectionOffer) {
    let _ = w.upgrade_in_event_loop(move |w| {
        let b = w.global::<ConnectionOfferBridge>();
        b.set_open(true);
        b.set_remote_device_name(offer.username.as_string().into());
        // TODO
    });
}
pub fn handle_connection_offer(w: &NectanWindow, state: Arc<NectanState>) {
    let bridge = w.global::<ConnectionOfferBridge>();
    bridge.on_accept(move |accept| {
        let state = state.clone();
        tokio::spawn(async move {
            if accept {
                info!("Accepted connection offer.");
                state.respond_connection_offer(UiResponse::Accept).await;
            } else {
                info!("Rejected connection offer.");
                state
                    .respond_connection_offer(UiResponse::Reject {
                        reason: Some(String::from("User rejected.")),
                    })
                    .await;
            }
        });
    });
}

pub fn handle_add_device(w: &NectanWindow, state: Arc<NectanState>) {
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
            invoke_from_event_loop(move || {
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
                                let result = connect(state, addr).await;

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

pub fn handle_scan_files(w: &NectanWindow) {
    let weak = w.as_weak();
    w.global::<OutcomingTransferModalBridge>()
        .on_select_files(move || {
            let weak = weak.clone();
            tokio::spawn(async move {
                if let Some(h) = rfd::AsyncFileDialog::new().pick_files().await
                    && !h.is_empty()
                {
                    let paths = filehandle_to_paths(h);
                    open_send_modal(weak, paths);
                }
            });
        });
}
pub fn handle_scan_folders(w: &NectanWindow) {
    let weak = w.as_weak();
    w.global::<OutcomingTransferModalBridge>()
        .on_select_folders(move || {
            let weak = weak.clone();
            tokio::spawn(async move {
                let Some(h) = rfd::AsyncFileDialog::new().pick_folders().await else {
                    return;
                };
                if h.is_empty() {
                    return;
                }

                let paths = filehandle_to_paths(h);
                open_send_modal(weak, paths);
            });
        });
}

pub fn filehandle_to_paths(h: Vec<FileHandle>) -> Vec<PathBuf> {
    h.iter().map(|h| h.path().to_path_buf()).collect()
}

pub fn handle_cancel_walker(w: &NectanWindow) {
    w.global::<OutcomingTransferModalBridge>()
        .on_transfer_walk_cancelled(move || {
            if let Some(walker) = ui_state().walker().take() {
                walker.stop();
            }
        });
}

pub fn open_send_modal(w: Weak<NectanWindow>, paths: Vec<PathBuf>) {
    println!("Starting walker for {paths:#?}");

    let common = common_parent(&paths);
    let builder = WalkBuilder::new(common);

    let walker = Walker::new(paths.clone(), true);

    // Store the walker in ui state so it can be cancelled
    let walker2 = walker.clone();

    let _ = w.upgrade_in_event_loop(move |w| {
        {
            let state = ui_state();
            *state.outcoming_transfer().match_builder.lock().unwrap() = Some(builder);

            let mut walker_lock = state.walker();
            if let Some(old_walker) = &*walker_lock {
                old_walker.stop();
            }
            *walker_lock = Some(walker2);
        }

        let transfer_name = gen_transfer_name();
        let bridge = w.global::<OutcomingTransferModalBridge>();
        bridge.set_transfer_name(transfer_name.into());
        bridge.set_walk_finished(false);
        bridge.set_walk_total_size("0 B".into());
        bridge.set_walk_total_entries(0);
        bridge.set_send_state(SendModalState::Initial);
        bridge.set_sending_open(true);

        let nodes: Vec<TreeNode> = paths
            .iter()
            .map(|h| TreeNode {
                depth: 0,
                expanded: false,
                is_file: h.is_file(),
                filename: h
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
                    .into(),
                path: h.to_string_lossy().to_string().into(),
            })
            .collect();
        let model = VecModel::from(nodes);
        bridge.set_nodes(ModelRc::from(Rc::new(model)));
    });

    walker.walk();

    std::thread::spawn(move || {
        loop {
            let total_entries = walker.total_entries();
            let total_size = walker.total_size();

            let _ = w.upgrade_in_event_loop(move |w| {
                let bridge = w.global::<OutcomingTransferModalBridge>();
                let size_str = RoundedDecimalBytes(total_size).to_string();
                bridge.set_walk_total_entries(total_entries as i32);
                bridge.set_walk_total_size(size_str.into());
            });
            if walker.finished() {
                let _ = w.upgrade_in_event_loop(move |w| {
                    let bridge = w.global::<OutcomingTransferModalBridge>();
                    let size_str = RoundedDecimalBytes(total_size).to_string();
                    bridge.set_walk_total_entries(total_entries as i32);
                    bridge.set_walk_total_size(size_str.into());
                    bridge.set_walk_finished(true);
                });
                return;
            }

            std::thread::sleep(Duration::from_millis(18));
        }
    });
}

pub fn handle_send(w: &NectanWindow, s: Arc<NectanState>) {
    let weak = w.as_weak();
    let bridge = w.global::<OutcomingTransferModalBridge>();
    bridge.on_send(move |destination_id, transfer_name, compression| {
        info!("Sending offer. {transfer_name}. Compression - {compression}");
        if let Some(walk_info) = ui_state().walker().take() {
            let transfer_id = Uuid::new_v4();
            let destination_device = device_id_from_base64(&destination_id).unwrap();

            let entries_num = walk_info.total_entries() as u32;
            let total_size = walk_info.total_size();
            let transfer_name = transfer_name.into();

            let tree = walk_info
                .take_tree()
                .expect("At this point the tree should be Some.")
                .into();

            let info = TransferOffer {
                transfer_id,
                transfer_name,
                entries_num,
                total_size,
                tree,
            };

            let s = s.clone();
            let weak = weak.clone();
            tokio::spawn(async move {
                match send_contents(&s, destination_device, info).await {
                    Ok(()) => {
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            let bridge = w.global::<OutcomingTransferModalBridge>();
                            bridge.set_send_state(crate::SendModalState::Accepted);
                        });
                    }
                    Err(e) => {
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            let b = w.global::<OutcomingTransferModalBridge>();
                            match e {
                                TransferOfferError::DeviceOffline => {
                                    b.set_send_state(SendModalState::Offline);
                                }
                                TransferOfferError::TransferRejected => {
                                    b.set_send_state(SendModalState::Rejected);
                                }
                                TransferOfferError::ConnectionFailed => {
                                    b.set_send_state(SendModalState::Error);
                                    b.set_sending_error_msg(
                                        "Connection failed. Device not reachable.".into(),
                                    );
                                }
                                TransferOfferError::InvalidDestination => {
                                    b.set_send_state(SendModalState::Error);
                                    b.set_sending_error_msg(
                                        "Invalid destination. Try restarting Nectan.".into(),
                                    );
                                }
                                TransferOfferError::UnexpectedResponse => {
                                    b.set_sending_error_msg(
                                        "Unexcpected error. Device sent wrong message.".into(),
                                    );
                                }
                            }
                        });
                    }
                }
            });
        }
    });
}
pub fn handle_drag_and_drop_files(w: &NectanWindow) {
    let weak = w.as_weak();
    w.window().on_winit_window_event(move |_win, event| {
        let state = ui_state();
        match event {
            WindowEvent::HoveredFile(file) => {
                let outcoming = state.outcoming_transfer();
                state.insert_path(file.into());

                let me = outcoming.total.fetch_add(1, Relaxed) + 1;
                let weak = weak.clone();

                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    let total = outcoming.total.load(Relaxed);
                    if me == total {
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            let paths = ui_state().take_paths();
                            open_send_modal(w.as_weak(), paths);
                        });
                    }
                });
            }
            WindowEvent::HoveredFileCancelled => {
                let Some(walker) = &*state.walker() else {
                    return EventResult::Propagate;
                };
                // Cancel the walker if the drag was cancelled
                if !state.outcoming_transfer().ready.load(Relaxed) {
                    walker.stop();
                    if let Some(w) = weak.upgrade() {
                        let b = w.global::<OutcomingTransferModalBridge>();
                        b.set_sending_open(false);
                    }
                }
                state.outcoming_transfer().ready.store(false, Relaxed);
                state.take_paths();
            }
            WindowEvent::DroppedFile(_file) => {
                state.outcoming_transfer().ready.store(true, Relaxed);
            }
            _ => {}
        };
        EventResult::Propagate
    });
}

pub fn handle_outcoming_transfer(w: &NectanWindow) {
    let weak = w.as_weak();
    let b = w.global::<OutcomingTransferModalBridge>();
    b.on_toggle_node(move |id, path| {
        let Some(w) = weak.upgrade() else {
            return;
        };
        let id = id as usize;
        let outcoming = ui_state().outcoming_transfer();
        let weak = weak.clone();
        let bridge = w.global::<OutcomingTransferModalBridge>();
        let mut nodes: Vec<TreeNode> = bridge.get_nodes().iter().map(|node| node.clone()).collect();
        if nodes[id].is_file {
            return;
        }
        let parent_depth = nodes[id].depth;

        if nodes[id].expanded {
            nodes[id].expanded = false;

            // Close the nodes
            let mut end = id as usize + 1;
            while end < nodes.len() && nodes[end].depth > parent_depth {
                end += 1;
            }
            nodes.drain(id as usize + 1..end);

            let model = VecModel::from(nodes);
            bridge.set_nodes(ModelRc::from(Rc::new(model)));
            return;
        }

        nodes[id as usize].expanded = true;

        // Need to build the fucking walker beacuse the pathtree from walker is yielded after the walk is finished
        // Don;t block the ui thread
        std::thread::spawn(move || match std::fs::read_dir(path.clone()) {
            Ok(dir) => {
                let mut lock = outcoming.match_builder.lock().unwrap();
                let builder = lock.as_mut().unwrap();

                let path = PathBuf::from(path.to_string());
                builder.add(path);
                let mut matchers = builder.build_matchers();

                // Match children of the node with ignore patterns
                let children: Vec<DirEntry> = dir.flatten().collect();
                let mut result = Vec::new();
                'outer: for entry in &children {
                    let Ok(is_dir) = entry.file_type().map(|t| t.is_dir()) else {
                        continue;
                    };
                    for matcher in &mut matchers {
                        if matcher.matched(entry.path(), is_dir).is_ignore() {
                            break 'outer;
                        }
                    }

                    let node = TreeNode {
                        depth: parent_depth + 1,
                        expanded: false,
                        is_file: !is_dir,
                        filename: entry.file_name().to_string_lossy().to_shared_string(),
                        path: entry.path().to_string_lossy().to_shared_string(),
                    };
                    result.push(node);
                }
                // Append the nodes to the tree
                nodes.splice((id + 1) as usize..(id + 1) as usize, result);
                println!("Result nodes {nodes:#?}");

                let _ = weak.upgrade_in_event_loop(move |w| {
                    let bridge = w.global::<OutcomingTransferModalBridge>();
                    let model = VecModel::from(nodes);
                    bridge.set_nodes(ModelRc::from(Rc::new(model)));
                });
            }
            Err(e) => {
                show_error(&weak, "Failed to read directory", e.to_string());
            }
        });
    });
}

pub fn handle_paste(w: &NectanWindow) {
    let weak = w.as_weak();
    w.on_pasted(move || {
        let weak = weak.clone();
        let ui = ui_state();
        let mut cl = ui.get_clipboard();
        let contents = cl.get();

        if let Ok(list) = contents.file_list() {
            let list: Vec<PathBuf> = list
                .into_iter()
                .map(|p| PathBuf::from(p.to_string_lossy().trim_end_matches(['\r', '\n'])))
                .collect();
            open_send_modal(weak, list);
        } else {
            println!("Failed to get files list from clipboard.");
        }
    })
}

pub fn handle_username_change(w: &NectanWindow, s: Arc<NectanState>) {
    let b = w.global::<SettingsBridge>();
    let weak = w.as_weak();
    b.on_change_username(move |username| {
        if let Some(w) = weak.upgrade() {
            let b = w.global::<SettingsBridge>();
            b.set_username_modal(false);
            b.set_username(username.clone());
        }

        match Username::new(username) {
            Ok(u) => {
                if let Err(e) = s.storage.set(storage::Value::Username(u)) {
                    show_error(&weak, "Error", e.to_string());
                }
            }
            Err(e) => {
                show_error(&weak, "Invalid username", e.to_string());
            }
        }
    });
}

pub fn setup_search(w: &NectanWindow) {
    let weak = w.as_weak();
    let matcher = Rc::new(RefCell::new(Matcher::new(Config::DEFAULT)));

    w.global::<SearchBridge>().on_query_changed(move |q| {
        let Some(w) = weak.upgrade() else { return };
        let all = ui_state().devices().items();

        let results: Vec<DeviceItem> = if q.trim().is_empty() {
            all
        } else {
            let pattern = Pattern::new(
                q.as_str(),
                CaseMatching::Ignore,
                Normalization::Smart,
                AtomKind::Fuzzy,
            );
            let mut m = matcher.borrow_mut();
            let names: Vec<String> = all.iter().map(|d| d.name.to_string()).collect();
            let ranked = pattern.match_list(names.iter().map(|s| s.as_str()), &mut m);

            ranked
                .into_iter()
                .filter_map(|(name, _score)| all.iter().find(|d| d.name.as_str() == name).cloned())
                .collect()
        };

        w.global::<SearchBridge>()
            .set_results(ModelRc::new(VecModel::from(results)));
    });
}
