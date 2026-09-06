use anyhow::Result;
use core::str;
use iroh::{
    Endpoint,
    endpoint::{Connection, RecvStream, SendStream, presets},
    protocol::{AcceptError, ProtocolHandler},
};
use n0_error::e;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::{path_tree::CompressedPathTree, walker::Walker};

pub const PROTOCOL_VERSION: u64 = 0;
pub const ALPN: &[u8] = b"nectan/0";

#[derive(Clone, Debug)]
pub struct NectanProtocol {
    version: u64,
    endpoint: Endpoint,
}

impl NectanProtocol {
    pub fn new(endpoint: Endpoint) -> Self {
        Self {
            endpoint,
            version: PROTOCOL_VERSION,
        }
    }
    pub fn endpoint(&self) -> Endpoint {
        self.endpoint.clone()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    Hello { username: String },
    TransferOffer { id: Uuid, tree: CompressedPathTree },
}

pub async fn write_message(tx: &mut SendStream, msg: &Message) -> Result<()> {
    let raw_msg = postcard::to_allocvec(msg).unwrap();
    let len = raw_msg.len() as u32;

    // Write length prefix
    tx.write_all(&len.to_be_bytes()).await?;
    // Write payload
    tx.write_all(&raw_msg).await?;

    Ok(())
}

pub async fn read_message(rx: &mut RecvStream) -> Result<Message> {
    // Read length prefix
    let mut len_buf = [0u8; 4];
    rx.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;

    // Read payload
    let mut buf = vec![0u8; len];
    rx.read_exact(&mut buf).await?;

    Ok(postcard::from_bytes(&buf)?)
}

impl ProtocolHandler for NectanProtocol {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let (tx, mut rx) = connection.accept_bi().await?;

        let Ok(msg) = read_message(&mut rx).await else {
            return Err(e!(AcceptError::NotAllowed));
        };

        let Message::Hello { username } = msg else {
            return Err(e!(AcceptError::NotAllowed));
        };

        let Ok(msg) = read_message(&mut rx).await else {
            return Err(e!(AcceptError::NotAllowed));
        };
        let Message::TransferOffer { id, tree } = msg else {
            return Err(e!(AcceptError::NotAllowed));
        };
        let paths_list = tree.decompress().unwrap().to_vec();
        println!("Got transfer offer: {paths_list:#?}");

        Ok(())
    }
    async fn shutdown(&self) {}
}

struct Device {
    username: String,
}

pub fn build_offer() -> CompressedPathTree {
    let paths = vec![PathBuf::from("/home/karol/Documents")];
    let walker = Walker::new(paths, true, true);
    walker.walk().join();
    walker.tree.lock().unwrap().take().unwrap()
}

// pub fn connect() {}
//
// pub fn stream_mux() {}
//
// pub fn handle_stream() {}

pub async fn setup() -> Result<()> {
    let builder = Endpoint::builder(presets::N0);
    let endpoint = builder.bind().await?;

    Ok(())
}
