use ignore::WalkBuilder;
use nectan_core::common::{common_parent, gen_transfer_name};
use nectan_core::devices::device_id_from_base64;
use nectan_core::format::{DecimalBytes, RoundedDecimalBytes};
use nectan_core::protocol::{NectanState, TransferOffer, send_text};
use nectan_core::transfers::{
    PendingTransfer, TransferOfferError, TransferStatus, Transfers,
    send_contents,
};
use nectan_core::walker::Walker;
use rfd::FileHandle;
use slint::{
    ComponentHandle, Global, Model, ModelNotify, ModelRc,
    ModelTracker, ToSharedString, VecModel, Weak,
};
use std::fs::DirEntry;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::Duration;
use std::{cell::RefCell, sync::Arc};
use tracing::info;
use uuid::Uuid;

use crate::OfferEntry;
use crate::Transfer;
use crate::devices::update_devices;
use crate::state::ui_state;
use crate::window::show_error;
use crate::{App, TransfersBridge};
use crate::{IncomingTransferOffer, OutcomingTransferModalBridge};
use crate::{IncomingTransferOfferBridge, SendModalState, TreeNode};

/// Lazy loader for the contents of selected transfer.
pub struct LazyFiles {
    src: RefCell<Option<Arc<PendingTransfer>>>,
    notify: ModelNotify,
}

impl LazyFiles {
    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            src: RefCell::new(None),
            notify: ModelNotify::default(),
        })
    }

    pub fn set_source(&self, src: Option<Arc<PendingTransfer>>) {
        *self.src.borrow_mut() = src;
        self.notify.reset();
    }

    pub fn refresh_range(&self, first: usize, count: usize) {
        let n = self.row_count();
        let start = first.min(n);
        for i in start..(start + count).min(n) {
            self.notify.row_changed(i);
        }
    }
}

impl Model for LazyFiles {
    type Data = crate::TransferItem;

    fn row_count(&self) -> usize {
        self.src
            .borrow()
            .as_ref()
            .map_or(0, |s| s.total_items as usize)
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        let src = self.src.borrow();
        let item = src.as_ref()?.get_item(row as u32).ok()?;

        if row >= self.row_count() {
            return None;
        }

        let progress = item.sent_bytes as f32 / item.size as f32;
        let percent_text = format!("{:.2}%", progress * 100.0).into();

        Some(crate::TransferItem {
            name: item.name().into(),
            percent_text,
            progress,
            size_text: if item.is_dir {
                "-".into()
            } else {
                DecimalBytes(item.size).to_string().into()
            },
        })
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
}

#[derive(Default)]
pub struct OutcomingTransfer {
    pub walker: Mutex<Option<Walker>>,
    pub total: AtomicU64,
    pub ready: AtomicBool,
    pub match_builder: Mutex<Option<WalkBuilder>>,
}

pub struct TransferObject {
    pending: Arc<PendingTransfer>,
    ui: Transfer,
}

impl TransferObject {
    fn new(pending: Arc<PendingTransfer>) -> Self {
        let ui = build_row(&pending);
        Self { pending, ui }
    }
}

fn build_row(p: &PendingTransfer) -> Transfer {
    let sent = p.sent_bytes.load(Relaxed) as f32;
    let progress = if p.total_size == 0 {
        0.0
    } else {
        sent / p.total_size as f32
    };

    let receiver_name = p.target_name.to_string().into();
    let status = match p.status() {
        TransferStatus::Downloading => {
            crate::TransferStatus::Processing
        }
        TransferStatus::Finished => crate::TransferStatus::Finished,
        TransferStatus::CancelledByRemote => {
            crate::TransferStatus::Cancelled
        }
    };

    let bps = p.speed.load(Relaxed);
    // Convert bits/s to mbps
    let mbps = bps as f32 / (1_000_000.0);
    let speed_text = format!("{mbps:.2} Mbps").into();

    Transfer {
        id: p.id.to_shared_string(),
        outcoming: !p.direction.is_incoming(),
        percent_text: format!("{:.2}%", progress * 100.0).into(),
        progress,
        receiver_name,
        size_text: DecimalBytes(p.total_size).to_string().into(),
        speed_text,
        status,
        title: p.name.clone().into(),
        total_items: p.total_items as i32,
    }
}

pub struct TransfersModel {
    pool: Arc<Transfers>,
    transfers: RefCell<Vec<TransferObject>>,
    notify: ModelNotify,
    /// Single files list: only one transfer's contents visible at a time.
    files: Rc<LazyFiles>,
}

impl TransfersModel {
    pub fn new(t: Arc<Transfers>) -> Self {
        Self {
            pool: t,
            transfers: RefCell::new(Vec::new()),
            notify: ModelNotify::default(),
            files: LazyFiles::new(),
        }
    }

