use anyhow::anyhow;
use anyhow::{Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use futures::StreamExt;
pub use iroh::EndpointId;
use iroh::endpoint::Path;
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
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, OnceCell};
use tokio::task::JoinHandle;
use tracing::{Instrument, debug_span, error, info, trace, warn};
use uuid::Uuid;

use crate::messages::{ConnectionOffer, StreamableMessage};
use crate::storage_utils::KvStore;
use crate::transfers::TransferDirection;
use crate::{
    devices::{Device, DeviceId, DeviceStatus::Online, Devices, UserInfo, Username},
    messages::{AppEvent, NetMessage, UiResponse},
    path_tree::{CompressedPathTree, PathTree},
    stream::StreamPair,
    transfers::Transfers,
};

pub const ONLINE_TIMEOUT: Duration = Duration::from_secs(5);
pub const PROTOCOL_VERSION: u64 = 0;
pub const DISCOVERY_URL: &str = "https://discovery.nectan.co";
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
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let state = self.state();
        let endpoint = self.endpoint();

        let devices = &state.devices;
        let singing_key = &state.signing_key;
        let my_device_id = state.device_id;

        let remote_ep_id = conn.remote_id();

        info!("Accepting new connection. {}", remote_ep_id.to_string());

        let (mut stream_tx, mut stream_rx) = conn.accept_bi().await?;

        let Ok(msg) = NetMessage::read_async(&mut stream_rx).await else {
            error!("Accepting connection. Failed to read hello.");
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
        let is_nearby = devices.is_nearby(&remote_ep_id);

        // If device is unknown, user needs to explicitly accept.
        // If devices is nearby connect automatically
        if stored.is_none() && !is_nearby {
            warn!("Unknown remote device.");

            let (respond, mut rx) = tokio::sync::mpsc::channel(1);
            let offer = {
                let mut lock = state.conn_offer.lock().await;
                if lock.is_some() {
                    error!("Rejected new connetion request. (Busy)");
                    let _ = NetMessage::Rejected {
                        reason: Some(String::from("I'm busy.")),
                    }
                    .write(&mut stream_tx)
                    .await;

                    return Err(e!(AcceptError::NotAllowed));
                }

                let offer = ConnectionOffer {
                    username: remote_username.clone(),
                    remote_device_id: remote_device_id,
                    nearby: is_nearby,
                    respond,
                };

                *lock = Some(offer.clone());
                offer
            };

            state.emit(AppEvent::ConnectionOffer { offer }).await;

            let Some(r) = rx.recv().await else {
                error!("Failed to read response to connection request.");
                return Err(e!(AcceptError::NotAllowed));
            };

            match r {
                UiResponse::Accept => {
                    trace!("Connection accepted.");
                }
                UiResponse::Reject { reason } => {
                    error!("Connection rejected. Reason [{:?}]", reason);
                    let _ = NetMessage::Rejected { reason: None }
                        .write(&mut stream_tx)
                        .await;
                    return Err(e!(AcceptError::NotAllowed));
                }
            }
        }

        let my_username = state.userinfo.username();
        let _ = NetMessage::Hello {
            id: my_device_id,
            signature,
            username: my_username.clone(),
        }
        .write(&mut stream_tx)
        .await;

        let device = Device {
            username: remote_username.clone(),
            id: remote_device_id,
            status: Online,
            deleted: false,
            connection: Some(conn.clone()),
            completed_transfers: stored.as_ref().map(|d| d.completed_transfers).unwrap_or(0),
            total_exchanged_data: stored.as_ref().map(|d| d.total_exchanged_data).unwrap_or(0),
            fav: stored.as_ref().map(|d| d.fav).unwrap_or(false),
        };

        let _ = devices.insert(remote_device_id, &device);

        state
            .emit(AppEvent::Connected {
                device_id: remote_device_id,
            })
            .await;

        info!("Accepted connetion {}", remote_username);
        tokio::spawn(handle_connection(state, device, conn));

        Ok(())
    }
    async fn shutdown(&self) {}
}

