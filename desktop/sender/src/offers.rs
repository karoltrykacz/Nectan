use crate::App;
use crate::ConnectionOfferBridge;
use crate::IncomingTransferOffer;
use crate::IncomingTransferOfferBridge;
use crate::OfferEntry;
use crate::state::ui_state;
use nectan_core::messages::ConnectionOffer;
use nectan_core::messages::UiResponse;
use nectan_core::protocol::NectanState;
use nectan_core::{
    format::DecimalBytes, protocol::TransferOfferRequest,
};
use slint::ComponentHandle;
use slint::ModelRc;
use slint::ToSharedString;
use slint::VecModel;
use slint::Weak;
use std::sync::Arc;
use tracing::info;

pub fn handle_offers(w: &App, state: Arc<NectanState>) {
    handle_connection_offer(w, state.clone());
    handle_incoming_transfer_offer(w, state);
}

fn handle_connection_offer(w: &App, state: Arc<NectanState>) {
    let bridge = w.global::<ConnectionOfferBridge>();
    bridge.on_accept(move |accept| {
        let state = state.clone();
        tokio::spawn(async move {
            if accept {
                info!("Accepted connection offer.");
                state
                    .respond_connection_offer(
                        UiResponse::AcceptConnection,
                    )
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
fn handle_incoming_transfer_offer(w: &App, state: Arc<NectanState>) {
    let b = w.global::<IncomingTransferOfferBridge>();

    let save_to = ui_state().save_to();
    b.set_target_path(save_to.to_string_lossy().to_string().into());

    let weak = w.as_weak();
    b.on_change_destination(move || {
        let weak = weak.clone();
        let fut = async move {
            let weak = weak.clone();
            let Some(h) =
                rfd::AsyncFileDialog::new().pick_folder().await
            else {
                return;
            };

            let path = h.path().to_path_buf();
            let Some(w) = weak.upgrade() else { return };

            ui_state().set_save_to(&path);
            w.global::<IncomingTransferOfferBridge>()
                .set_target_path(
                    path.to_string_lossy().to_string().into(),
                );
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
    w: &Weak<App>,
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

pub fn show_connection_offer(w: &Weak<App>, offer: ConnectionOffer) {
    let _ = w.upgrade_in_event_loop(move |w| {
        let b = w.global::<ConnectionOfferBridge>();
        b.set_open(true);
        b.set_remote_device_name(offer.username.as_string().into());
    });
}