    pub fn files(&self) -> Rc<LazyFiles> {
        self.files.clone()
    }

    pub fn update_all(&self) {
        let new: Vec<TransferObject> = self
            .pool
            .get_pending_transfers()
            .iter()
            .map(|t| TransferObject::new(t.clone()))
            .collect();
        *self.transfers.borrow_mut() = new;
        self.notify.reset();
    }

    pub fn update_row(&self, row: usize) {
        {
            let mut transfers = self.transfers.borrow_mut();
            let Some(t) = transfers.get_mut(row) else {
                return;
            };
            t.ui = build_row(&t.pending);
        }
        self.notify.row_changed(row);
    }

    pub fn select_files(&self, id: &str) {
        let id = Uuid::parse_str(id).unwrap();
        let src = self
            .transfers
            .borrow()
            .iter()
            .find(|t| t.pending.id == id)
            .map(|t| t.pending.clone());
        self.files.set_source(src);
    }

    pub fn remove(&self, id: Uuid) {
        let mut transfers = self.transfers.borrow_mut();
        if let Some(pos) =
            transfers.iter().position(|t| t.pending.id == id)
        {
            transfers.swap_remove(pos);
            drop(transfers);
            self.notify.reset();
        }
    }
}

impl Model for TransfersModel {
    type Data = Transfer;

