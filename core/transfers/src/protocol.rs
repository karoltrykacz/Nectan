use anyhow::bail;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier};
use futures::StreamExt;
pub use iroh::EndpointId;
use iroh::{
    Endpoint, EndpointAddr,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler, Router},
};
use iroh_mdns_address_lookup::{DiscoveryEvent, MdnsAddressLookup};
use n0_error::e;
use rand::{rand_core::UnwrapErr, rngs::SysRng};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering::SeqCst},
    },
    time::Duration,
};
use tokio::{
    sync::{Mutex, OnceCell},
    task::JoinHandle,
};
use tracing::{Instrument, debug_span, error, info, trace, warn};
use uuid::Uuid;

use crate::{
    devices::{
        Device, DeviceId,
        DeviceStatus::{self, Nearby, Online},
        Devices, UserInfo, Username,
    },
    messages::{
        AppEvent, ConnectionOffer,
        NetMessage::{self, Ack},
        StreamableMessage, UiResponse,
    },
    storage::Storage,
    stream::StreamPair,
    transfers::{TransferDirection, Transfers},
    walker::TransferEntry,
};

pub const ONLINE_TIMEOUT: Duration = Duration::from_secs(10);
pub const PROTOCOL_VERSION: u64 = 0;
pub const DISCOVERY_URL: &str = "https://discovery.nectan.co";
pub const ALPN: &[u8] = b"nectan/0";

#[derive(Clone, Debug)]
pub struct NectanProtocol {
    #[allow(dead_code)]
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
                    remote_device_id,
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
                UiResponse::AcceptConnection => {
                    trace!("Connection accepted.");
                }
                UiResponse::Reject { reason } => {
                    error!("Connection rejected. Reason [{:?}]", reason);
                    let _ = NetMessage::Rejected { reason: None }
                        .write(&mut stream_tx)
                        .await;

                    // WAit for ack
                    let _ = NetMessage::read_async(&mut stream_rx).await;

                    return Err(e!(AcceptError::NotAllowed));
                }
                _ => panic!("This response is not allowed."),
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

        state
            .emit(AppEvent::Connected {
                device_id: remote_device_id,
            })
            .await;

        // Check again (mdns may lag a bit) (and it costs practically nothing)
        let is_nearby = devices.is_nearby(&remote_ep_id);

        let device = Device {
            username: remote_username.clone(),
            id: remote_device_id,
            status: if is_nearby { Nearby } else { Online },
            deleted: false,
            connection: Some(conn.clone()),
            completed_transfers: stored
                .as_ref()
                .map(|d| d.completed_transfers)
                .unwrap_or(0),
            total_exchanged_data: stored
                .as_ref()
                .map(|d| d.total_exchanged_data)
                .unwrap_or(0),
            fav: stored.as_ref().map(|d| d.fav).unwrap_or(false),
        };

        let _ = devices.insert(remote_device_id, &device);

        info!("Accepted connetion {}", remote_username);
        tokio::spawn(handle_connection(state, device, conn));

        Ok(())
    }
    async fn shutdown(&self) {}
}

async fn handle_connection(
    state: Arc<NectanState>,
    device: Device,
    conn: Connection,
) {
    let conn_id = conn.stable_id();
    info!("Handling connetion {conn_id}");
    let id = conn.stable_id().to_string();
    let span = debug_span!("connection", id);
    let sender_name = device.username;

    async move {
        while let Ok(pair) = StreamPair::accept(&conn).await {
            let span = debug_span!("stream", stream_id = %pair.stream_id());
            tokio::spawn(
                handle_stream(
                    state.clone(),
                    pair,
                    device.id,
                    sender_name.clone(),
                )
                .instrument(span),
            );
        }

        if state.devices.remove_conn(device.id, conn_id) {
            let _ = state
                .emit(AppEvent::DeviceWentOffline {
                    device_id: device.id,
                })
                .await;
        }
    }
    .instrument(span)
    .await;
}

