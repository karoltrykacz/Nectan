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
use reqwest::{Client, ClientBuilder, StatusCode};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, OnceCell};
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
pub const MDNS_NAME: &[u8] = b"nectan_user";

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

    #[must_use]
    pub fn endpoint(&self) -> Endpoint {
        self.endpoint.clone()
    }

    #[must_use]
    pub fn state(&self) -> Arc<NectanState> {
        Arc::clone(&self.state)
    }
}

impl ProtocolHandler for NectanProtocol {
    async fn accept(
        &self,
        conn: Connection,
    ) -> Result<(), AcceptError> {
        let state = self.state();
        let endpoint = self.endpoint();

        let devices = &state.devices;
        let singing_key = &state.signing_key;
        let my_device_id = state.device_id;

        let remote_ep_id = conn.remote_id();

        info!(
            "Accepting new connection. {}",
            remote_ep_id.to_string()
        );

        let (mut stream_tx, mut stream_rx) = conn.accept_bi().await?;

        let Ok(msg) = NetMessage::read_async(&mut stream_rx).await
        else {
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
                error!(
                    "Failed to read response to connection request."
                );
                return Err(e!(AcceptError::NotAllowed));
            };

            match r {
                UiResponse::AcceptConnection => {
                    trace!("Connection accepted.");
                }
                UiResponse::Reject { reason } => {
                    error!(
                        "Connection rejected. Reason [{:?}]",
                        reason
                    );
                    let _ = NetMessage::Rejected { reason: None }
                        .write(&mut stream_tx)
                        .await;

                    // Wait for ack
                    let _ =
                        NetMessage::read_async(&mut stream_rx).await;

                    return Err(e!(AcceptError::NotAllowed));
                }
                UiResponse::AcceptOffer { .. } => {
                    panic!("This response is not allowed.");
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
                .map_or(0, |d| d.completed_transfers),
            total_exchanged_data: stored
                .as_ref()
                .map_or(0, |d| d.total_exchanged_data),
            fav: stored.as_ref().is_some_and(|d| d.fav),
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
            let span =
                debug_span!("stream", stream_id = %pair.stream_id());
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
            let () = state
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
            handle_transfer_offer(
                state,
                stream,
                sender,
                sender_name,
                offer,
            )
            .await?;
        }
        NetMessage::TextMessage { content } => {
            state
                .emit(AppEvent::TextMessage {
                    from: sender_name.clone(),
                    device_id: sender,
                    content,
                })
                .await;
            let _ = stream.write(&Ack).await;
        }
        NetMessage::TransferStream { transfer_id } => {
            state
                .transfers
                .forward_incoming_stream(transfer_id, stream)
                .await?;
        }
        NetMessage::TransferCancelled { transfer_id } => {
            state.transfers.transfer_cancelled(transfer_id).await?;
        }
        NetMessage::Accepted
        | NetMessage::Rejected { .. }
        | NetMessage::Hello { .. } => {
            error!(
                "Stream handler received forbidden message. {msg:#?}"
            );
            bail!(
                "Stream handler received forbidden message. {msg:#?}"
            )
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
            .emit(AppEvent::Autoaccepted {
                from: sender_name.clone(),
            })
            .await;

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
            reason: Some(
                "Receiver is busy with another offer.".to_string(),
            ),
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
    mdns: Arc<Mutex<Option<MdnsAddressLookup>>>,
    router: OnceCell<Router>,
    signing_key: SigningKey,
    userinfo: UserInfo,
    device_id: DeviceId,
    http_client: Client,
    pub storage: Arc<Storage>,
}

impl NectanState {
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        userinfo: UserInfo,
        device_id: DeviceId,
        signing_key: SigningKey,
        devices: Devices,
        transfers: Transfers,
        app_event_tx: tokio::sync::mpsc::Sender<AppEvent>,
        storage: Arc<Storage>,
        mdns: Arc<Mutex<Option<MdnsAddressLookup>>>,
    ) -> Self {
        let client_builder = ClientBuilder::new();

        #[cfg(target_os = "android")]
        {
            let roots = webpki_root_certs::TLS_SERVER_ROOT_CERTS
                .iter()
                .map(|cert| {
                    reqwest::Certificate::from_der(cert.as_ref())
                })
                .collect::<reqwest::Result<Vec<_>>>()
                .expect("Failed to get roots");
            client_builder = client_builder.tls_certs_only(roots);
        }

        let http_client = client_builder
            .build()
            .expect("Failed to build http client.");

        NectanState {
            transfers: Arc::new(transfers),
            app_event_tx,
            conn_offer: Arc::new(Mutex::new(None)),
            transfer_offer: Arc::new(Mutex::new(None)),
            devices,
            mdns,
            router: OnceCell::new(),
            signing_key,
            userinfo,
            device_id,
            http_client,
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
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.debug_struct("NectanState").finish()
    }
}

async fn activate_mdns(s: &NectanState) {
    loop {
        let endpoint = s.endpoint().unwrap();
        let mdns = MdnsAddressLookup::builder()
            .service_name("nectan_user")
            .advertise(true)
            .build(endpoint.id());

        let mdns = match mdns {
            Ok(mdns) => {
                endpoint.address_lookup().unwrap().add(mdns.clone());
                info!("Attached mdns address lookup.");
                mdns
            }
            Err(e) => {
                error!("Failed to build mdns. {}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };
        *s.mdns.lock().await = Some(mdns);
        return;
    }
}

pub async fn start_mdns_discovery(s: Arc<NectanState>) {
    activate_mdns(&s).await;
    handle_mdns_events(&s).await;
}

async fn handle_mdns_events(state: &Arc<NectanState>) {
    let Some(mdns) = state.mdns.lock().await.clone() else {
        error!("Failed to get mdns.");
        return;
    };

    let mut events = mdns.subscribe().await;
    while let Some(event) = events.next().await {
        match event {
            DiscoveryEvent::Discovered { endpoint_info, .. } => {
                let ep_id = endpoint_info.endpoint_id;

                if ep_id == state.endpoint_id().unwrap() {
                    continue;
                }

                if state.devices.add_nearby(ep_id) {
                    info!("Found device [MDNS].");
                    state.emit(AppEvent::FoundNearby).await;
                    let _ = connect(
                        state.clone(),
                        EndpointAddr::from(endpoint_info.endpoint_id),
                    );
                }
            }
            DiscoveryEvent::Expired { endpoint_id } => {
                state.devices.remove_nearby(endpoint_id);
            }
            _ => {}
        }
    }
}

#[allow(clippy::large_enum_variant)]
pub enum ConnectStatus {
    ConnectionFailed,
    AlreadyConnected,
    RequestDelivered,
    AuthFailed,
    UserRejected,
    Accepted(Device),
}

impl std::fmt::Display for ConnectStatus {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        let s = match self {
            ConnectStatus::ConnectionFailed => "Connection failed",
            ConnectStatus::AlreadyConnected => "Already connected",
            ConnectStatus::RequestDelivered => "Request delivered",
            ConnectStatus::AuthFailed => "Authentication failed",
            ConnectStatus::UserRejected => {
                "User rejected the request"
            }
            ConnectStatus::Accepted(_device) => "Accepted",
        };
        write!(f, "{s}")
    }
}

#[allow(clippy::too_many_lines)]
pub fn connect(
    state: Arc<NectanState>,
    target: EndpointAddr,
) -> tokio::sync::mpsc::Receiver<ConnectStatus> {
    let (tx, rx) = tokio::sync::mpsc::channel(1);

    tokio::spawn(async move {
        if state.devices.is_alive(target.id) {
            warn!("Connecting to the device cancelled. [CONNECTED]",);
            let _ = tx.send(ConnectStatus::AlreadyConnected).await;
            return;
        }

        let Some(endpoint) = state.endpoint() else {
            warn!(
                "Connection to device failed. Router not attached.",
            );
            let _ = tx.send(ConnectStatus::ConnectionFailed).await;
            return;
        };

        let Ok(conn) = endpoint.connect(target.clone(), ALPN).await
        else {
            error!("Connecting to the device failed.");
            let _ = tx.send(ConnectStatus::ConnectionFailed).await;
            return;
        };

        let Ok((mut stream_tx, mut stream_rx)) = conn.open_bi().await
        else {
            let _ = tx.send(ConnectStatus::ConnectionFailed).await;
            return;
        };

        let signature = state.signing_key.sign(target.id.as_bytes());
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

        let Ok(res) = NetMessage::read_async(&mut stream_rx).await
        else {
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
                    .verify(
                        endpoint.id().as_bytes(),
                        &remote_signature,
                    )
                    .is_err()
                {
                    let _ = tx.send(ConnectStatus::AuthFailed).await;
                    return;
                }

                let stored_remote_device: Option<Device> =
                    state.devices.get(&remote_device_id);

                let is_nearby = state.devices.is_nearby(&target.id);

                let device = Device {
                    username: remote_username.clone(),
                    id: remote_device_id,
                    connection: Some(conn.clone()),
                    deleted: false,
                    status: if is_nearby { Nearby } else { Online },
                    completed_transfers: stored_remote_device
                        .as_ref()
                        .map_or(0, |d| d.completed_transfers),
                    total_exchanged_data: stored_remote_device
                        .as_ref()
                        .map_or(0, |d| d.total_exchanged_data),
                    fav: stored_remote_device
                        .as_ref()
                        .is_some_and(|d| d.fav),
                };
                let _ =
                    state.devices.insert(remote_device_id, &device);

                state
                    .emit(AppEvent::Connected {
                        device_id: remote_device_id,
                    })
                    .await;

                info!("Connected [{}]", remote_username);
                let _ = tx
                    .send(ConnectStatus::Accepted(device.clone()))
                    .await;
                handle_connection(state, device, conn).await;
            }
            NetMessage::Rejected { reason } => {
                warn!(
                    "Remote device rejected connection. [{:?}]",
                    reason
                );
                let _ = tx.send(ConnectStatus::UserRejected).await;
                let _ = NetMessage::Ack.write(&mut stream_tx).await;
            }
            r => {
                error!("Bad remote device response. {:#?}", r);
                let _ =
                    tx.send(ConnectStatus::ConnectionFailed).await;
            }
        }
    });

    rx
}

#[must_use]
pub fn gen_device_id() -> (DeviceId, SigningKey) {
    let mut csprng = UnwrapErr(SysRng);
    let key = SigningKey::generate(&mut csprng);
    (key.verifying_key(), key)
}

#[derive(Deserialize, Serialize)]
pub struct EndpointAnnouncePayload {
    pub device_id: DeviceId,
    pub endpoint_id: EndpointId,
    pub devices_to_resolve: Vec<DeviceId>,
    pub nonce: Uuid,
    pub signature: Signature,
}

#[derive(Deserialize, Serialize)]
pub struct EndpointAnncounceResponse {
    pub devices: Vec<(DeviceId, EndpointId)>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Nonce {
    pub inner: Uuid,
}

pub async fn get_nonce(state: &NectanState) -> anyhow::Result<Uuid> {
    let r = state
        .http()
        .post(format!("{DISCOVERY_URL}/nonce"))
        .send()
        .await?;
    let bytes = r.bytes().await?;
    Ok(postcard::from_bytes::<Nonce>(&bytes)?.inner)
}

pub async fn announce_endpoint(state: Arc<NectanState>) {
    let endpoint_id = state.endpoint_id().unwrap();
    let device_id = state.device_id();
    let devices_to_resolve = state.devices.get_all_ids();
    let response = loop {
        println!("Announcing");

        let Ok(nonce) = get_nonce(&state).await else {
            info!("Failed to get nonce");
            tokio::time::sleep(Duration::from_millis(1000)).await;
            continue;
        };
        info!("Generated nonce {:?}", nonce);

        let message = (
            device_id.as_bytes(),
            endpoint_id.as_bytes(),
            &nonce.to_bytes_le(),
        );
        let signature: Signature =
            state.sign(&postcard::to_allocvec(&message).unwrap());

        let payload = EndpointAnnouncePayload {
            device_id,
            endpoint_id,
            signature,
            devices_to_resolve: devices_to_resolve.clone(),
            nonce,
        };
        let payload = postcard::to_allocvec(&payload).unwrap();

        match state
            .http()
            .post(format!("{DISCOVERY_URL}/announce_endpoint"))
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
                            >(
                                &body
                            )
                            .unwrap();
                        }
                        Err(e) => error!(
                            "Annouce endpoint. Failed to parse response: {:?}, retrying...",
                            e
                        ),
                    }
                }
                StatusCode::UNAUTHORIZED => {
                    error!("Announce endpoint. UNAUTHORIZED");
                    return;
                }
                status => {
                    error!(
                        "Announce endpoint unexpected status: {status}, retrying..."
                    );
                }
            },
            Err(e) => {
                error!("Announce enpoint network error {e}.");
            }
        }
        warn!("Announce endpoint retry.");
        tokio::time::sleep(Duration::from_millis(5000)).await;
    };

    trace!("Endpoint announced.");

    for (_device_id, endpoint_id) in response.devices {
        connect(state.clone(), EndpointAddr::from(endpoint_id));
    }
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

pub async fn send_text(
    state: &NectanState,
    target: DeviceId,
    content: String,
) -> anyhow::Result<()> {
    let Some(device) = state.devices.get(&target) else {
        bail!("Device not found")
    };
    let Some(conn) = device.connection.clone() else {
        bail!("Offline")
    };
    let Ok((mut tx, _rx)) = conn.open_bi().await else {
        bail!("Offline")
    };
    let msg = NetMessage::TextMessage { content };
    if msg.write(&mut tx).await.is_err() {
        bail!("Offline")
    }
    Ok(())
}
