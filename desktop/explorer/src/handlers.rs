use anyhow::bail;
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
    protocol::{
        ConnectStatus, NectanState, TransferOffer, TransferOfferRequest,
        connect,
    },
    storage::SpeedLimits,
    transfers::{TransferOfferError, send_contents},
    walker::Walker,
};
use rfd::FileHandle;
use slint::{
    ComponentHandle, Model, ModelRc, TimerMode, VecModel, Weak,
    invoke_from_event_loop,
};
use slint::{
    ToSharedString,
    winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent},
};
use std::{
    fs::DirEntry,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, atomic::Ordering::Relaxed},
    time::Duration,
};
use tokio::sync::mpsc::Receiver;
use tracing::{error, info};
use uuid::Uuid;

use crate::{
    AddDeviceBridge, ConnectionOfferBridge, Error, IncomingTransferOffer,
    IncomingTransferOfferBridge, LookupState, NectanWindow, OfferEntry,
    OutcomingTransferModalBridge, SendModalState, SettingsBridge,
    TransfersBridge, TreeNode, WindowBridge,
    devices::update_devices,
    state::ui_state,
    transfers::{format_speed, map_speed, unmap_speed, update_transfers},
};

pub const MB: f32 = 1024.0 * 1024.0;

pub fn show_error(
    weak: &slint::Weak<NectanWindow>,
    title: impl Into<slint::SharedString>,
    msg: impl Into<slint::SharedString>,
) {
    let (title, msg) = (title.into(), msg.into());
    let _ = weak.upgrade_in_event_loop(move |w| {
        w.global::<Error>().invoke_show_error(title, msg);
    });
}