async fn handle_stream(
    state: Arc<NectanState>,
    mut stream: StreamPair,
    sender: DeviceId,
    sender_name: Username,
) -> anyhow::Result<()> {
    let msg: NetMessage = stream.read().await?;

    match msg {
        NetMessage::TransferOfferMsg { offer } => {
            handle_transfer_offer(state, stream, sender, sender_name, offer)
                .await?;
        }
        NetMessage::TransferStream { transfer_id } => {
            state
                .transfers
                .forward_incoming_stream(transfer_id, stream)
                .await?;
        }
        NetMessage::TransferCancelled { transfer_id } => {
            state.transfers.transfer_cancelled(transfer_id)?;
        }
        NetMessage::Accepted
        | NetMessage::Rejected { .. }
        | NetMessage::Hello { .. } => {
            error!("Stream handler received forbidden message. {msg:#?}");
            bail!("Stream handler received forbidden message. {msg:#?}")
        }
        NetMessage::Ack => {
            let _ = stream.write(&Ack).await;
        }
    }

    Ok(())
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TransferOffer {
    pub transfer_name: String,
    pub transfer_id: Uuid,
    pub entries_num: u32,
    pub total_size: u64,
    pub entries: Arc<Vec<TransferEntry>>,
}

#[derive(Clone, Debug)]
pub struct TransferOfferRequest {
    pub sender_name: String,
    pub inner: TransferOffer,
    pub respond: tokio::sync::mpsc::Sender<UiResponse>,
}

async fn handle_transfer_offer(
    state: Arc<NectanState>,
    mut stream: StreamPair,
    sender: DeviceId,
    sender_name: Username,
    offer: Box<TransferOffer>,
) -> anyhow::Result<()> {
    let (respond, mut rx) = tokio::sync::mpsc::channel(1);

    // Check if offer is suspectible to auto accept
    if should_auto_accept(&state, sender) {
        let destination = state.storage.default_dir();
        stream
            .write(&NetMessage::from(UiResponse::AcceptOffer {
                destination: destination.clone(),
            }))
            .await?;
        info!("Auto-accepted transfer.");

        state
            .transfers
            .add_transfer(
                sender,
                sender_name,
                TransferDirection::Incoming,
                destination,
                *offer,
            )
            .await;

        return Ok(());
    }

    // Reject the offer if we already have some offer
    let offer = {
        let mut lock = state.transfer_offer.lock().await;
        if lock.is_some() {
            info!("Offer slot occupied.");
            None
        } else {
            info!("Offer slot empty.");
            let offer = TransferOfferRequest {
                sender_name: sender_name.to_string(),
                inner: *offer,
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

    if let UiResponse::AcceptOffer { destination } = r {
        state
            .transfers
            .add_transfer(
                sender,
                sender_name,
                TransferDirection::Incoming,
                destination,
                offer.inner,
            )
            .await;
    }

    Ok(())
}

#[derive(Clone)]
pub struct NectanState {
    pub transfers: Arc<Transfers>,
    pub app_event_tx: tokio::sync::mpsc::Sender<AppEvent>,
    /// Only one connection offer allowed at a time
    conn_offer: Arc<Mutex<Option<ConnectionOffer>>>,
    /// Only one transfer offer allowed at a time
    pub transfer_offer: Arc<Mutex<Option<TransferOfferRequest>>>,
    pub devices: Devices,
    mdns: MdnsAddressLookup,
    router: OnceCell<Router>,
    signing_key: SigningKey,
    userinfo: UserInfo,
    device_id: DeviceId,
    http_client: Client,
    pub seq_num: SeqNumber,
    pub storage: Arc<Storage>,
}

impl NectanState {
    pub async fn build(
        userinfo: UserInfo,
        device_id: DeviceId,
        signing_key: SigningKey,
        devices: Devices,
        app_event_tx: tokio::sync::mpsc::Sender<AppEvent>,
        storage: Arc<Storage>,
        mdns: MdnsAddressLookup,
    ) -> Self {
        NectanState {
            transfers: Arc::new(Transfers::new(
                devices.clone(),
                app_event_tx.clone(),
            )),
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
            storage,
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

    pub fn toggle_mdns(&self) {}

    pub fn router(&self) -> Option<Router> {
        self.router.get().cloned()
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

    pub fn endpoint(&self) -> Option<Endpoint> {
        self.router.get().map(|r| r.endpoint().clone())
    }

    pub fn endpoint_id(&self) -> Option<EndpointId> {
        self.router.get().map(|r| r.endpoint().id())
    }

    pub fn endpoint_ticket(&self) -> Option<String> {
        self.router.get().map(|r| r.endpoint().id().to_string())
    }
}

impl std::fmt::Debug for NectanState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NectanState").finish()
    }
}

pub fn start_mdns_discovery(state: &NectanState) {
    let state = Arc::new(state.clone());
    tokio::spawn(async move {
        info!("Started local discovery");

        let mut events = state.mdns.subscribe().await;

        while let Some(event) = events.next().await {
            match event {
                DiscoveryEvent::Discovered { endpoint_info, .. } => {
                    let ep_id = endpoint_info.endpoint_id;

                    // Endpoint id is safe to unwrap because mdns is callled after the router is succesfully binded
                    if ep_id == state.endpoint_id().unwrap() {
                        continue;
                    }

                    if state.devices.add_nearby(ep_id) {
                        info!("Found device [MDNS].");
                        state.emit(AppEvent::FoundNearby).await;
                        let _ =
                            connect(state.clone(), endpoint_info.endpoint_id);
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

pub enum ConnectStatus {
    ConnectionFailed,
    AlreadyConnected,
    RequestDelivered,
    AuthFailed,
    UserRejected,
    // Ok
    Accepted(Device),
}

impl std::fmt::Display for ConnectStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ConnectStatus::ConnectionFailed => "Connection failed",
            ConnectStatus::AlreadyConnected => "Already connected",
            ConnectStatus::RequestDelivered => "Request delivered",
            ConnectStatus::AuthFailed => "Authentication failed",
            ConnectStatus::UserRejected => "User rejected the request",
            ConnectStatus::Accepted(_device) => "Accepted",
        };
        write!(f, "{}", s)
    }
}

pub fn connect(
    state: Arc<NectanState>,
    target: EndpointId,
) -> tokio::sync::mpsc::Receiver<ConnectStatus> {
    let (tx, rx) = tokio::sync::mpsc::channel(1);

    tokio::spawn(async move {
        if state.devices.is_alive(target) {
            warn!("Connecting to the device cancelled. [CONNECTED]",);
            let _ = tx.send(ConnectStatus::AlreadyConnected).await;
            return;
        }

        let Some(endpoint) = state.endpoint() else {
            warn!("Connection to device failed. Router not attached.",);
            let _ = tx.send(ConnectStatus::ConnectionFailed).await;
            return;
        };

        let Ok(conn) = endpoint.connect(target, ALPN).await else {
            error!("Connecting to the device failed.");
            let _ = tx.send(ConnectStatus::ConnectionFailed).await;
            return;
        };

        let Ok((mut stream_tx, mut stream_rx)) = conn.open_bi().await else {
            let _ = tx.send(ConnectStatus::ConnectionFailed).await;
            return;
        };

        let signature = state.signing_key.sign(target.as_bytes());
        let username = state.userinfo.username();

        if (NetMessage::Hello {
            id: state.device_id,
            username: username.clone(),
            signature,
        }
        .write(&mut stream_tx)
        .await
        .is_err())
        {
            let _ = tx.send(ConnectStatus::ConnectionFailed).await;
            return;
        }

        let _ = tx.send(ConnectStatus::RequestDelivered).await;

        let Ok(res) = NetMessage::read_async(&mut stream_rx).await else {
            error!("Failed to read remote device response.");
            let _ = tx.send(ConnectStatus::ConnectionFailed).await;
            return;
        };

        match res {
            NetMessage::Hello {
                id: remote_device_id,
                username: remote_username,
                signature: remote_signature,
                ..
            } => {
                if remote_device_id
                    .verify(endpoint.id().as_bytes(), &remote_signature)
                    .is_err()
                {
                    let _ = tx.send(ConnectStatus::AuthFailed).await;
                    return;
                }

                let stored_remote_device: Option<Device> =
                    state.devices.get(&remote_device_id);

                let is_nearby = state.devices.is_nearby(&target);

                let device = Device {
                    username: remote_username.clone(),
                    id: remote_device_id,
                    connection: Some(conn.clone()),
                    deleted: false,
                    status: if is_nearby { Nearby } else { Online },
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
                let _ = tx.send(ConnectStatus::Accepted(device.clone())).await;
                handle_connection(state, device, conn).await;
            }
            NetMessage::Rejected { reason } => {
                warn!("Remote device rejected connection. [{:?}]", reason);
                let _ = tx.send(ConnectStatus::UserRejected).await;
                let _ = NetMessage::Ack.write(&mut stream_tx).await;

                return;
            }
            r => {
                error!("Bad remote device response. {:#?}", r);
                let _ = tx.send(ConnectStatus::ConnectionFailed).await;
                return;
            }
        }
    });

    rx
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

pub fn announce_endpoint(state: Arc<NectanState>) {
    tokio::spawn(async move {
        let endpoint_id = state.endpoint_id().unwrap();
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
                                info!("Announced endpoint");
                                break postcard::from_bytes::<
                                    EndpointAnncounceResponse,
                                >(&body)
                                .unwrap()
                                .seq_num;
                            }
                            Err(e) => error!(
                                "Annouce endpoint. Failed to parse response: {:?}, retrying...",
                                e
                            ),
                        }
                    }
                    StatusCode::NOT_FOUND => {
                        error!("Announce endpoint. NOT_FOUND");
                        // panic here ??? - register???
                        return;
                    }
                    StatusCode::UNAUTHORIZED => {
                        error!("Announce endpoint. UNAUTHORIZED");
                        return;
                    }
                    status => {
                        error!(
                            "Announce endpoint unexpected status: {status}, retrying..."
                        )
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

        // After announcing resolve devices
        if let Ok(response) = resolve_devices(&state, None).await {
            for (_device_id, endpoint_id) in response.devices {
                connect(state.clone(), endpoint_id);
            }
        }
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
            if state.storage.registered() {
                info!("User registered.");
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
                        info!("Device succesfully registered");
                        let _ = state.storage.update(|s| {
                            s.registered = true;
                        });
                    }
                    StatusCode::OK => {
                        break;
                    }
                    status => {
                        warn!(
                            "Unexpected register status: {}, retrying...",
                            status
                        );
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
    pub devices: Vec<(DeviceId, EndpointId)>,
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
) -> anyhow::Result<ResolveDevicesResponse> {
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

    let endpoint_id = state.endpoint_id().unwrap();
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

fn should_auto_accept(s: &NectanState, device: DeviceId) -> bool {
    let device = s.devices.get(&device).unwrap();

    if device.fav && s.storage.autoaccept_from_favourites() {
        return true;
    }

    if device.status == DeviceStatus::Nearby
        && s.storage.autoaccept_from_nearby()
    {
        return true;
    }

    false
}
