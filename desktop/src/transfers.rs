use nectan_core::protocol::NectanState;
use slint::{ComponentHandle, Model, ModelNotify, ModelRc, ModelTracker, Timer, TimerMode, Weak};
use std::{cell::RefCell, sync::Arc, time::Duration};
use uuid::Uuid;

use crate::{NectanWindow, TransferItem, TransfersBridge, state::ui_state};

pub struct TransfersModel {
    transfers: RefCell<Vec<TransferItem>>,
    notify: ModelNotify,
}

impl TransfersModel {
    pub fn new() -> Self {
        Self {
            transfers: RefCell::new(Vec::new()),
            notify: ModelNotify::default(),
        }
    }
    pub fn update_all(&self, new_transfers: Vec<TransferItem>) {
        *self.transfers.borrow_mut() = new_transfers;
        self.notify.reset();
    }

    pub fn update_row(&self, row: usize, t: TransferItem) {
        self.transfers.borrow_mut()[row] = t;
        self.notify.row_changed(row);
    }
}

impl Model for TransfersModel {
    type Data = TransferItem;

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
pub fn update_transfers_list(_w: &Weak<NectanWindow>, _state: &NectanState) {
    tracing::info!("Updating transfers.");
    let model = ui_state().transfers();
    let t: Vec<TransferItem> = vec![TransferItem {
        id: Uuid::new_v4().to_string().into(),
        files: ModelRc::default(),
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

    let _ = _w.upgrade_in_event_loop(move |w| {
        let timer = Box::leak(Box::new(Timer::default()));
        timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
            let ui = ui_state().transfers();
            let model_clone = ui.clone();

            for i in 0..model_clone.row_count() {
                let mut item = model_clone.row_data(i).unwrap();
                item.progress = (item.progress + (0.01 * 2.0 * (i + 1) as f32)) % 1.0;
                item.percent_text = format!("{:.3}%", item.progress * 100.0).into();
                model_clone.update_row(i, item);
            }
        });
    });
}
