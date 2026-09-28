use nectan_core::format::BinaryBytes;
use nectan_core::protocol::NectanState;
use nectan_core::transfers::{self, PendingTransfer, PendingTransfers, Transfers};
use slint::{ComponentHandle, Model, ModelNotify, ModelRc, ModelTracker, Timer, TimerMode, Weak};
use std::rc::Rc;
use std::sync::RwLock;
use std::{cell::RefCell, sync::Arc, time::Duration};
use uuid::Uuid;

use crate::Transfer;
use crate::{NectanWindow, TransferItem, TransfersBridge, state::ui_state};

pub struct TransfersModel {
    transfers: RefCell<Vec<Transfer>>,
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
    pub fn new() -> Self {
        Self {
            transfers: RefCell::new(Vec::new()),
            notify: ModelNotify::default(),
            files: LazyFiles::new(),
        }
    }
    pub fn update_all(&self, new_transfers: Vec<Transfer>) {
        *self.transfers.borrow_mut() = new_transfers;
        self.notify.reset();
    }

    pub fn update_row(&self, row: usize, t: Transfer) {
        self.transfers.borrow_mut()[row] = t;
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
        self.transfers.borrow().get(row).cloned()
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
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

pub async fn update_transfers(_w: &Weak<NectanWindow>, _state: &NectanState) {
    // let transfers = _state.transfers.get_transfers().iter().map(|t|TransferItem{status: "Dow"});
    // let _ = _w.upgrade_in_event_loop(move |w| {});
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
            let mut item = transfers.row_data(i).unwrap();
            item.progress = (item.progress + (0.01 * 2.0 * (i + 1) as f32)) % 1.0;
            item.percent_text = format!("{:.3}%", item.progress * 100.0).into();
            transfers.update_row(i, item);
        }
    });
}

pub fn set_transfers(_w: &Weak<NectanWindow>, state: &NectanState) {
    tracing::info!("Updating transfers.");
    let event_tx = state.app_event_tx.clone();

    let _ = _w.upgrade_in_event_loop(move |w| {
        let model = ui_state().transfers();
        let lazy = LazyFiles::new();
        lazy.set_source(Some(Arc::new(PendingTransfer::default(10_000, event_tx))));

        let t: Vec<Transfer> = vec![Transfer {
            id: Uuid::new_v4().to_string().into(),
            total_files: 10_000,
            items: ModelRc::from(lazy),
            outcoming: true,
            percent_text: "12.312%".into(),
            progress: 0.12,
            receiver_name: "Zbyszek".into(),
            speed_text: "123 MB/S".into(),
            status: "Downloading".into(),
            title: "Gold Fish".into(),
            size_text: "123 GB".into(),
        }];
        model.update_all(t);

        // let timer = Box::leak(Box::new(Timer::default()));
        // timer.start(
        //     TimerMode::Repeated,
        //     Duration::from_millis(1000),
        //     move || {
        //         let transfers = ui_state().transfers();
        //         for i in 0..transfers.row_count() {
        //             let mut item = transfers.row_data(i).unwrap();
        //             item.progress = (item.progress + (0.01 * 2.0 * (i + 1) as f32)) % 1.0;
        //             item.percent_text = format!("{:.3}%", item.progress * 100.0).into();
        //             transfers.update_row(i, item);
        //         }
        //     },
        // );
    });
}

// impl Model for LazyFiles {
//     type Data = TransferItem;
//
//     fn row_count(&self) -> usize {
//         0
//         // self.src.read().unwrap().len()
//     }
//     fn row_data(&self, row: usize) -> Option<FileData> {
//         let g = self.src.read().unwrap();
//         g.get(row).map(|f| {
//             let p = if f.size == 0 {
//                 0.0
//             } else {
//                 f.done as f32 / f.size as f32
//             };
//             FileData {
//                 name: f.name.as_str().into(),
//                 size_text: human_size(f.size).into(),
//                 percent_text: format!("{:.0}%", p * 100.0).into(),
//                 progress: p,
//             }
//         })
//     }
//
//     fn model_tracker(&self) -> &dyn ModelTracker {
//         &self.notify
//     }
// }
