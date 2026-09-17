use anyhow::{Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use futures::StreamExt;
use getrandom::{SysRng, rand_core::UnwrapErr};
use iroh::{
    Endpoint, EndpointAddr,
    endpoint::{Connection, RecvStream, SendStream, presets},
    endpoint_info::UserData,
    protocol::{AcceptError, ProtocolHandler, Router},
};
use iroh_mdns_address_lookup::{DiscoveryEvent, MdnsAddressLookup};
use n0_error::e;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, atomic::Ordering::Relaxed},
    time::Duration,
};
use tokio::sync::oneshot;
use tracing::{Instrument, debug_span, error, warn};
use uuid::Uuid;

use crate::{
    devices::{Device, DeviceId, DeviceStatus::Online, DevicesPool, UserInfo, Username},
    messages::{AppEvent, NetMessage, UiResponse},
    path_tree::{CompressedPathTree, PathTree},
    stream::StreamPair,
    transfers::{PendingTransfers, recieve_item},
    walker::Walker,
};

pub const ONLINE_TIMEOUT: Duration = Duration::from_secs(5);
pub const PROTOCOL_VERSION: u64 = 0;
pub const ALPN: &[u8] = b"nectan/0";

#[derive(Clone, Debug)]
pub struct NectanProtocol {
    version: u64,
    endpoint: Endpoint,
    state: Arc<NectanState>,
}

impl NectanProtocol {
    pub fn new(endpoint: Endpoint, state: Arc<NectanState>) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            endpoint,
            state,
        }
    }
    pub fn endpoint(&self) -> Endpoint {
        self.endpoint.clone()
    }
    pub fn state(&self) -> Arc<NectanState> {
        Arc::clone(&self.state)
    }
}

impl ProtocolHandler for NectanProtocol {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let state = self.state();
        let devices = &state.devices;
        let endpoint = self.endpoint();

        let devices = &state.devices;
        let singing_key = &state.signing_key;
        let my_device_id = state.device_id;

        let remote_device_endpoint_id = connection.remote_id();
        let remote_addr = EndpointAddr::new(remote_device_endpoint_id);

        let (mut stream_tx, mut stream_rx) = connection.accept_bi().await?;

        let Ok(msg) = NetMessage::read_async(&mut stream_rx).await else {
            return Err(e!(AcceptError::NotAllowed));
        };

        let NetMessage::Hello {
            id: remote_device_id,
            username: remote_username,
            signature: remote_signature,
            ..
        } = msg
        else {
            tracing::error!("Accepting connection. Bad message.");
            return Err(e!(AcceptError::NotAllowed));
        };

        if remote_device_id
            .verify(endpoint.id().as_bytes(), &remote_signature)
            .is_err()
        {
            tracing::error!("Bad signature");
            return Err(e!(AcceptError::NotAllowed));
        }

        let signature = singing_key.sign(remote_device_endpoint_id.as_bytes());
        let stored = devices.get(&remote_device_id);

        // If device is unknown, user needs to explicitly accept.
        if stored.is_none() {
            let (respond, rx) = oneshot::channel();

            state
                .emit(AppEvent::NewConnectionRequest {
                    request_id: Uuid::new_v4(),
                    username: remote_username.clone(),
                    remote_device_id,
                    nearby: devices.is_nearby(&remote_device_endpoint_id),
                    respond,
                })
                .await;

            let Ok(r) = rx.await else {
                return Err(e!(AcceptError::NotAllowed));
            };

            let _ = NetMessage::from(r).write(&mut stream_tx).await;

            match r {
                // Continue
                UiResponse::Accept => {}
                UiResponse::Reject { reason } => {
                    error!("new connection rejected. Reason [{:?}]", reason);
                    return Err(e!(AcceptError::NotAllowed));
                }
            }
        }

        let username = state.userinfo.username();
        let _ = NetMessage::Hello {
            id: my_device_id,
            signature,
            username: username.clone(),
        }
        .write(&mut stream_tx)
        .await;

        let d = Device {
            username,
            id: remote_device_id,
            endpoint_addr: Some(remote_addr.clone()),
            status: Online,
            deleted: false,
            connection: Some(connection.clone()),
            completed_transfers: stored.as_ref().map(|d| d.completed_transfers).unwrap_or(0),
            total_exchanged_data: stored.as_ref().map(|d| d.total_exchanged_data).unwrap_or(0),
            fav: stored.as_ref().map(|d| d.fav).unwrap_or(false),
        };

        let _ = devices.insert(remote_device_id, d);

        state
            .emit(AppEvent::Connected {
                device_id: remote_device_id,
            })
            .await;

        tokio::task::spawn(handle_connection(state, connection, remote_username));

        Ok(())
    }
    async fn shutdown(&self) {}
}

async fn handle_connection(
    state: Arc<NectanState>,
    conn: Connection,
    sender_name: Username,
    // sender_id: DeviceId,
) {
    let id = conn.stable_id().to_string();
    let span = debug_span!("connection", id);
    let sender_name: Arc<str> = sender_name.as_string().into();

    async move {
        while let Ok(pair) = StreamPair::accept(&conn).await {
            let span = debug_span!("stream", stream_id = %pair.stream_id());
            tokio::spawn(handle_stream(state.clone(), pair, sender_name.clone()).instrument(span));
        }
    }
    .instrument(span)
    .await;
}

