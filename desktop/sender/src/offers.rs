use crate::ConnectionOfferBridge;
use crate::IncomingTransferOffer;
use crate::IncomingTransferOfferBridge;
use crate::NectanSender;
use crate::OfferEntry;
use nectan_core::messages::ConnectionOffer;
use nectan_core::{
    format::DecimalBytes, protocol::TransferOfferRequest,
};
use slint::ComponentHandle;
use slint::ModelRc;
use slint::ToSharedString;
use slint::VecModel;
use slint::Weak;

pub fn show_transfer_offer(
    w: &Weak<NectanSender>,
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

pub fn show_connection_offer(
    w: &Weak<NectanSender>,
    offer: ConnectionOffer,
) {
    let _ = w.upgrade_in_event_loop(move |w| {
        let b = w.global::<ConnectionOfferBridge>();
        b.set_open(true);
        b.set_remote_device_name(offer.username.as_string().into());
    });
}
