use crate::NectanSender;
use crate::devices::update_devices;
use crate::offers::{show_connection_offer, show_transfer_offer};
use crate::transfers::update_transfers;
use nectan_core::{messages::AppEvent, protocol::NectanState};
use notify_rust::{Hint, Notification, Timeout};
use slint::Weak;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use tracing::{error, info};

pub async fn event_listener(
    w: Weak<NectanSender>,
    mut rx: Receiver<AppEvent>,
    state: Arc<NectanState>,
) {
    while let Some(msg) = rx.recv().await {
        info!("AppEvent. {msg:?}");
        match msg {
            AppEvent::Autoaccepted { from } => {
                notify(
                    "Accepted Transfer",
                    Some(format!("Accepted transfer from {from}")),
                );
            }
            AppEvent::TransfersUpdated => {
                update_transfers(&w);
            }
            AppEvent::IncomingTransferOffer { offer } => {
                notify(
                    "Incoming transfer",
                    Some(format!(
                        "{} wants to send you something.",
                        offer.sender_name
                    )),
                );
                // show_transfer_offer(&w, offer);
            }
            AppEvent::ConnectionOffer { offer } => {
                notify(
                    "Incoming connection",
                    Some(format!(
                        "{} wants to connect with you.",
                        offer.username
                    )),
                );
                show_connection_offer(&w, offer);
            }
            AppEvent::FoundNearby => {
                notify("Discovered nearby device", None);
                update_devices(&w, &state);
            }
            AppEvent::Connected { .. } => {
                update_devices(&w, &state);
            }
            AppEvent::DeviceWentOffline { .. } => {
                update_devices(&w, &state);
            }
        }
    }
}

pub fn notify(summary: impl Into<String>, body: Option<String>) {
    let summary = summary.into();
    tokio::spawn(async move {
        let mut n = Notification::new();
        n.summary(&summary)
            .icon("nectan")
            .hint(Hint::DesktopEntry("nectan".into()))
            .appname("Nectan")
            .timeout(Timeout::Milliseconds(3000));
        if let Some(b) = &body {
            n.body(b);
        }
        match n.show_async().await {
            Ok(handle) => {
                tokio::time::sleep(std::time::Duration::from_secs(
                    10,
                ))
                .await;
                drop(handle);
            }
            Err(e) => error!("notification failed: {e}"),
        }
    });
}