async fn handle_stream(
    state: Arc<NectanState>,
    mut stream: StreamPair,
    sender_name: Arc<str>,
) -> Result<()> {
    let msg = stream.read_request().await?;

    match msg {
        NetMessage::TransferOfferMsg { offer } => {
            handle_transfer_offer(state, stream, sender_name, offer).await?;
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
    state: Arc<NectanState>,
    mut stream: StreamPair,
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
        &NetMessage::OfferRejeted {
            reason: Some("Receiver is busy with another offer.".to_string()),
        }
        .write(stream.tx())
        .await;
        return Ok(());
    }
    let offer = offer.unwrap();

    let _ = state
        .app_event_tx
        .send(AppEvent::IncomingTransferOffer { offer });

    if let Some(r) = rx.recv().await {
        // let msg = NetMessage::from(r).write(&mut stream.tx());
        // let _ = write_message(&mut pair, &msg).await;
        // stream.tx().finish();
    }

    Ok(())
}

#[derive(Clone)]
pub struct NectanState {
    pending_transfers: PendingTransfers,
    app_event_tx: tokio::sync::mpsc::Sender<AppEvent>,
    incoming_transfer_offer: Arc<Mutex<Option<TransferOffer>>>,
    devices: DevicesPool,
    pub mdns: MdnsAddressLookup,
    pub router: Router,
    pub signing_key: SigningKey,
    userinfo: UserInfo,
    device_id: DeviceId,
}

impl NectanState {
    pub async fn new(
        userinfo: UserInfo,
        device_id: DeviceId,
        signing_key: SigningKey,
        router: Router,
        app_event_tx: tokio::sync::mpsc::Sender<AppEvent>,
    ) -> Self {
        let user_data: UserData = STANDARD.encode(device_id.to_bytes()).parse().unwrap();
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
            app_event_tx,
            incoming_transfer_offer: Arc::new(Mutex::new(None)),
            devices: DevicesPool::new(None).expect("Failed to create devices pool."),
            mdns,
            router,
            signing_key,
            userinfo,
            device_id,
        }
    }
    pub fn sender(&self) -> tokio::sync::mpsc::Sender<AppEvent> {
        self.app_event_tx.clone()
    }
    pub fn respond_to_offer(&self, r: UiResponse) {
        if let Some(offer) = self.incoming_transfer_offer.lock().unwrap().take() {
            let _ = offer.respond.send(r);
        }
    }
    async fn emit(&self, event: AppEvent) {
        let _ = self.app_event_tx.send(event).await;
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

pub async fn connect(state: &NectanState, target: EndpointAddr) -> Result<Device> {
    let connection = state
        .router
        // .as_ref()
        // .unwrap()
        .endpoint()
        .connect(target, ALPN)
        .await?;
    // .map_err(|e| {
    //     tracing::error!("Connecting to the device failed. {e:#?}",);
    //     e!(NectanProtocolError::ConnectionFailed)
    // })?;
    bail!("Not implemented");

    let (mut tx, mut rx) = connection.open_bi().await?;

    let signature = state.signing_key.sign(target.id.as_bytes());
    let username = state.userinfo.username();

    NetMessage::Hello {
        id: state.device_id,
        username,
        signature,
    }
    .write(&mut tx)
    .await?;

    match NetMessage::read_async(&mut rx).await? {
        NetMessage::Hello {
            id,
            username,
            signature,
            ..
        } => {
            id.verify(state.router.endpoint().id().as_bytes(), &signature)?;

            let stored_remote_device: Option<Device> = state.devices.get(&id);

            let remote_device = Device {
                username: username.clone(),
                id,
                endpoint_addr: Some(target.clone()),
                connection: Some(connection),
                deleted: false,
                status: Online,
                completed_transfers: stored_remote_device
                    .as_ref()
                    .map(|d| d.completed_transfers)
                    .unwrap_or(0),
                total_exchanged_data: stored_remote_device
                    .as_ref()
                    .map(|d| d.total_exchanged_data)
                    .unwrap_or(0),
                fav: stored_remote_device
                    .as_ref()
                    .map(|d| d.fav)
                    .unwrap_or(false),
            };

            // let devices_pool: DevicesPool = state.devices_pool.clone();
            // let _ = devices_pool.insert(id, remote_device.clone());
            // let msg = Message::new(MessagePayload::DeviceWentOnline {
            //     device_id: id,
            //     on_local_network: on_local,
            // });
            // let _ = state.event_broadcast.send(msg);
            // let transfers_pool = state.transfers_pool.clone();
            //
            // let state = state.clone();
            // tokio::spawn(async move {
            //     stream_acceptor(id, connection, state.into(), username).await;
            // });

            Ok(remote_device)
        }
        NetMessage::RejectConnection => {
            warn!("Remote device rejected connection");
            bail!("Remote device rejected connection.")
        }
        _ => {
            error!("Bad remote device response");
            bail!("Failed to connect. Remote device responded with unexpected message.")
        }
    }
}

fn validate_path_component(component: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !component.contains('/'),
        "path components must not contain the only correct path separator, /"
    );
    Ok(())
}

pub fn gen_device_id() -> (DeviceId, SigningKey) {
    let mut csprng = UnwrapErr(SysRng);
    let key = SigningKey::generate(&mut csprng);
    (key.verifying_key(), key)
}

// TEMPORARY
pub async fn make_router(state: Arc<NectanState>) -> Router {
    let builder = Endpoint::builder(presets::N0);
    let endpoint = builder.bind().await.unwrap();
    let prot = NectanProtocol::new(endpoint.clone(), state);
    Router::builder(endpoint).accept(ALPN, prot).spawn()
}