pub fn handle_window_controls(w: &NectanWindow) {
    let bridge = w.global::<WindowBridge>();
    let w_weak = w.as_weak();

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
fn notify(offer_from: &str) {
    // let mut n = notify_rust::Notification::new();
    // n.appname("Nectan")
    //     .summary("Nectan")
    //     .body(&format!("You have a new offer from {}.", offer_from));
    //
    // #[cfg(target_os = "linux")]
    // {
    //     n.icon("nectan")
    //         .hint(Hint::Category("email".to_owned()))
    //         .hint(Hint::DesktopEntry("nectan".to_owned()))
    //         .sound_name("message-new-instant");
    // }
    //
    // #[cfg(target_os = "macos")]
    // {
    //     n.sound_name("Ping");
    // }
    //
    // #[cfg(target_os = "windows")]
    // {
    //     n.sound_name("Mail"); // maps to ms-winsoundevent:Notification.Mail
    // }
    //
    // let _ = n.show();
    //
    // let r = notify_rust::Notification::new()
    //     .appname("Nectan")
    //     .summary("Nectan ")
    //     .body("Test {}.")
    //     .icon("Nectan")
    //     .hint(Hint::Category("email".to_owned()))
    //     .show();
}

pub async fn event_listener(
    w: Weak<NectanWindow>,
    mut rx: Receiver<AppEvent>,
    state: Arc<NectanState>,
) {
    notify("Siema");

    while let Some(msg) = rx.recv().await {
        match msg {
            AppEvent::Autoaccepted { from } => {
                info!("Autoaccepted {from}");
            }
            AppEvent::TransfersUpdated => {
                update_transfers(&w);
            }
            AppEvent::IncomingTransferOffer { offer } => {
                show_transfer_offer(&w, offer);
            }
            AppEvent::ConnectionOffer { offer } => {
                show_connection_offer(&w, offer);
            }
            AppEvent::FoundNearby => {
                update_devices(&w, &state);
            }
            AppEvent::Connected { .. } => {
                update_devices(&w, &state);
            }
            AppEvent::TransferOfferDelivered => {
                info!("Delivered transfer offer.");
                println!("Delivered transfer offer.");
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

pub fn handle_incoming_transfer_offer(
    w: &NectanWindow,
    state: Arc<NectanState>,
) {
    let b = w.global::<IncomingTransferOfferBridge>();

    let save_to = ui_state().save_to();
    b.set_target_path(save_to.to_string_lossy().to_string().into());

    let weak = w.as_weak();
    b.on_change_destination(move || {
        let weak = weak.clone();
        let fut = async move {
            let weak = weak.clone();
            let Some(h) = rfd::AsyncFileDialog::new().pick_folder().await
            else {
                return;
            };

            let path = h.path().to_path_buf();
            let Some(w) = weak.upgrade() else { return };

            ui_state().set_save_to(&path);
            w.global::<IncomingTransferOfferBridge>()
                .set_target_path(path.to_string_lossy().to_string().into());
        };
        let _ = slint::spawn_local(fut);
    });

    let weak = w.as_weak();
    let s = state.clone();
    b.on_accept(move || {
        info!("Accepted");
        if let Some(w) = weak.upgrade() {
            let b = w.global::<IncomingTransferOfferBridge>();
            b.set_open(false);
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
            b.set_open(false);

            let s = s.clone();
            tokio::spawn(async move {
                s.respond_transfer_offer(UiResponse::Reject {
                    reason: Some("User rejected.".to_string()),
                })
                .await;
            });
        }
    });
}

pub fn show_transfer_offer(
    w: &Weak<NectanWindow>,
    offer: TransferOfferRequest,
) {
    let _ = w.upgrade_in_event_loop(move |w| {
        let b = w.global::<IncomingTransferOfferBridge>();
        let nodes: Vec<_> = offer
            .inner
            .entries
            .iter()
            .map(|e| OfferEntry {
                filename: e
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
                    .into(),
                is_dir: e.is_dir,
                size: DecimalBytes(e.size).to_shared_string(),
            })
            .collect();

        let offer = IncomingTransferOffer {
            from: offer.sender_name.into(),
            total_size: format!("{}", DecimalBytes(1_500)).into(),
            transfer_name: offer.inner.transfer_name.into(),
            total_entries: offer.inner.entries_num as i32,
        };

        b.set_entries(ModelRc::new(VecModel::from(nodes)));
        b.set_offer(offer);
        b.set_open(true);
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
                state
                    .respond_connection_offer(UiResponse::AcceptConnection)
                    .await;
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

    bridge.on_copy_code(|s| {
        let code = s.to_string();

        // println!(
        //     "Copied {code} [{:?}]",
        //     // ui_state().get_clipboard().set_text(&code)
        // );
    });

    bridge.on_digits_only(|s| {
        s.chars()
            .filter(|c| c.is_ascii_digit())
            .take(6)
            .collect::<String>()
            .into()
    });

    bridge.on_format_code(|s| {
        let digits: String =
            s.chars().filter(|c| c.is_ascii_digit()).take(6).collect();
        if digits.len() > 3 {
            format!("{} {}", &digits[..3], &digits[3..]).into()
        } else {
            digits.into()
        }
    });

    bridge.on_request_code(move || {
        let ui = ui_state();
        let weak = ui_weak.clone();

        ui.timer().start(
            TimerMode::Repeated,
            std::time::Duration::from_millis(1000),
            move || {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                let time_left = w.global::<AddDeviceBridge>().get_time_left();
                w.global::<AddDeviceBridge>().set_time_left(time_left - 1);

                if time_left == 0 {
                    let ui = ui_state();
                    ui.timer().stop();
                }
            },
        );

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
                            let code_split =
                                format!("{} {}", &code[0..3], &code[3..6]);
                            bridge.set_our_code(code_split.into());
                        }
                        Err(e) => {
                            ui.global::<Error>().invoke_show_error(
                                "Error".into(),
                                e.to_string().into(),
                            );
                            // bridge.invoke_reset_add_remote();
                            bridge.set_open(false);
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
            ui_weak.upgrade_in_event_loop(|ui| {
                ui.global::<AddDeviceBridge>()
                    .set_lookup_state(LookupState::Checking);
            })?;

            let result = lookup_code(code.to_string(), &state).await;
            if let Err(e) = result {
                match e {
                    CodeLookupError::NotFound => {
                        ui_weak.upgrade_in_event_loop(|ui| {
                            ui.global::<AddDeviceBridge>()
                                .set_lookup_state(LookupState::CodeNotFound);
                        })?;
                    }
                    CodeLookupError::ConnectionFailed => {
                        ui_weak.upgrade_in_event_loop(|ui| {
                            ui.global::<AddDeviceBridge>().set_lookup_state(
                                LookupState::ConnectionFailed,
                            );
                        })?;
                    }
                    CodeLookupError::NectanService => {
                        ui_weak.upgrade_in_event_loop(|ui| {
                            ui.global::<Error>().invoke_show_error(
                                "Nectan failed".into(),
                                "Unexpected error.".into(),
                            );
                        })?;
                    }
                }
                bail!("Code err")
            }

            let addr = result.unwrap();
            ui_weak.upgrade_in_event_loop(|ui| {
                println!("Connecting");
                ui.global::<AddDeviceBridge>()
                    .set_lookup_state(LookupState::Connecting);
            })?;

            let mut rx = connect(state, addr);
            let result = rx.recv().await.unwrap();

            if let ConnectStatus::RequestDelivered = result {
                println!("Request delivered");
                ui_weak.upgrade_in_event_loop(move |ui| {
                    ui.global::<AddDeviceBridge>().set_lookup_state(
                        LookupState::WaitingForRemoteApproval,
                    );
                })?;
                let result = rx.recv().await.unwrap();
                match result {
                    ConnectStatus::UserRejected => {
                        println!("User rejected");
                        ui_weak.upgrade_in_event_loop(move |ui| {
                            ui.global::<AddDeviceBridge>()
                                .set_lookup_state(LookupState::Rejected);
                        })?;
                    }
                    ConnectStatus::Accepted(_device) => {
                        println!("Accepted");
                        ui_weak.upgrade_in_event_loop(move |ui| {
                            ui.global::<AddDeviceBridge>()
                                .set_lookup_state(LookupState::Connected);
                        })?;
                    }
                    ConnectStatus::AuthFailed => {
                        println!("AuthFailed");
                        ui_weak.upgrade_in_event_loop(move |ui| {
                            ui.global::<AddDeviceBridge>()
                                .set_lookup_state(LookupState::Rejected);
                        })?;
                    }
                    ConnectStatus::ConnectionFailed => {
                        println!("Conn failed");
                        ui_weak.upgrade_in_event_loop(move |ui| {
                            ui.global::<AddDeviceBridge>().set_lookup_state(
                                LookupState::ConnectionFailed,
                            );
                        })?;
                    }
                    ConnectStatus::RequestDelivered
                    | ConnectStatus::AlreadyConnected => {
                        panic!("Impossible state")
                    }
                }
                return Ok(());
            }

            match result {
                ConnectStatus::ConnectionFailed => {
                    ui_weak.upgrade_in_event_loop(move |ui| {
                        println!("Connection failed");
                        ui.global::<AddDeviceBridge>()
                            .set_lookup_state(LookupState::ConnectionFailed);
                    })?;
                }
                ConnectStatus::AlreadyConnected => {
                    println!("Already connected");
                    ui_weak.upgrade_in_event_loop(move |ui| {
                        ui.global::<AddDeviceBridge>()
                            .set_lookup_state(LookupState::AlreadyConnected);
                    })?;
                }
                _ => {
                    panic!("Impossible state")
                }
            }
            Ok::<(), anyhow::Error>(())
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
                let Some(h) = rfd::AsyncFileDialog::new().pick_folders().await
                else {
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

    let walker = Walker::new(paths.clone());

    // Store the walker in ui state so it can be cancelled
    let walker2 = walker.clone();

    let _ = w.upgrade_in_event_loop(move |w| {
        {
            let state = ui_state();
            *state.outcoming_transfer().match_builder.lock().unwrap() =
                Some(builder);

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
        bridge.set_open(true);

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
    spawn_walk_notifier(walker, w);
}

fn spawn_walk_notifier(walker: Walker, w: Weak<NectanWindow>) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(18));

            let total_entries = walker.total_entries();
            let total_size = walker.total_size();

            let _ = w.upgrade_in_event_loop(move |w| {
                let bridge = w.global::<OutcomingTransferModalBridge>();
                let size_str = RoundedDecimalBytes(total_size).to_string();
                bridge.set_walk_total_entries(total_entries as i32);
                bridge.set_walk_total_size(size_str.into());
            });
            if walker.finished() {
                info!("Walker finished. Ready to send.");
                let _ = w.upgrade_in_event_loop(move |w| {
                    let bridge = w.global::<OutcomingTransferModalBridge>();
                    let size_str = RoundedDecimalBytes(total_size).to_string();
                    bridge.set_walk_total_entries(total_entries as i32);
                    bridge.set_walk_total_size(size_str.into());
                    bridge.set_walk_finished(true);
                });
                return;
            }
        }
    });
}

pub fn handle_send(w: &NectanWindow, s: Arc<NectanState>) {
    let weak = w.as_weak();
    let bridge = w.global::<OutcomingTransferModalBridge>();

    bridge.on_send(move |destination_id, transfer_name, compression| {
        info!("Sending offer. {transfer_name}. Compression - {compression}");
        if let Some(walk_info) = ui_state().walker().take() {
            let destination_device = device_id_from_base64(&destination_id).unwrap();

            let transfer_id = Uuid::new_v4();
            let entries_num = walk_info.total_entries() as u32;
            let total_size = walk_info.total_size();
            let transfer_name = transfer_name.into();

            let entries = walk_info
                .take_entries()
                .expect("At this point the tree should be Some.")
                .into();

            let info = TransferOffer {
                transfer_id,
                transfer_name,
                entries_num,
                total_size,
                entries,
            };
            let root_path = walk_info.root_path.clone();

            let s = s.clone();
            let weak = weak.clone();
            tokio::spawn(async move {
                match send_contents(&s, destination_device, info, root_path).await {
                    Ok(()) => {
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            let bridge = w.global::<OutcomingTransferModalBridge>();
                            bridge.set_send_state(crate::SendModalState::Accepted);
                        });
                        update_transfers(&weak);
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
                        b.set_open(false);
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
        let mut nodes: Vec<TreeNode> = bridge.get_nodes().iter().collect();
        if nodes[id].is_file {
            return;
        }
        let parent_depth = nodes[id].depth;

        if nodes[id].expanded {
            nodes[id].expanded = false;

            // Close the nodes
            let mut end = id + 1;
            while end < nodes.len() && nodes[end].depth > parent_depth {
                end += 1;
            }
            nodes.drain(id + 1..end);

            let model = VecModel::from(nodes);
            bridge.set_nodes(ModelRc::from(Rc::new(model)));
            return;
        }

        nodes[id].expanded = true;

        // Need to build the walker beacuse the pathtree from walker is yielded after the walk is finished
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
                    let Ok(is_dir) = entry.file_type().map(|t| t.is_dir())
                    else {
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
                        filename: entry
                            .file_name()
                            .to_string_lossy()
                            .to_shared_string(),
                        path: entry.path().to_string_lossy().to_shared_string(),
                    };
                    result.push(node);
                }
                // Append the nodes to the tree
                nodes.splice((id + 1)..(id + 1), result);

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
    let weak = w.as_weak();
    b.on_rescan_dir(move || {
        let wk = weak.clone();
        let Some(w) = weak.upgrade() else { return };
        let b = w.global::<OutcomingTransferModalBridge>();
        b.set_walk_finished(false);
        b.set_walk_total_size("0 B".into());
        b.set_walk_total_entries(0);

        // Reuse old walker
        let state = ui_state();
        let lock = state.walker();
        if let Some(walker) = &*lock {
            println!("Rescanning");
            walker.rescan();

            spawn_walk_notifier(walker.clone(), wk);
        } else {
            println!("Walker not found!");
        }
    })
}

pub fn handle_paste(w: &NectanWindow) {
    // let weak = w.as_weak();
    // w.on_pasted(move || {
    //     let weak = weak.clone();
    //     let ui = ui_state();
    //     let mut cl = ui.get_clipboard();
    //     let contents = cl.get();
    //
    //     if let Ok(list) = contents.file_list() {
    //         let list: Vec<PathBuf> = list
    //             .into_iter()
    //             .map(|p| PathBuf::from(p.to_string_lossy().trim_end_matches(['\r', '\n'])))
    //             .collect();
    //         open_send_modal(weak, list);
    //     } else {
    //         println!("Failed to get files list from clipboard.");
    //     }
    // })
}

pub fn handle_username_change(w: &NectanWindow, s: Arc<NectanState>) {
    let b = w.global::<WindowBridge>();

    if let Some(u) = s.storage.username() {
        b.set_username(u.as_string().into());
    }

    let weak = w.as_weak();
    b.on_change_username(move |username| {
        let Some(w) = weak.upgrade() else { return };

        match Username::new(username) {
            Ok(u) => {
                if let Err(e) = s.storage.update(|s| s.username = Some(u)) {
                    show_error(&weak, "Error", e.to_string());
                }
            }
            Err(e) => {
                show_error(&weak, "Invalid username", e.to_string());
            }
        }
    });
}

pub fn handle_open_transfers(w: &NectanWindow) {
    let weak = w.as_weak();
    w.global::<TransfersBridge>().on_open_transfers(move || {
        update_transfers(&weak);
    });
}

pub fn handle_settings(w: &NectanWindow, s: Arc<NectanState>) {
    let b = w.global::<SettingsBridge>();
    let st = &s.storage;

    b.set_self_accept_nearby(st.read(|s| s.autoaccept_from_nearby));
    b.set_self_accept_favourites(st.read(|s| s.autoaccept_from_nearby));
    b.set_devices_notifications(st.read(|s| s.device_notifications));
    b.set_transfers_notifications(st.read(|s| s.device_notifications));
    b.set_connection_notifications(st.read(|s| s.connection_notifications));

    let state = s.clone();
    let weak = w.as_weak();
    b.on_change_default_dir(move || {
        let weak = weak.clone();
        let s = state.clone();
        tokio::spawn(async move {
            let Some(h) = rfd::AsyncFileDialog::new().pick_folder().await
            else {
                return;
            };
            let path = h.path();
            // Ensure if has writing permission
            if let Err(e) =
                std::fs::write(path.join("test.txt"), b"Permission check")
            {
                show_error(&weak, "Error", e.to_string());
                return;
            }
            let _ = std::fs::remove_file(path);
            let _ = s.storage.update(|s| s.default_destination = path.into());
            let path = path.to_string_lossy().to_shared_string();
            let _ = weak.upgrade_in_event_loop(move |w| {
                w.global::<SettingsBridge>().set_default_download_dir(path);
            });
        });
    });

    let state = s.clone();
    b.on_toggle_accept_nearby(move || {
        let _ = state
            .storage
            .update(|s| s.autoaccept_from_nearby = !s.autoaccept_from_nearby);
    });

    let state = s.clone();
    b.on_toggle_accept_favourites(move || {
        let _ = state.storage.update(|s| {
            s.autoaccept_from_favourites = !s.autoaccept_from_favourites
        });
    });

    b.on_open_github(move || {
        let _ = open::that("https://github.com/karoltrykacz/Nectan");
    });

    b.on_join_discord(move || {
        let _ = open::that("https://discord.gg/your-server");
    });

    b.on_open_privacy(move || {
        let _ = open::that("https://nectan.co/privacy");
    });

    b.on_format_speed(move |speed| format_speed(map_speed(speed)).into());

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
        let _ = state.storage.update(|s| s.speed_limits = speed_limits);
        // TODO update transfers
    });

    // b.on_
}

// fn ext_of(p: &Path) -> String {
//     p.extension()
//         .and_then(|e| e.to_str())
//         .map(|e| e.to_lowercase())
//         .unwrap_or_default()
// }
//
// fn make_preview(path: &Path) -> Option<Image> {
//     let ext = ext_of(path);
//
//     if ext == "svg" || ext == "svgz" {
//         return Image::load_from_path(path).ok();
//     }
//
//     let img = image::open(path).ok()?;
//     let small = img.resize(
//         PREVIEW_SIZE,
//         PREVIEW_SIZE,
//         image::imageops::FilterType::Triangle,
//     );
//     let rgba = small.to_rgba8();
//
//     let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
//         rgba.as_raw(),
//         rgba.width(),
//         rgba.height(),
//     );
//     Some(Image::from_rgba8(buf))
// }
//
// const DIR: &str = "/media/karol/kox/";
// const PREVIEW_SIZE: u32 = 256;
//
// pub fn handle_explorer(w: &NectanWindow) {
//     let Ok(read_dir) = std::fs::read_dir(DIR) else {
//         eprintln!("cannot read {DIR}");
//         return;
//     };
//
//     ui_state().add_path(DIR.into());
//
//     let mut entries: Vec<Entry> = read_dir
//         .filter_map(|e| e.ok())
//         .map(|e| {
//             let path = e.path();
//             let ft = e.file_type().ok();
//             let kind = if ft.map(|t| t.is_symlink()).unwrap_or(false) {
//                 EntryType::Symlink
//             } else if ft.map(|t| t.is_dir()).unwrap_or(false) {
//                 EntryType::Folder
//             } else {
//                 EntryType::File
//             };
//
//             // blocking: decode right here
//             let preview = if matches!(kind, EntryType::File) {
//                 make_preview(&path).unwrap_or_default()
//             } else {
//                 Image::default()
//             };
//
//             Entry {
//                 name: e.file_name().to_string_lossy().to_string().into(),
//                 path: path.to_string_lossy().to_string().into(),
//                 r#type: kind,
//                 preview,
//             }
//         })
//         .collect();
//
//     entries.sort_by_key(|e| {
//         (
//             !matches!(e.r#type, EntryType::Folder),
//             e.name.to_lowercase(),
//         )
//     });
//
//     w.global::<ExplorerBridge>()
//         .set_files(ModelRc::from(Rc::new(VecModel::from(entries))));
//
//     let weak = w.as_weak();
//     w.global::<ExplorerBridge>().on_forward(move || {
//         println!("Forward");
//         let Some(p) = ui_state().next_path() else {
//             println!("No history");
//             return;
//         };
//         load_entry(&weak, &p);
//     });
//
//     let weak = w.as_weak();
//     w.global::<ExplorerBridge>().on_back(move || {
//         println!("Back");
//         let Some(p) = ui_state().last_path() else {
//             println!("No history");
//             return;
//         };
//         load_entry(&weak, &p);
//     });
//
//     let weak = w.as_weak();
//     w.global::<ExplorerBridge>().on_open(move |entry| {
//         let p = entry.path.to_string();
//         let path = Path::new(&p);
//         load_entry(&weak, path);
//     });
//
//     let weak = w.as_weak();
//     w.global::<ExplorerBridge>().on_copy(move |from, to| {
//         // Todo change this to model
//         let Some(w) = weak.upgrade() else { return };
//         let e = w.global::<ExplorerBridge>().get_files();
//         let mut paths = Vec::new();
//         for i in from..to {
//             let Some(r) = e.row_data(i as usize) else {
//                 continue;
//             };
//             let path = PathBuf::from(r.path.to_string());
//             paths.push(path);
//         }
//         println!("Copying {:#?}", paths);
//         let _ = copy_files(&paths);
//     });
// }
// fn load_entry(weak: &Weak<NectanWindow>, path: &Path) {
//     let Ok(read_dir) = std::fs::read_dir(path) else {
//         eprintln!("cannot read {DIR}");
//         return;
//     };
//
//     let mut entries: Vec<Entry> = read_dir
//         .filter_map(|e| e.ok())
//         .map(|e| {
//             let path = e.path();
//             let ft = e.file_type().ok();
//             let kind = if ft.map(|t| t.is_symlink()).unwrap_or(false) {
//                 EntryType::Symlink
//             } else if ft.map(|t| t.is_dir()).unwrap_or(false) {
//                 EntryType::Folder
//             } else {
//                 EntryType::File
//             };
//
//             let preview = if matches!(kind, EntryType::File) {
//                 // make_preview(&path).unwrap_or_default()
//                 Image::default()
//             } else {
//                 Image::default()
//             };
//
//             Entry {
//                 name: e.file_name().to_string_lossy().to_string().into(),
//                 path: path.to_string_lossy().to_string().into(),
//                 r#type: kind,
//                 preview,
//             }
//         })
//         .collect();
//
//     entries.sort_by_key(|e| {
//         (
//             !matches!(e.r#type, EntryType::Folder),
//             e.name.to_lowercase(),
//         )
//     });
//     let Some(w) = weak.upgrade() else { return };
//
//     let b = w.global::<ExplorerBridge>();
//     b.set_files(ModelRc::from(Rc::new(VecModel::from(entries))));
//     b.set_current(path.to_string_lossy().to_string().into());
//
//     println!("Added path {path:?}");
//     ui_state().add_path(path.into());
// }

// use clipboard_rs::Clipboard;
// fn copy_files(paths: &[PathBuf]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//     let ctx = ClipboardContext::new()?;
//     let uris: Vec<String> = paths
//         .iter()
//         .filter_map(|p| Url::from_file_path(p).ok())
//         .map(|u| u.to_string())
//         .collect();
//     ctx.set_files(uris)?;
//     Ok(())
// }

// #[cfg(target_os = "windows")]
// pub fn open_with_dialog(path: &Path) -> std::io::Result<()> {
//     Command::new("rundll32.exe")
//         .arg("shell32.dll,OpenAs_RunDLL")
//         .arg(path)
//         .spawn()?;
//     Ok(())
// }
//

//
// #[cfg(target_os = "linux")]
// pub fn open_with_dialog(path: &Path) -> std::io::Result<()> {
//     use ashpd::desktop::open_uri::OpenFileRequest;
//     use std::os::fd::AsFd;
//     let file = std::fs::File::open(path)?;
//
//     tokio::spawn(async move {
//         let fd = file.as_fd();
//         let _ = OpenFileRequest::default().ask(true).send_file(&fd).await;
//     });
//     Ok(())
// }
//
// #[cfg(target_os = "macos")]
// pub fn open_with_dialog(path: &Path) -> std::io::Result<()> {
//     // mac has no chooser dialog, so user picks the app
//     let app = rfd::FileDialog::new()
//         .set_title("Choose application")
//         .set_directory("/Applications")
//         .add_filter("Application", &["app"])
//         .pick_file();
//
//     if let Some(app) = app {
//         Command::new("open").arg("-a").arg(app).arg(path).spawn()?;
//     }
//     Ok(())
// }
//
// static LIB: &[u8] = include_bytes!(concat!(
//     "/home/karol/vendor/pdfium/linux-x64/lib/libpdfium.so"
// ));
//
// fn load_pdfium() -> Result<Pdfium, PdfiumError> {
//     let dir = std::env::temp_dir().join("nectan-pdfium");
//     std::fs::create_dir_all(&dir).map_err(PdfiumError::IoError)?;
//
//     let path = dir.join(Pdfium::pdfium_platform_library_name());
//     if std::fs::metadata(&path).map(|m| m.len() as usize).ok() != Some(LIB.len()) {
//         std::fs::write(&path, LIB).map_err(PdfiumError::IoError)?;
//     }
//     Ok(Pdfium::new(Pdfium::bind_to_library(&path)?))
// }
//
// thread_local! {
//     static PDFIUM: Result<Pdfium, PdfiumError> = load_pdfium();
// }
//
// pub fn render_pdf_page(path: &str, page_no: i32, width: i32) -> Result<Image, PdfiumError> {
//     PDFIUM.with(|p| {
//         let s = Instant::now();
//         let pdfium = p
//             .as_ref()
//             .map_err(|_| PdfiumError::PdfiumLibraryBindingsAlreadyInitialized)?;
//         let doc = pdfium.load_pdf_from_file(path, None)?;
//         let page = doc.pages().get(page_no)?;
//         let rgba = page
//             .render_with_config(&PdfRenderConfig::new().set_target_width(width))?
//             .as_image()?
//             .to_rgba8();
//         let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
//             rgba.as_raw(),
//             rgba.width(),
//             rgba.height(),
//         );
//         println!("PDF render took {:?}", s.elapsed());
//         Ok(Image::from_rgba8(buf))
//     })
// }
//
// pub fn set_file_preview(w: &NectanWindow) {
//     let b = w.global::<PdfPreviewBridge>();
//     let path = "/home/karol/Downloads/sample.pdf";
//     let r = render_pdf_page(path, 0, 1920);
//     let _ = open::that_detached(path);
//
//     match r {
//         Ok(img) => {
//             b.set_pdf_preview(img);
//         }
//         Err(e) => {
//             println!("Err {e:?}");
//         }
//     }
//
//     let b = w.global::<TextPreviewBridge>();
//     b.on_open_with(|| {
//         // todo get real path
//         let path = Path::new("/home/karol/test.txt");
//         let _ = open_with_dialog(path);
//     });
//
//     let w = w.as_weak();
//     std::thread::spawn(move || {
//         let path = "/home/karol/test.txt";
//         let text = std::fs::read_to_string(&path).unwrap();
//         let doc_name = "big.txt".into();
//         let doc_size = (text.len() as u64).to_string().into();
//         let lines: Vec<SharedString> = text.lines().map(|l| l.into()).collect();
//         let modified = "16 May 2026".into();
//
//         let _ = w.upgrade_in_event_loop(move |w| {
//             let b = w.global::<TextPreviewBridge>();
//             b.set_doc_text(ModelRc::new(VecModel::from(lines)));
//             b.set_doc_size(doc_size);
//             b.set_doc_name(doc_name);
//             b.set_doc_modified(modified);
//         });
//     });
// }
//
//
