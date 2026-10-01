use nectan_core::format::BinaryBytes;
use nectan_core::protocol::NectanState;
use nectan_core::transfers::{PendingTransfer, PendingTransfers, Transfers};
use slint::{
    ComponentHandle, Model, ModelNotify, ModelRc, ModelTracker, Timer, TimerMode, ToSharedString,
    Weak,
};
use std::rc::Rc;
use std::time::Instant;
use std::{cell::RefCell, sync::Arc, time::Duration};
use uuid::Uuid;

use crate::Transfer;
use crate::{NectanWindow, TransferItem, TransfersBridge, state::ui_state};

// idk how to call that crap
pub struct TransferObject {
    pending: Arc<PendingTransfer>,
    ui_shit: Transfer,
}

pub struct TransfersModel {
    pool: Arc<Transfers>,
    transfers: RefCell<Vec<TransferObject>>,
    notify: ModelNotify,
    files: Rc<LazyFiles>,
}

/// Lazy (files) files loader for transfer contents
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
        Some(crate::TransferItem {
            name: item.name().into(),
            percent_text: format!(
                "{:.0}%",
                (item.sent_bytes as f32 / item.size as f32).max(100.0)
            )
            .into(),
            size_text: BinaryBytes(item.size).to_string().into(),
            progress: 0.12,
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
    pub fn update_all(&self) {
        let start = Instant::now();
        println!("Map took {:?}", start.elapsed());
        let pending = self.pool.get_pending_transfers();
        let new = pending
            .iter()
            .map(|t| TransferObject {
                pending: t.clone(),
                ui_shit: Transfer {
                    outcoming: false,
                    percent_text: "12%".to_shared_string(),
                    progress: 0.12,
                    receiver_name: "Huj".to_shared_string(),
                    speed_text: "piz".to_shared_string(),
                    status: "huj".into(),
                    title: "Kurwy".into(),
                    total_files: 696969,
                    id: t.id.to_shared_string(),
                    size_text: "123 GB".into(),
                    items: ModelRc::new(LazyFiles::new()),
                },
            })
            .collect();
        *self.transfers.borrow_mut() = new;
        self.notify.reset();
    }

    pub fn update_row(&self, row: usize) {
        let t = &mut self.transfers.borrow_mut()[row];
        let p = &t.pending;
        let new = Transfer {
            id: p.id.to_shared_string(),
            items: t.ui_shit.items.clone(),
            outcoming: !p.direction.is_incoming(),
            percent_text: "pizda".into(),
            progress: 0.0,
            receiver_name: "Cwel".into(),
            size_text: "HUj".into(),
            speed_text: "Kurwa".into(),
            status: "sex".into(),
            title: "Jukuwry".into(),
            total_files: 123,
        };
        // let
        t.ui_shit = new;
        self.notify.row_changed(row);
    }

    pub fn open_files(&self, src: Option<Arc<PendingTransfer>>) {
        self.files.set_source(src);
    }
}

impl Model for TransfersModel {
    type Data = Transfer;

    fn row_count(&self) -> usize {
        self.transfers.borrow().len()
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        self.transfers.borrow().get(row).map(|t| t.ui_shit.clone())
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
}

/// Called when transfer state changed or
pub fn update_transfers(w: &Weak<NectanWindow>, state: &NectanState) {
    let _ = w.upgrade_in_event_loop(move |_w| {
        ui_state().transfers().update_all();
    });
}

/// Updates range of the transfer's contents list
pub fn handle_refresh_items_list(w: &NectanWindow) {
    let bridge = w.global::<TransfersBridge>();
    bridge.on_refresh_range(move |first, count| {
        ui_state()
            .transfers()
            .files
            .refresh_range(first.max(0) as usize, count.max(0) as usize);
    });
}

pub fn handle_refresh_transfers_list(w: &NectanWindow) {
    let bridge = w.global::<TransfersBridge>();
    bridge.on_refresh_transfers(move || {
        let transfers = ui_state().transfers();
        for i in 0..transfers.row_count() {
            transfers.update_row(i);
        }
    });
}

pub fn set_transfers(_w: &Weak<NectanWindow>, state: &NectanState) {
    tracing::info!("Updating transfers.");
    let event_tx = state.app_event_tx.clone();
    let _ = _w.upgrade_in_event_loop(move |_w| {
        let model = ui_state().transfers();
        // let lazy = LazyFiles::new();
        // lazy.set_source(Some(Arc::new(PendingTransfer::default(10_000, event_tx))));
        model.update_all();
    });
}

// pub fn handle_transfer_controls(w: &NectanWindow, state: Arc<NectanState>) {
//     let bridge = w.global::<TransfersBridge>();
//     let s = state.clone();
//     bridge.on_delete_transfer(move |id| {
//         let transfer_id = Uuid::parse_str(&id).unwrap();
//         let s = s.clone();
//         tokio::spawn(async move {
//             let _ = s.transfers_pool.delete_transfer(transfer_id).await;
//         });
//     });
//
//     bridge.on_pause_resume_transfer(move |id| {
//         let transfer_id = Uuid::parse_str(&id).unwrap();
//         let s = state.clone();
//         tokio::spawn(async move {
//             let _ = s
//                 .transfers_pool
//                 .pause_resume_transfer(transfer_id, true)
//                 .await;
//         });
//     });
// }
