use anyhow::Result;
use core::str;
use futures::channel::oneshot::Receiver;
use iroh::{
    Endpoint,
    endpoint::{Connection, RecvStream, SendStream, presets},
    protocol::{AcceptError, ProtocolHandler},
};
use n0_error::e;
use serde::{Deserialize, Serialize, de};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, RwLock, atomic::Ordering::Relaxed},
    time::Duration,
};
use uuid::Uuid;

use crate::{
    path_tree::CompressedPathTree,
    protocol::{
        Message::{AcceptOffer, Hello, RejectOffer, TransferOffer, TransferStream},
        TransferDirection::Outcoming,
    },
    transfers::recieve_item,
    walker::Walker,
};

pub const ONLINE_TIMEOUT: Duration = Duration::from_secs(5);
pub const PROTOCOL_VERSION: u64 = 0;
pub const ALPN: &[u8] = b"nectan/0";

#[derive(Clone, Debug)]
pub struct NectanProtocol {
    version: u64,
    endpoint: Endpoint,
    state: NectanState,
}

impl NectanProtocol {
    pub fn new(endpoint: Endpoint, state: NectanState) -> Self {
        Self {
            endpoint,
            state,
            version: PROTOCOL_VERSION,
        }
    }
    pub fn endpoint(&self) -> Endpoint {
        self.endpoint.clone()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    Hello {
        username: String,
    },
    TransferOffer {
        transfer_id: Uuid,
        tree: CompressedPathTree,
    },
    AcceptOffer,
    RejectOffer {
        reason: Option<String>,
    },
    TransferStream {
        transfer_id: Uuid,
    },
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
        let (mut tx, mut rx) = connection.accept_bi().await?;

        let Ok(msg) = read_message(&mut rx).await else {
            return Err(e!(AcceptError::NotAllowed));
        };

        let Message::Hello { username } = msg else {
            println!("Msg not allowed.");
            return Err(e!(AcceptError::NotAllowed));
        };

        let Ok(msg) = read_message(&mut rx).await else {
            println!("Msg not allowed.");
            return Err(e!(AcceptError::NotAllowed));
        };

        let Message::TransferOffer {
            transfer_id: id,
            tree,
        } = msg
        else {
            println!("Msg not allowed.");
            return Err(e!(AcceptError::NotAllowed));
        };

        let paths_list = tree.decompress().unwrap().to_vec();
        let preview: Vec<_> = paths_list.iter().take(4).collect();
        println!("Got transfer offer: {preview:#?}");

        println!("Accepting");
        write_message(&mut tx, &Message::AcceptOffer).await.unwrap();
        // let _ = tx.finish();

        tracing::debug!("Wrote accept steam msg.");
        let state = self.state.clone();
        tokio::spawn(async move {
            handle_connection(state, connection).await;
        });

        Ok(())
    }
    async fn shutdown(&self) {}
}

async fn handle_connection(state: NectanState, conn: Connection) {
    loop {
        tracing::info!("Accepting bidi stream.");
        match conn.accept_bi().await {
            Ok((tx, rx)) => {
                tracing::info!("New stream opened;");
                let state = state.clone();
                tokio::spawn(async move {
                    let _ = handle_stream(state, tx, rx).await;
                });
            }
            Err(e) => {
                tracing::error!("Stream failed: {e:#?}");
                break;
            }
        }
    }
}
async fn handle_stream(state: NectanState, tx: SendStream, mut rx: RecvStream) -> Result<()> {
    let msg = read_message(&mut rx).await?;

    tracing::info!("READ NEW STREAM MSG {msg:#?}");

    match msg {
        Hello { .. } => {}
        TransferOffer {
            transfer_id: id,
            tree,
        } => {}
        AcceptOffer => {}
        RejectOffer { reason } => {}
        TransferStream { transfer_id } => {
            println!("New transfer stream.");
            handle_transfer_stream(state, transfer_id, tx, rx).await;
        }
    }

    Ok(())
}
#[derive(Clone, Copy)]
enum TransferDirection {
    Outcoming,
    Incoming,
}

#[derive(Clone)]
pub struct PendingTransfer {
    id: Uuid,
    direction: TransferDirection,
}

#[derive(Clone)]
pub struct PendingTransfers {
    inner: Arc<std::sync::RwLock<HashMap<Uuid, PendingTransfer>>>,
}

impl PendingTransfers {
    pub fn new() -> Self {
        PendingTransfers {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    pub fn get(&self, id: Uuid) -> Option<PendingTransfer> {
        self.inner.read().unwrap().get(&id).cloned()
    }
}

#[derive(Clone)]
pub struct NectanState {
    pending_transfers: PendingTransfers,
}
impl NectanState {
    pub fn new() -> Self {
        NectanState {
            pending_transfers: PendingTransfers::new(),
        }
    }
}

impl std::fmt::Debug for NectanState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NectanState").finish()
    }
}

async fn handle_transfer_stream(
    state: NectanState,
    transfer_id: Uuid,
    tx: SendStream,
    mut rx: RecvStream,
) {
    // let Some(pending) = state.pending_transfers.get(transfer_id) else {
    //     return;
    // };
    //
    println!("Receiving item");
    let result = recieve_item(tx, rx).await;
    println!("Receiver item result {result:#?}");
}

struct Device {
    username: String,
}

pub fn build_offer() -> CompressedPathTree {
    let paths = vec![PathBuf::from("/home/karol/Documents")];
    let walker = Walker::new(paths, true, true);
    walker.walk().join().unwrap();
    let total_size = walker.total_size.load(Relaxed);
    println!("Total size of offer. {total_size}");
    walker.tree.lock().unwrap().take().unwrap()
}

// pub fn connect() {}
// pub fn stream_mux() {}

pub async fn setup() -> Result<()> {
    let builder = Endpoint::builder(presets::N0);
    let endpoint = builder.bind().await?;
    Ok(())
}

pub async fn start_addr_watcher() {
    // let watcher = endpoint.watch_addr();
    // tokio::spawn(async move {
    //     let mut updates = watcher.stream();
    //     while let Some(addr) = updates.next().await {
    //         tracing::trace!("EP1 changed {addr:#?}");
    //     }
    // });
}
