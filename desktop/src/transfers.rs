use nectan_core::format::BinaryBytes;
use nectan_core::protocol::NectanState;
use nectan_core::transfers::{PendingTransfer, TransferStatus, Transfers};
use slint::{ComponentHandle, Model, ModelNotify, ModelRc, ModelTracker, ToSharedString, Weak};
use std::fmt::format;
use std::rc::Rc;
use std::sync::atomic::Ordering::Relaxed;
use std::{cell::RefCell, sync::Arc};

use crate::Transfer;
use crate::state::ui_state;
use crate::{NectanWindow, TransfersBridge};

/// One transfer: backend handle + cached UI row.
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
        TransferStatus::Downloading => crate::TransferStatus::Processing,
        TransferStatus::Finished => crate::TransferStatus::Finished,
    };

    let mbps = p.speed.load(Relaxed) as f32 / 1_000_000.0;
    let speed_text = format!("{mbps} MB/s").into();

    Transfer {
        id: p.id.to_shared_string(),
        outcoming: !p.direction.is_incoming(),
        percent_text: format!("{:.2}%", progress * 100.0).into(),
        progress,
        receiver_name,
        size_text: BinaryBytes(p.total_size).to_string().into(),
        speed_text,
        status,
        title: p.name.clone().into(),
        total_files: p.total_files as i32,
    }
}

pub struct TransfersModel {
    pool: Arc<Transfers>,
    transfers: RefCell<Vec<TransferObject>>,
    notify: ModelNotify,
    /// Single files list: only one transfer's contents visible at a time.
    files: Rc<LazyFiles>,
}

/// Lazy loader for the contents of ONE selected transfer.
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

    /// Switch source. `None` = close list.
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
            .map_or(0, |s| s.total_entries as usize)
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        let src = self.src.borrow();
        let item = src.as_ref()?.get_item(row as u32).ok()?;

        if row >= self.row_count() {
            return None;
        }

        let progress = item.sent_bytes as f32;
        let percent_text = format!("{:.2}%", progress * 100.0).into();

        println!("Item progress {}", progress);

        Some(crate::TransferItem {
            name: item.name().into(),
            percent_text,
            size_text: BinaryBytes(item.size).to_string().into(),
            progress,
        })
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
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
        let src = self
            .transfers
            .borrow()
            .iter()
            .find(|t| t.pending.id.to_string() == id)
            .map(|t| t.pending.clone());
        self.files.set_source(src);
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

pub fn update_transfers(w: &Weak<NectanWindow>) {
    let _ = w.upgrade_in_event_loop(move |_w| {
        ui_state().transfers().update_all();
    });
}

pub fn set_transfers(w: &Weak<NectanWindow>, _state: &NectanState) {
    tracing::info!("Updating transfers.");
    let _ = w.upgrade_in_event_loop(move |_w| {
        ui_state().transfers().update_all();
    });
}

/// UI picked transfer whose files to show.
pub fn handle_lazyfiles_source(w: &NectanWindow) {
    w.global::<TransfersBridge>()
        .on_selected_lazyfiles_source(|id| {
            ui_state().transfers().select_files(id.as_str());
        });
}

/// Updates range of the transfer's contents list.
pub fn handle_refresh_items_list(w: &NectanWindow) {
    w.global::<TransfersBridge>()
        .on_refresh_range(move |first, count| {
            ui_state()
                .transfers()
                .files
                .refresh_range(first.max(0) as usize, count.max(0) as usize);
        });
}

pub fn handle_refresh_transfers_list(w: &NectanWindow) {
    w.global::<TransfersBridge>().on_refresh_transfers(move || {
        let transfers = ui_state().transfers();
        for i in 0..transfers.row_count() {
            transfers.update_row(i);
        }
    });
}

pub fn init_models(w: &NectanWindow) {
    let m = ui_state().transfers(); // Rc<TransfersModel>
    let b = w.global::<TransfersBridge>();
    b.set_transfers(ModelRc::from(m.clone()));
    b.set_files(ModelRc::from(m.files()));
}

pub fn handle_transfer_controls(w: &NectanWindow, state: Arc<NectanState>) {
    let bridge = w.global::<TransfersBridge>();

    let s = state.clone();
    bridge.on_delete_transfer(move |_id| {
        let _s = s.clone();
        // tokio::spawn(async move {
        //     let _ = _s.transfers_pool.delete_transfer(transfer_id).await;
        // });
    });

    bridge.on_pause_resume_transfer(move |_id| {
        let _s = state.clone();
        // tokio::spawn(async move {
        //     let _ = _s
        //         .transfers_pool
        //         .pause_resume_transfer(transfer_id, true)
        //         .await;
        // });
    });
}
