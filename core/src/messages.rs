use anyhow::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::Signature;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    devices::{DeviceId, Username},
    path_tree::CompressedPathTree,
    protocol::{TransferOffer, TransferOfferInner},
};

/// Set of messages sent over the network
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetMessage {
    Hello {
        id: DeviceId,
        username: Username,
        signature: Signature,
    },
    TransferOfferMsg {
        offer: TransferOfferInner<CompressedPathTree>,
    },
    TransferStream {
        transfer_id: Uuid,
    },
    Accepted,
    Rejected {
        reason: Option<String>,
    },
}
impl NetMessage {
    pub async fn read_async<R: crate::stream::RecvStream>(rx: &mut R) -> Result<Self> {
        // Read length prefix
        let mut len_buf = [0u8; 4];
        rx.recv_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;

        // Read payload
        let mut buf = vec![0u8; len];
        rx.recv_exact(&mut buf).await?;

        Ok(postcard::from_bytes(&buf)?)
    }

    pub async fn write<T: crate::stream::SendStream + tokio::io::AsyncWriteExt + Unpin>(
        &self,
        tx: &mut T,
    ) -> Result<()> {
        let raw_msg = postcard::to_allocvec(self).unwrap();
        let len = raw_msg.len() as u32;

        // Write length prefix
        tx.write_all(&len.to_be_bytes()).await?;
        // Write payload
        tx.write_all(&raw_msg).await?;

        Ok(())
    }
}

#[derive(Clone)]
pub struct ConnectionOffer {
    pub username: Username,
    pub remote_device_id: DeviceId,
    pub nearby: bool,
    pub respond: tokio::sync::mpsc::Sender<UiResponse>,
}

pub enum AppEvent {
    FoundNearby,
    Connected { device_id: DeviceId },
    ConnectionOffer { offer: ConnectionOffer },
    IncomingTransferOffer { offer: TransferOffer },
    TransferOfferDelivered,
    DeviceWentOffline { device_id: DeviceId },
}

impl std::fmt::Debug for AppEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeviceWentOffline { device_id } => f
                .debug_struct("DeviceWentOffline")
                .field("device_id", device_id)
                .finish(),
            Self::TransferOfferDelivered => f.debug_struct("TransferOfferDelivered").finish(),
            Self::FoundNearby => f.debug_struct("FoundNearby").finish(),
            Self::Connected { device_id } => f
                .debug_struct("Connected")
                .field("device_id", &STANDARD.encode(&device_id))
                .finish(),
            Self::ConnectionOffer { offer } => {
                f.debug_struct("NewConnectionRequest")
                    .field("username", &offer.username.to_string())
                    .field(
                        "remote_device_id",
                        &STANDARD.encode(&offer.remote_device_id),
                    )
                    .field("nearby", &offer.nearby)
                    .field("respond", &"Sender { .. }") // Optional placeholder
                    .finish()
            }
            Self::IncomingTransferOffer { offer } => f
                .debug_struct("IncomingTransferOffer")
                .field("offer", offer)
                .finish(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum UiResponse {
    Accept,
    Reject { reason: Option<String> },
}

impl From<UiResponse> for NetMessage {
    fn from(resp: UiResponse) -> Self {
        match resp {
            UiResponse::Accept => NetMessage::Accepted,
            UiResponse::Reject { reason } => NetMessage::Rejected { reason },
        }
    }
}
