use anyhow::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::Signature;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    devices::{DeviceId, Username},
    path_tree::CompressedPathTree,
    protocol::{TransferOffer, TransferOfferRequest},
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
        offer: TransferOffer<CompressedPathTree>,
    },
    Accepted,
    Rejected {
        reason: Option<String>,
    },
    /// Forwards the stream to Transfers manager
    TransferStream {
        transfer_id: Uuid,
    },
    // Forwards the stream to Specified Container (if has permission)
    ContainerStream {
        id: Uuid,
    },
}

pub trait StreamableMessage: Sized {
    async fn read_async<R: crate::stream::RecvStream>(rx: &mut R) -> Result<Self>;
    async fn write<T: crate::stream::SendStream>(&self, tx: &mut T) -> Result<()>;
}

impl<M: Serialize + serde::de::DeserializeOwned> StreamableMessage for M {
    async fn read_async<R: crate::stream::RecvStream>(rx: &mut R) -> Result<Self> {
        let mut len_buf = [0u8; 4];
        rx.recv_exact(&mut len_buf).await?;

        let len = u32::from_be_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];

        rx.recv_exact(&mut buf).await?;
        Ok(postcard::from_bytes(&buf)?)
    }

    async fn write<T: crate::stream::SendStream>(&self, tx: &mut T) -> Result<()> {
        let raw_msg = postcard::to_allocvec(self)?;
        let len = raw_msg.len() as u32;
        tx.send(&len.to_be_bytes()).await?;
        tx.send(&raw_msg).await?;
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
    IncomingTransferOffer { offer: TransferOfferRequest },
    TransferOfferDelivered,
    DeviceWentOffline { device_id: DeviceId },
    TransferUpdated,
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
