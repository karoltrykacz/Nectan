use anyhow::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use core::str;
use ed25519_dalek::VerifyingKey;
use futures::StreamExt;
use iroh::{
    Endpoint,
    endpoint::{Connection, RecvStream, SendStream, presets},
    endpoint_info::UserData,
    protocol::{AcceptError, ProtocolHandler},
};
use iroh_mdns_address_lookup::{DiscoveryEvent, MdnsAddressLookup};
use n0_error::e;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, atomic::Ordering::Relaxed},
    time::Duration,
};
use uuid::Uuid;

use crate::{
    devices::DevicesPool,
    messages::{AppEvent, NetMessage, UiResponse, read_message, write_message},
    path_tree::{CompressedPathTree, PathTree},
    transfers::{PendingTransfers, recieve_item},
    walker::Walker,
};

pub const ONLINE_TIMEOUT: Duration = Duration::from_secs(5);
pub const PROTOCOL_VERSION: u64 = 0;
pub const ALPN: &[u8] = b"nectan/0";

pub type DeviceId = VerifyingKey;

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

impl ProtocolHandler for NectanProtocol {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let (mut tx, mut rx) = connection.accept_bi().await?;

        let Ok(msg) = read_message(&mut rx).await else {
            return Err(e!(AcceptError::NotAllowed));
        };

        let NetMessage::Hello { username } = msg else {
            println!("Msg not allowed.");
            return Err(e!(AcceptError::NotAllowed));
        };

        let Ok(msg) = read_message(&mut rx).await else {
            println!("Msg not allowed.");
            return Err(e!(AcceptError::NotAllowed));
        };

        let NetMessage::TransferOfferMsg { offer } = msg else {
            return Err(e!(AcceptError::NotAllowed));
        };

        println!("Accepting");
        write_message(&mut tx, &NetMessage::OfferAccepted)
            .await
            .unwrap();
        // let _ = tx.finish();

        tracing::debug!("Wrote accept steam msg.");
        let state = self.state.clone();
        let sender_name = "ghuj".to_string();
        // let sender_id = SigningKey::generate()
        tokio::spawn(async move {
            handle_connection(state, connection, sender_name).await;
        });

        Ok(())
    }
    async fn shutdown(&self) {}
}

async fn handle_connection(
    state: NectanState,
    conn: Connection,
    sender_name: String,
    // sender_id: DeviceId,
) {
    let sender_name: Arc<str> = sender_name.into();
    loop {
        match conn.accept_bi().await {
            Ok((tx, rx)) => {
                let state = state.clone();
                let name = sender_name.clone();

                tokio::spawn(async move {
                    let _ = handle_stream(state, tx, rx, name).await;
                });
            }
            Err(e) => {
                tracing::error!("Stream failed: {e:#?}");
                break;
            }
        }
    }
}