    fn row_count(&self) -> usize {
        self.transfers.borrow().len()
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        self.transfers.borrow().get(row).map(|t| t.ui.clone())
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
}

pub fn update_transfers(w: &Weak<App>) {
    let _ = w.upgrade_in_event_loop(move |_w| {
        ui_state().transfers().update_all();
    });
}

pub fn set_transfers(w: &Weak<App>, _state: &NectanState) {
    info!("Updating transfers.");
    let _ = w.upgrade_in_event_loop(move |_w| {
        ui_state().transfers().update_all();
    });
}

pub fn handle_transfers(w: &App, s: Arc<NectanState>) {
    let model = Rc::new(TransfersModel::new(s.transfers.clone()));
    TransfersBridge::get(w)
        .set_transfers(ModelRc::from(model.clone()));
    ui_state().transfers_model.set(model).ok().unwrap();
    init_models(w);
    handle_scan_files(w);
    handle_scan_folders(w);
    handle_cancel_walker(w);
    handle_outcoming(w, s.clone());
    handle_refresh_items_list(w);
    handle_refresh_transfers_list(w);
    handle_send(w, s.clone());
    handle_transfer_controls(w, s.clone());
}

fn init_models(w: &App) {
    let m = ui_state().transfers();

    w.global::<TransfersBridge>()
        .set_transfers(ModelRc::from(m.clone()));

    w.global::<TransfersBridge>()
        .set_files(ModelRc::from(m.files()));
}

/// UI picked transfer whose files to show.
fn handle_lazyfiles_source(w: &App) {
    w.global::<TransfersBridge>().on_selected_lazyfiles_source(
        |id| {
            ui_state().transfers().select_files(id.as_str());
        },
    );
}

/// Updates range of the transfer's contents list.
fn handle_refresh_items_list(w: &App) {
    w.global::<TransfersBridge>().on_refresh_range(
        move |first, count| {
            ui_state().transfers().files.refresh_range(
                first.max(0) as usize,
                count.max(0) as usize,
            );
        },
    );
}

fn handle_refresh_transfers_list(w: &App) {
    w.global::<TransfersBridge>().on_refresh_transfers(move || {
        let transfers = ui_state().transfers();
        for i in 0..transfers.row_count() {
            transfers.update_row(i);
        }
    });
}

fn handle_transfer_controls(w: &App, state: Arc<NectanState>) {
    let b = w.global::<TransfersBridge>();
    let s = state.clone();
    b.on_delete_transfer(move |id| {
        let id = Uuid::parse_str(&id).unwrap();
        ui_state().transfers().remove(id);
        let s = s.clone();
        tokio::spawn(async move {
            let _ = s.transfers.delete_transfer(id).await;
        });
    });
}

fn handle_scan_files(w: &App) {
    let weak = w.as_weak();
    w.global::<OutcomingTransferModalBridge>().on_select_files(
        move || {
            let weak = weak.clone();
            tokio::spawn(async move {
                if let Some(h) =
                    rfd::AsyncFileDialog::new().pick_files().await
                    && !h.is_empty()
                {
                    let paths = filehandle_to_paths(h);
                    open_send_modal(weak, paths);
                }
            });
        },
    );
}
pub fn handle_scan_folders(w: &App) {
    let weak = w.as_weak();
    w.global::<OutcomingTransferModalBridge>()
        .on_select_folders(move || {
            let weak = weak.clone();
            tokio::spawn(async move {
                let Some(h) =
                    rfd::AsyncFileDialog::new().pick_folders().await
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

fn filehandle_to_paths(h: Vec<FileHandle>) -> Vec<PathBuf> {
    h.iter().map(|h| h.path().to_path_buf()).collect()
}

fn handle_cancel_walker(w: &App) {
    w.global::<OutcomingTransferModalBridge>()
        .on_transfer_walk_cancelled(move || {
            if let Some(walker) = ui_state().walker().take() {
                walker.stop();
            }
        });
}

pub fn open_send_modal(w: Weak<App>, paths: Vec<PathBuf>) {
    println!("Starting walker for {paths:#?}");

    let common = common_parent(&paths);
    let builder = WalkBuilder::new(common);

    let walker = Walker::new(paths.clone());

    // Store the walker in ui state so it can be cancelled
    let walker2 = walker.clone();

    let _ = w.upgrade_in_event_loop(move |w| {
        {
            let state = ui_state();
            *state
                .outcoming_transfer()
                .match_builder
                .lock()
                .unwrap() = Some(builder);

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

fn spawn_walk_notifier(walker: Walker, w: Weak<App>) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(18));

            let total_entries = walker.total_entries();
            let total_size = walker.total_size();

            let _ = w.upgrade_in_event_loop(move |w| {
                let bridge =
                    w.global::<OutcomingTransferModalBridge>();
                let size_str =
                    RoundedDecimalBytes(total_size).to_string();
                bridge.set_walk_total_entries(total_entries as i32);
                bridge.set_walk_total_size(size_str.into());
            });
            if walker.finished() {
                info!("Walker finished. Ready to send.");
                let _ = w.upgrade_in_event_loop(move |w| {
                    let bridge =
                        w.global::<OutcomingTransferModalBridge>();
                    let size_str =
                        RoundedDecimalBytes(total_size).to_string();
                    bridge
                        .set_walk_total_entries(total_entries as i32);
                    bridge.set_walk_total_size(size_str.into());
                    bridge.set_walk_finished(true);
                });
                return;
            }
        }
    });
}

fn handle_send(w: &App, s: Arc<NectanState>) {
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
                let (delivered_tx, delivered_rx) = tokio::sync::oneshot::channel();

                let w = weak.clone();
                tokio::spawn(async move{
                    if let Ok(_delivered) = delivered_rx.await{
                        let _ = w.upgrade_in_event_loop(move |w| {
                            let b= w.global::<OutcomingTransferModalBridge>();
                            b.set_send_state(SendModalState::WaitingForResponse);
                        });

                    }

                });

                match send_contents(&s, destination_device, info, root_path, delivered_tx).await {
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

fn handle_outcoming(w: &App, s: Arc<NectanState>) {
    let b = w.global::<OutcomingTransferModalBridge>();
    let weak = w.as_weak();
    b.on_send_text(move |target, content| {
        let target = device_id_from_base64(&target).unwrap();
        let w = weak.clone();
        let state = s.clone();

        tokio::spawn(async move {
            if let Err(e) =
                send_text(&state, target, content.into()).await
            {
                show_error(
                    &w,
                    "Error",
                    format!("Failed to send text message. {}", e),
                );
            }
        });
    });

    let weak = w.as_weak();
    b.on_toggle_node(move |id, path| {
        let Some(w) = weak.upgrade() else {
            return;
        };
        let id = id as usize;
        let outcoming = ui_state().outcoming_transfer();
        let weak = weak.clone();
        let bridge = w.global::<OutcomingTransferModalBridge>();
        let mut nodes: Vec<TreeNode> =
            bridge.get_nodes().iter().collect();
        if nodes[id].is_file {
            return;
        }
        let parent_depth = nodes[id].depth;

        if nodes[id].expanded {
            nodes[id].expanded = false;

            // Close the nodes
            let mut end = id + 1;
            while end < nodes.len() && nodes[end].depth > parent_depth
            {
                end += 1;
            }
            nodes.drain(id + 1..end);

            let model = VecModel::from(nodes);
            bridge.set_nodes(ModelRc::from(Rc::new(model)));
            return;
        }

        nodes[id].expanded = true;

        // Need to build the walker beacuse the pathtree from walker is yielded after the walk is finished
        std::thread::spawn(move || {
            match std::fs::read_dir(path.clone()) {
                Ok(dir) => {
                    let mut lock =
                        outcoming.match_builder.lock().unwrap();
                    let builder = lock.as_mut().unwrap();

                    let path = PathBuf::from(path.to_string());
                    builder.add(path);
                    let mut matchers = builder.build_matchers();

                    // Match children of the node with ignore patterns
                    let children: Vec<DirEntry> =
                        dir.flatten().collect();
                    let mut result = Vec::new();
                    'outer: for entry in &children {
                        let Ok(is_dir) =
                            entry.file_type().map(|t| t.is_dir())
                        else {
                            continue;
                        };
                        for matcher in &mut matchers {
                            if matcher
                                .matched(entry.path(), is_dir)
                                .is_ignore()
                            {
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
                            path: entry
                                .path()
                                .to_string_lossy()
                                .to_shared_string(),
                        };
                        result.push(node);
                    }
                    // Append the nodes to the tree
                    nodes.splice((id + 1)..(id + 1), result);

                    let _ = weak.upgrade_in_event_loop(move |w| {
                        let bridge = w
                            .global::<OutcomingTransferModalBridge>();
                        let model = VecModel::from(nodes);
                        bridge
                            .set_nodes(ModelRc::from(Rc::new(model)));
                    });
                }
                Err(e) => {
                    show_error(
                        &weak,
                        "Failed to read directory",
                        e.to_string(),
                    );
                }
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

const MIN_BPS: f32 = 1_000.0;
const MAX_BPS: f32 = 10_000_000_000.0;

/// Exponential speed map
pub fn map_speed(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    MIN_BPS * (MAX_BPS / MIN_BPS).powf(t)
}

pub fn unmap_speed(bps: f32) -> f32 {
    let bps = bps.clamp(MIN_BPS, MAX_BPS);
    (bps / MIN_BPS).ln() / (MAX_BPS / MIN_BPS).ln()
}

pub fn format_speed(bps: f32) -> String {
    const UNITS: [&str; 4] = ["Bps", "Kbps", "Mbps", "Gbps"];
    const LAST: usize = UNITS.len() - 1;

    let mut value = bps.max(0.0);
    let mut unit = 0;

    while unit < LAST && value >= 1000.0 {
        value /= 1000.0;
        unit += 1;
    }

    let rounded = (value * 10.0).round() / 10.0;
    if rounded >= 1000.0 && unit < LAST {
        value = rounded / 1000.0;
        unit += 1;
    }

    let s = format!("{:.1}", value);
    let s = s.trim_end_matches('0').trim_end_matches('.');

    format!("{} {}", s, UNITS[unit])
}