async fn handle_connection(state: Arc<NectanState>, device: Device, conn: Connection) {
    let conn_id = conn.stable_id();
    info!("Handling connetion {conn_id}");
    let id = conn.stable_id().to_string();
    let span = debug_span!("connection", id);
    let sender_name: Arc<str> = device.username.as_string().into();

    async move {
        while let Ok(pair) = StreamPair::accept(&conn).await {
            let span = debug_span!("stream", stream_id = %pair.stream_id());
            tokio::spawn(
                handle_stream(state.clone(), pair, device.id, sender_name.clone()).instrument(span),
            );
        }
    }
    .instrument(span)
    .await;
}

async fn handle_stream(
    state: Arc<NectanState>,
    mut stream: StreamPair,
    sender: DeviceId,
    sender_name: Arc<str>,
) -> Result<()> {
    let msg: NetMessage = stream.read().await?;

    match msg {
        NetMessage::TransferOfferMsg { offer } => {
            handle_transfer_offer(state, stream, sender, sender_name, offer).await?;
        }
        NetMessage::TransferStream { transfer_id } => {
            state
                .transfers
                .forward_incoming_stream(transfer_id, stream)
                .await?;
        }
        _ => {
            error!("Stream handler received forbidden message. {msg:#?}");
            bail!("Stream handler received forbidden message. {msg:#?}")
        }
    }

    Ok(())
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TransferOffer<T> {
    pub transfer_name: String,
    pub transfer_id: Uuid,
    pub entries_num: u32,
    pub total_size: u64,
    pub tree: Arc<T>,
}

impl TransferOffer<CompressedPathTree> {
    pub fn decompress(self) -> anyhow::Result<TransferOffer<PathTree>> {
        let tree = Arc::new(self.tree.as_ref().decompress()?);

        Ok(TransferOffer {
            transfer_name: self.transfer_name,
            transfer_id: self.transfer_id,
            entries_num: self.entries_num,
            total_size: self.total_size,
            tree,
        })
    }
}

impl TransferOffer<PathTree> {
    pub fn compress(self) -> anyhow::Result<TransferOffer<CompressedPathTree>> {
        let tree = Arc::new(self.tree.as_ref().compress());

        Ok(TransferOffer {
            transfer_name: self.transfer_name,
            transfer_id: self.transfer_id,
            entries_num: self.entries_num,
            total_size: self.total_size,
            tree,
        })
    }
}

#[derive(Clone, Debug)]
pub struct TransferOfferRequest {
    pub sender_name: String,
    pub inner: TransferOffer<PathTree>,
    pub respond: tokio::sync::mpsc::Sender<UiResponse>,
}

async fn handle_transfer_offer(
    state: Arc<NectanState>,
    mut stream: StreamPair,
    sender: DeviceId,
    sender_name: Arc<str>,
    offer: TransferOffer<CompressedPathTree>,
) -> Result<()> {
    let (respond, mut rx) = tokio::sync::mpsc::channel(1);

    // Reject the offer if we already have some offer
    let offer = {
        let mut lock = state.transfer_offer.lock().await;
        if lock.is_some() {
            info!("Offer slot occupied.");
            None
        } else {
            info!("Offer slot empty.");
            let inner = offer.decompress()?;
            let offer = TransferOfferRequest {
                sender_name: sender_name.to_string(),
                inner,
                respond,
            };
            *lock = Some(offer.clone());
            Some(offer)
        }
    };

    if offer.is_none() {
        NetMessage::Rejected {
            reason: Some("Receiver is busy with another offer.".to_string()),
        }
        .write(stream.tx())
        .await?;
        return Ok(());
    }
    let offer = offer.unwrap();

    // TODO! (expensive clone)
    state
        .emit(AppEvent::IncomingTransferOffer {
            offer: offer.clone(),
        })
        .await;

    let Some(r) = rx.recv().await else {
        bail!("Failed to receive user response")
    };
    stream.write(&NetMessage::from(r.clone())).await?;
    info!("Wrote response");

    if let UiResponse::Accept = r {
        state
            .transfers
            .new_offer(sender, TransferDirection::Incoming, offer.inner)
            .await;
    }

    Ok(())
}

#[derive(Clone)]
pub struct NectanState {
    pub transfers: Arc<Transfers>,
    app_event_tx: tokio::sync::mpsc::Sender<AppEvent>,
    /// Only one connection offer allowed at a time
    conn_offer: Arc<Mutex<Option<ConnectionOffer>>>,
    /// Only one transfer offer allowed at a time
    transfer_offer: Arc<Mutex<Option<TransferOfferRequest>>>,
    pub devices: Devices,
    mdns: MdnsAddressLookup,
    router: OnceCell<Router>,
    signing_key: SigningKey,
    userinfo: UserInfo,
    device_id: DeviceId,
    http_client: Client,
    pub seq_num: SeqNumber,
    pub store: KvStore,
}

impl NectanState {
    pub async fn build(
        userinfo: UserInfo,
        device_id: DeviceId,
        signing_key: SigningKey,
        devices: Devices,
        app_event_tx: tokio::sync::mpsc::Sender<AppEvent>,
        store: KvStore,
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
            transfers: Arc::new(Transfers::new(devices.clone(), app_event_tx.clone())),
            app_event_tx,
            conn_offer: Arc::new(Mutex::new(None)),
            transfer_offer: Arc::new(Mutex::new(None)),
            devices,
            mdns,
            router: OnceCell::new(),
            signing_key,
            userinfo,
            device_id,
            http_client: Client::new(),
            seq_num: SeqNumber::new(),
            store,
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
    pub async fn respond_transfer_offer(&self, r: UiResponse) {
        if let Some(offer) = self.transfer_offer.lock().await.take() {
            let _ = offer.respond.send(r).await;
        }
    }
    pub async fn respond_connection_offer(&self, r: UiResponse) {
        if let Some(offer) = self.conn_offer.lock().await.take() {
            let _ = offer.respond.send(r).await;
        }
    }
    pub async fn emit(&self, event: AppEvent) {
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

// async fn handle_transfer_stream(
//     state: NectanState,
//     transfer_id: Uuid,
//     tx: SendStream,
//     mut rx: RecvStream,
// ) {
//     // let Some(pending) = state.pending_transfers.get(transfer_id) else {
//     //     return;
//     // };
//
//     println!("Receiving item");
//     let result = recieve_item(tx, rx).await;
//     println!("Receiver item result {result:#?}");
// }

pub fn start_mdns_discovery(state: &NectanState) {
    let state = Arc::new(state.clone());
    tokio::spawn(async move {
        info!("Started local discovery");

        let mut events = state.mdns.subscribe().await;

        while let Some(event) = events.next().await {
            match event {
                DiscoveryEvent::Discovered { endpoint_info, .. } => {
                    let ep_id = endpoint_info.endpoint_id;

                    if ep_id == state.endpoint_id() {
                        continue;
                    }

                    if state.devices.add_nearby(ep_id) {
                        info!("Found device [MDNS].");
                        let _ = state.app_event_tx.send(AppEvent::FoundNearby);
                        tokio::spawn(connect(state.clone(), endpoint_info.endpoint_id));
                    }
                }
                DiscoveryEvent::Expired { endpoint_id } => {
                    state.devices.remove_nearby(endpoint_id);
                }
                _ => {}
            }
        }
    });
}

pub async fn connect(state: Arc<NectanState>, target: EndpointId) -> Result<()> {
    if state.devices.is_alive(target) {
        warn!("Connecting to the device cancelled. [CONNECTED]",);
        bail!("Already connected")
    }

    let conn = state
        .router()
        .endpoint()
        .connect(target, ALPN)
        .await
        .map_err(|e| {
            error!("Connecting to the device failed. [{e}]");
            anyhow!("Connection failed. {}", e)
        })?;

    let (mut stream_tx, mut stream_rx) = conn.open_bi().await?;

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

            let device = Device {
                username: remote_username.clone(),
                id: remote_device_id,
                connection: Some(conn.clone()),
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
            let _ = state.devices.insert(remote_device_id, &device);

            state
                .emit(AppEvent::Connected {
                    device_id: remote_device_id,
                })
                .await;

            info!("Connected [{}]", remote_username);

            tokio::spawn(handle_connection(state, device, conn));
            Ok(())
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

pub fn announce_endpoint(state: &NectanState) {
    let state = state.clone();
    tokio::spawn(async move {
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
                        let bytes = response.bytes().await;
                        match bytes {
                            Ok(body) => {
                                let Ok(res) =
                                    postcard::from_bytes::<EndpointAnncounceResponse>(&body)
                                else {
                                    continue;
                                };
                                break res.seq_num;
                            }
                            Err(e) => error!(
                                "Annouce endpoint. Failed to parse response: {:?}, retrying...",
                                e
                            ),
                        }
                    }
                    StatusCode::NOT_FOUND => {
                        error!("Announce endpoint. NOT_FOUND");
                        // ???
                        // return;
                    }
                    StatusCode::UNAUTHORIZED => {
                        error!("Announce endpoint. UNAUTHORIZED");
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
    });
}

#[derive(Deserialize, Serialize)]
pub struct DeviceCreateRequest {
    pub device_id: DeviceId,
}

pub fn start_registration_loop(state: &NectanState) -> JoinHandle<()> {
    let state = state.clone();
    tokio::spawn(async move {
        let payload = DeviceCreateRequest {
            device_id: state.device_id(),
        };
        let payload = postcard::to_allocvec(&payload).unwrap();

        loop {
            let registered = state
                .store
                .get("registered")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if registered {
                info!("User is registered.");
                break;
            }

            match state
                .http()
                .post(format!("{DISCOVERY_URL}/create_device"))
                .body(payload.clone())
                .send()
                .await
            {
                Ok(response) => match response.status() {
                    StatusCode::CREATED => {
                        if state
                            .store
                            .set("registered", serde_json::Value::Bool(true))
                            .is_ok()
                        {
                            info!("Device succesfully registered");
                            break;
                        }
                    }
                    StatusCode::OK => {
                        break;
                    }
                    status => {
                        warn!("Unexpected register status: {}, retrying...", status);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                },
                Err(e) => {
                    error!("Register request failed: {:?}, retrying...", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    })
}

#[derive(Deserialize, Serialize)]
pub struct ResolveDevicesResponse {
    pub devices: Vec<(DeviceId, EndpointAddr)>,
}

#[derive(Serialize, Deserialize)]
pub struct ResolveDevicesRequest {
    pub devices: Vec<DeviceId>,
    pub endpoint_id: EndpointId,
    pub device_id: DeviceId,
    pub signature: Signature,
}

pub async fn resolve_devices(
    state: &NectanState,
    devices: Option<Vec<DeviceId>>,
) -> Result<ResolveDevicesResponse> {
    let client = state.http_client.clone();

    let devices = match devices {
        None => state.devices.get_all_ids(),
        Some(d) => d,
    };

    if devices.is_empty() {
        info!("No devices to resolve.");
        return Ok(ResolveDevicesResponse { devices: vec![] });
    }

    info!("Resolving [{}] devices.", devices.len());

    let endpoint_id = state.endpoint_id();
    let device_id = state.device_id;
    let signature = state.sign(endpoint_id.as_bytes());

    let payload = ResolveDevicesRequest {
        devices,
        endpoint_id,
        device_id,
        signature,
    };
    let payload = postcard::to_allocvec(&payload).unwrap();

    let response = client
        .post(format!("{DISCOVERY_URL}/resolve_devices"))
        .body(payload)
        .send()
        .await?;

    info!("Resolve devices response status {}", response.status());
    let bytes = response.bytes().await?;
    let response: ResolveDevicesResponse = postcard::from_bytes(&bytes)?;

    info!("Resolved devices");
    Ok(response)
}