async fn handle_stream(
    state: NectanState,
    tx: SendStream,
    mut rx: RecvStream,
    sender_name: Arc<str>,
) -> Result<()> {
    let msg = read_message(&mut rx).await?;

    match msg {
        NetMessage::TransferOfferMsg { offer } => {
            handle_transfer_offer(state, tx, sender_name, offer).await?;
        }
        _ => {
            tracing::error!("Forbidden message in stream handler. {msg:#?}");
        }
    }

    Ok(())
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TransferOfferInner<T> {
    pub transfer_name: String,
    pub transfer_id: Uuid,
    pub entries_num: u64,
    pub total_size: u64,
    pub tree: Arc<T>,
}

impl TransferOfferInner<CompressedPathTree> {
    pub fn decompress(self) -> anyhow::Result<TransferOfferInner<PathTree>> {
        let tree = Arc::new(self.tree.as_ref().decompress()?);

        Ok(TransferOfferInner {
            transfer_name: self.transfer_name,
            transfer_id: self.transfer_id,
            entries_num: self.entries_num,
            total_size: self.total_size,
            tree,
        })
    }
}

#[derive(Clone)]
pub struct TransferOffer {
    // sender_id: DeviceId,
    pub sender_name: String,
    pub inner: TransferOfferInner<PathTree>,
    pub respond: tokio::sync::mpsc::Sender<UiResponse>,
}

async fn handle_transfer_offer(
    state: NectanState,
    mut tx: SendStream,
    sender_name: Arc<str>,
    offer: TransferOfferInner<CompressedPathTree>,
) -> Result<()> {
    let (respond, mut rx) = tokio::sync::mpsc::channel(1);

    // Reject the offer if we already have some offer
    let offer = {
        let mut lock = state.incoming_transfer_offer.lock().unwrap();
        if lock.is_some() {
            None
        } else {
            let inner = offer.decompress()?;
            let offer = TransferOffer {
                sender_name: sender_name.to_string(),
                inner,
                respond,
            };
            *lock = Some(offer.clone());
            Some(offer)
        }
    };

    if offer.is_none() {
        let _ = write_message(
            &mut tx,
            &NetMessage::OfferRejeted {
                reason: Some("Receiver is busy with another offer.".to_string()),
            },
        )
        .await;
        let _ = tx.finish();
        return Ok(());
    }
    let offer = offer.unwrap();

    let _ = state
        .app_event_tx
        .send(AppEvent::IncomingTransferOffer { offer });

    if let Some(r) = rx.recv().await {
        let msg = NetMessage::from(r);
        let _ = write_message(&mut tx, &msg).await;
        let _ = tx.finish();
    }

    Ok(())
}

#[derive(Clone)]
pub struct NectanState {
    pending_transfers: PendingTransfers,
    app_event_tx: tokio::sync::broadcast::Sender<AppEvent>,
    incoming_transfer_offer: Arc<Mutex<Option<TransferOffer>>>,
    devices: DevicesPool,
    pub mdns: MdnsAddressLookup,
}

impl NectanState {
    pub async fn new(client_id: DeviceId) -> Self {
        let user_data: UserData = STANDARD.encode(client_id.to_bytes()).parse().unwrap();
        let builder = Endpoint::builder(presets::N0).user_data_for_address_lookup(user_data);
        let endpoint = builder.bind().await.expect("Failed to bind endpoint");

        let mdns = MdnsAddressLookup::builder()
            .service_name("nectan_user")
            .advertise(true)
            .build(endpoint.id())
            .unwrap();

        endpoint.address_lookup().unwrap().add(mdns.clone());

        NectanState {
            pending_transfers: PendingTransfers::new(),
            app_event_tx: tokio::sync::broadcast::Sender::new(16),
            incoming_transfer_offer: Arc::new(Mutex::new(None)),
            devices: DevicesPool::new(None).expect("Failed to create devices pool."),
            mdns,
        }
    }

    pub fn subscribe_to_events(&self) -> tokio::sync::broadcast::Receiver<AppEvent> {
        self.app_event_tx.subscribe()
    }

    pub fn sender(&self) -> tokio::sync::broadcast::Sender<AppEvent> {
        self.app_event_tx.clone()
    }

    // pub fn clear_offer(&self) {
    //     *self.incoming_transfer_offer.lock().unwrap() = None;
    // }

    pub fn respond_to_offer(&self, r: UiResponse) {
        if let Some(offer) = self.incoming_transfer_offer.lock().unwrap().take() {
            let _ = offer.respond.send(r);
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

    println!("Receiving item");
    let result = recieve_item(tx, rx).await;
    println!("Receiver item result {result:#?}");
}

pub fn build_offer() -> PathTree {
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
//
pub fn start_mdns_discovery(state: &NectanState) {
    let state = state.clone();
    tokio::spawn(async move {
        tracing::trace!("Started local discovery");

        let mut events = state.mdns.subscribe().await;

        while let Some(event) = events.next().await {
            match event {
                DiscoveryEvent::Discovered { endpoint_info, .. } => {
                    let Some(device_id) = endpoint_info
                        .user_data()
                        .and_then(|data| STANDARD.decode(data.as_ref()).ok())
                        .and_then(|bytes| bytes.as_slice().try_into().ok())
                        .and_then(|arr: [u8; 32]| VerifyingKey::from_bytes(&arr).ok())
                    else {
                        tracing::error!("Discovery error. Failed to decode user data.");
                        continue;
                    };

                    let _ = state.app_event_tx.send(AppEvent::FoundNearby);
                    tracing::info!("Discovery event. Found new device.");
                    let addr = endpoint_info.to_endpoint_addr();
                    tracing::info!("Address {addr:#?}");
                    // TODO connect here
                }
                DiscoveryEvent::Expired { endpoint_id } => {
                    // TODO
                }
                _ => {}
            }
        }
    });
}

fn validate_path_component(component: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !component.contains('/'),
        "path components must not contain the only correct path separator, /"
    );
    Ok(())
}
