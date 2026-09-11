use anyhow::Result;
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
    OfferAccepted,
    OfferRejeted {
        reason: Option<String>,
    },
    TransferStream {
        transfer_id: Uuid,
    },
    RejectConnection,
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
pub enum AppEvent {
    // TODO
    FoundNearby,
    // TODO
    NewConnectionRequest,
    IncomingTransferOffer { offer: TransferOffer },
}

pub enum UiResponse {
    Accept,
    Reject { reason: Option<String> },
}

impl From<UiResponse> for NetMessage {
    fn from(resp: UiResponse) -> Self {
        match resp {
            UiResponse::Accept => NetMessage::OfferAccepted,
            UiResponse::Reject { reason } => NetMessage::OfferRejeted { reason },
        }
    }
}
