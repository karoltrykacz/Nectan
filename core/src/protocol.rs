use anyhow::anyhow;
use anyhow::{Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use futures::StreamExt;
pub use iroh::EndpointId;
use iroh::{
    Endpoint, EndpointAddr,
    endpoint::{Connection, RecvStream, SendStream, presets},
    endpoint_info::UserData,
    protocol::{AcceptError, ProtocolHandler, Router},
};
use iroh_mdns_address_lookup::{DiscoveryEvent, MdnsAddressLookup};
use n0_error::e;
use rand::rand_core::UnwrapErr;
use rand::rngs::SysRng;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::SeqCst;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, atomic::Ordering::Relaxed},
    time::Duration,
};
use tokio::sync::OnceCell;
use tokio::sync::oneshot;
use tracing::{Instrument, debug, debug_span, error, info, trace, warn};
use uuid::Uuid;

use crate::{
    devices::{Device, DeviceId, DeviceStatus::Online, Devices, UserInfo, Username},
    messages::{AppEvent, NetMessage, UiResponse},
    path_tree::{CompressedPathTree, PathTree},
    stream::StreamPair,
    transfers::{PendingTransfers, recieve_item},
    walker::Walker,
};

pub const ONLINE_TIMEOUT: Duration = Duration::from_secs(5);
pub const PROTOCOL_VERSION: u64 = 0;
pub const DISCOVERY_URL: &str = "https://nectan.ngrok.dev";
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
        let endpoint = self.endpoint();

        let devices = &state.devices;
        let singing_key = &state.signing_key;
        let my_device_id = state.device_id;

        let remote_ep_id = connection.remote_id();
        let remote_addr = EndpointAddr::new(remote_ep_id);

        trace!("Accepting new connection. {}", remote_ep_id.to_string());

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
            error!("Accepting connection. Bad message.");
            return Err(e!(AcceptError::NotAllowed));
        };

        if remote_device_id
            .verify(endpoint.id().as_bytes(), &remote_signature)
            .is_err()
        {
            error!("Bad signature");
            return Err(e!(AcceptError::NotAllowed));
        }

        let signature = singing_key.sign(remote_ep_id.as_bytes());
        let stored = devices.get(&remote_device_id);

        // If device is unknown, user needs to explicitly accept.
        if stored.is_none() {
            let (respond, rx) = oneshot::channel();
            state
                .emit(AppEvent::NewConnectionRequest {
                    request_id: Uuid::new_v4(),
                    username: remote_username.clone(),
                    remote_device_id,
                    nearby: devices.is_nearby(&remote_ep_id),
                    respond,
                })
                .await;

            let Ok(r) = rx.await else {
                return Err(e!(AcceptError::NotAllowed));
            };
            match r {
                UiResponse::Accept => {}
                UiResponse::Reject { reason } => {
                    error!("new connection rejected. Reason [{:?}]", reason);
                    let _ = NetMessage::Rejected { reason: None }
                        .write(&mut stream_tx)
                        .await;
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

        info!("Accepted new connection from {}", remote_username);
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
            error!("Forbidden message in stream handler. {msg:#?}");
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

#[derive(Clone, Debug)]
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
        &NetMessage::Rejected {
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
    devices: Devices,
    mdns: MdnsAddressLookup,
    router: OnceCell<Router>,
    signing_key: SigningKey,
    userinfo: UserInfo,
    device_id: DeviceId,
    http_client: Client,
    pub seq_num: SeqNumber,
}

impl NectanState {
    pub async fn build(
        userinfo: UserInfo,
        device_id: DeviceId,
        signing_key: SigningKey,
        devices: Devices,
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
            devices,
            mdns,
            router: OnceCell::new(),
            signing_key,
            userinfo,
            device_id,
            http_client: Client::new(),
            seq_num: SeqNumber::new(),
        }
    }
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }
    pub fn attach_router(&self, router: &Router) {
        self.router
            .set(router.clone())
            .expect("Router already attached");
    }
    pub fn router(&self) -> Router {
        self.router.get().unwrap().clone()
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
    pub fn http(&self) -> &Client {
        &self.http_client
    }
    pub fn sign(&self, msg: &[u8]) -> Signature {
        self.signing_key.sign(msg)
    }
    pub fn endpoint_id(&self) -> EndpointId {
        self.router().endpoint().id()
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
    //         trace!("EP1 changed {addr:#?}");
    //     }
    // });
}
//
pub fn start_mdns_discovery(state: &NectanState) {
    let state = state.clone();
    tokio::spawn(async move {
        trace!("Started local discovery");

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
                        error!("Discovery error. Failed to decode user data.");
                        continue;
                    };

                    let _ = state.app_event_tx.send(AppEvent::FoundNearby);
                    info!("Discovery event. Found new device.");
                    let addr = endpoint_info.to_endpoint_addr();
                    info!("Address {addr:#?}");
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

pub async fn connect(state: &NectanState, target: EndpointId) -> Result<Device> {
    let connection = state
        .router()
        .endpoint()
        .connect(target, ALPN)
        .await
        .map_err(|e| {
            error!("Connecting to the device failed. {e:#?}",);
            anyhow!("Connection failed. {}", e)
        })?;

    let (mut stream_tx, mut stream_rx) = connection.open_bi().await?;

    let signature = state.signing_key.sign(target.as_bytes());
    let username = state.userinfo.username();

    NetMessage::Hello {
        id: state.device_id,
        username: username.clone(),
        signature,
    }
    .write(&mut stream_tx)
    .await?;

    let Ok(res) = NetMessage::read_async(&mut stream_rx).await else {
        bail!("Failed to read response.")
    };

    match res {
        NetMessage::Hello {
            id: remote_device_id,
            username: remote_username,
            signature: remote_signature,
            ..
        } => {
            remote_device_id
                .verify(state.router().endpoint().id().as_bytes(), &remote_signature)
                .map_err(|_| anyhow!("Failed to verify signature."))?;

            let stored_remote_device: Option<Device> = state.devices.get(&remote_device_id);
            // let on_local = state.devices.is_nearby(&target.id);

            let remote_device = Device {
                username: remote_username.clone(),
                id: remote_device_id,
                connection: Some(connection.clone()),
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

            state
                .emit(AppEvent::Connected {
                    device_id: remote_device_id,
                })
                .await;

            let state = state.clone();
            tokio::spawn(handle_connection(state.into(), connection, username));
            Ok(remote_device)
        }
        NetMessage::Rejected { reason } => {
            warn!("Remote device rejected connection. [{:?}]", reason);
            bail!("Remote device rejected connection.")
        }
        r => {
            error!("Bad remote device response. {:#?}", r);
            bail!("Remote device sent bad message.")
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

#[derive(Clone, Debug, Default)]
pub struct SeqNumber {
    inner: Arc<OnceLock<AtomicU64>>,
}

impl SeqNumber {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(OnceLock::new()),
        }
    }

    pub fn set(&self, val: u64) {
        self.inner.set(AtomicU64::new(val)).ok();
    }

    pub fn get_and_inc(&self) -> Option<u64> {
        self.inner.get().map(|s| s.fetch_add(1, SeqCst))
    }
}

#[derive(Deserialize, Serialize)]
pub struct EndpointAnnouncePayload {
    pub device_id: DeviceId,
    pub endpoint_id: EndpointId,
    pub signature: Signature,
}

#[derive(Deserialize, Serialize)]
pub struct EndpointAnncounceResponse {
    pub seq_num: u64,
}

pub async fn announce_endpoint(state: &NectanState) {
    let endpoint_id = state.endpoint_id();
    let device_id = state.device_id();
    let signature: Signature = state.sign(endpoint_id.as_bytes());
    let payload = EndpointAnnouncePayload {
        device_id,
        endpoint_id,
        signature,
    };
    let payload = postcard::to_allocvec(&payload).unwrap();

    let seq = loop {
        match state
            .http()
            .post(format!("{}/announce_endpoint", DISCOVERY_URL))
            .body(payload.clone())
            .send()
            .await
        {
            Ok(response) => match response.status() {
                StatusCode::OK => {
                    break 10;
                    // match response.json::<EndpointAnncounceResponse>().await {
                    //     Ok(body) => {
                    //         break body.seq_num;
                    //     }
                    //     Err(e) => error!(
                    //         "Annouce endpoint. Failed to parse response: {:?}, retrying...",
                    //         e
                    //     ),
                    // }
                    // return;
                }
                StatusCode::UNAUTHORIZED => {
                    warn!("Announce endpoint. UNAUTHORIZED");
                    return;
                }
                StatusCode::NOT_FOUND => {
                    error!("Announce endpoint. NOT_FOUND");
                    return;
                }
                status => {
                    error!("Announce endpoint unexpected status: {status}, retrying...")
                }
            },
            Err(e) => {
                error!("Announce enpoint network error {e}.");
            }
        }
        warn!("Announce endpoint retry.");
        tokio::time::sleep(Duration::from_millis(4000)).await;
    };

    trace!("Endpoint announced. Recovered sequence number {seq}");
    state.seq_num.set(seq + 1);
}
