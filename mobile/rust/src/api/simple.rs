use crate::frb_generated::StreamSink;
use anyhow::Result;
pub use base64::{Engine as _, engine::general_purpose::STANDARD};
pub use chrono::{DateTime, Utc};
pub use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
pub use flutter_rust_bridge::DartFnFuture;
use flutter_rust_bridge::setup_log_to_console;
use iroh::EndpointId;
use jwalk::ClientState;
pub use log::info;
use nectan_core::{
    common::get_signing_key,
    devices::{
        self, Device, Devices, UserInfo, Username, device_id_from_base64, device_id_to_base64,
    },
    messages::{AppEvent, UiResponse},
    protocol::{NectanState, TransferOffer},
    setup_core,
    storage::Storage,
    transfers::{TransferItemError, TransferOfferError, TransferStatus, send_contents},
    walker::Walker,
};
pub use rand_core::OsRng;
pub use reqwest::{Client, StatusCode};
pub use serde::{Deserialize, Serialize};
pub use serde_json::{Value, de};
pub use std::{
    cell::OnceCell,
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};
pub use tokio::{
    runtime::Runtime,
    sync::{Mutex, RwLock, mpsc, mpsc::Receiver},
    time::{Duration, sleep},
};
use tracing_subscriber::EnvFilter;
pub use tracing_subscriber::fmt::format;
pub use uuid::Uuid;

static ENGINE: OnceLock<Engine> = OnceLock::new();
static CLIENT: OnceLock<Client> = OnceLock::new();

pub fn get_username() -> Option<String> {
    store().username().map(|u| u.as_string())
}

pub fn get_base64_device_id() -> String {
    device_id_to_base64(&state().device_id())
}

/// flutter_rust_bridge:ignore
pub struct Engine {
    pub state: tokio::sync::OnceCell<Arc<NectanState>>,
    pub lookup_addr_placeholder: Mutex<Option<EndpointId>>,
    pub walker: Arc<Mutex<Option<Walker>>>,
    pub transfers_accumulator: Mutex<Vec<TransferMetaDto>>,
    pub storage: Arc<Storage>,
}

/// flutter_rust_bridge:ignore
pub fn engine() -> &'static Engine {
    ENGINE.get().expect("Engine not initialized")
}

/// flutter_rust_bridge:ignore
pub fn store() -> Arc<Storage> {
    engine().storage.clone()
}

/// flutter_rust_bridge:ignore
pub fn client() -> reqwest::Client {
    CLIENT.get_or_init(Client::new).clone()
}

/// flutter_rust_bridge:ignore
pub fn state() -> Arc<NectanState> {
    engine().state.get().unwrap().clone()
}

/// The username must be valid before calling this
pub fn save_username(username: String) -> anyhow::Result<()> {
    log::info!("Saving username.");
    let uname = Username::new(username).expect("Invalid username");
    store().set(nectan_core::storage::Value::Username(uname))
}

/// Adds received content to gallery
static SCAN_DIR_CALLBACK: OnceLock<Box<dyn Fn(String) -> DartFnFuture<()> + Send + Sync>> =
    OnceLock::new();

pub fn register_callback(cb: impl Fn(String) -> DartFnFuture<()> + Send + Sync + 'static) {
    SCAN_DIR_CALLBACK.set(Box::new(cb)).ok();
}

pub async fn scan_dir(msg: String) {
    if let Some(cb) = SCAN_DIR_CALLBACK.get() {
        cb(msg).await;
    }
}

/// TODO
pub fn set_show_connected_to_nearby(show_again: bool) {
    // let _ = engine()
    //     .store
    //     .set("show_connected_to_nearby", Value::Bool(show_again));
}

/// TODO
pub fn show_connected_to_nearby() -> bool {
    // engine()
    //     .store
    //     .get("show_connected_to_nearby")
    //     .unwrap_or(Value::Bool(true)) // show that dialog by default
    //     .as_bool()
    //     .unwrap_or(true)
    true
}

pub fn init_engine(base_path: String) -> bool {
    setup_log_to_console(log::LevelFilter::Debug);

    let _ = tracing_subscriber::fmt()
    .with_env_filter(EnvFilter::new(
    "rustls=off,rustls::client=off,rustls::server=off,nectan-core=debug,rust_lib_nectan_mobile=debug,transfers=debug,devices=debug,utils=debug,types=debug,rust_lib_nectan_mobile::simple=debug",
    ))
    .try_init();

    if let Some(e) = ENGINE.get() {
        log::warn!("🟨 Engine already intialized");
        e.storage.username().is_none()
    } else {
        let path = std::path::PathBuf::from(base_path).join("Nectan");
        info!("Storage path {path:?}");

        let storage = Storage::new(&path);
        let is_first_launch = storage.username().is_none();

        let _ = ENGINE.set(Engine {
            state: tokio::sync::OnceCell::new(),
            lookup_addr_placeholder: Mutex::new(None),
            walker: Arc::new(Mutex::new(None)),
            transfers_accumulator: Mutex::new(Vec::new()),
            storage: Arc::new(storage),
        });

        log::info!("Engine intialized.");

        is_first_launch
    }
}

// Assuming this is the enum in your current crate
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub enum TransferDirection {
    Outcoming,
    Incoming,
}

impl From<nectan_core::transfers::TransferDirection> for TransferDirection {
    fn from(child_dir: nectan_core::transfers::TransferDirection) -> Self {
        match child_dir {
            nectan_core::transfers::TransferDirection::Outcoming => TransferDirection::Outcoming,
            nectan_core::transfers::TransferDirection::Incoming => TransferDirection::Incoming,
        }
    }
}

// This is BULLSHIT
#[derive(Deserialize, Serialize, PartialEq, Eq, Debug, Clone, Copy)]
pub enum TransferStateDto {
    Processing,
    Cancelled,
    Finished,
}

impl From<TransferStatus> for TransferStateDto {
    fn from(state: TransferStatus) -> Self {
        match state {
            TransferStatus::Downloading => TransferStateDto::Processing,
            TransferStatus::Finished => TransferStateDto::Finished,
            TransferStatus::CancelledByRemote => TransferStateDto::Cancelled,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TransferMetaDto {
    pub id: String,
    pub name: String,
    /// base64 encoding
    pub remote_device_id: String,
    pub remote_device_name: String,
    pub direction: TransferDirection,
    pub total_size: u64,
    pub sent_bytes: u64,
    pub entires_num: u64,
    pub status: TransferStateDto,
    pub speed_bps: u64,
    pub failed_entries: u64,
}

#[derive(Debug, Clone)]
pub struct TransferItemDto {
    pub id: u64,
    pub path: String,
    pub name: String, // derive name from path
    pub size: u64,
    pub sent_bytes: u64,
    pub error: Option<SendItemErrorDto>, // error enum
}

#[derive(Debug, Deserialize, Clone)]
pub enum SendItemErrorDto {
    FileIOError,
    FileOpenPermissionDenied,
    FileNotFound,
    StreamError,
    OpenFail,
    Terminated,
    FileAlreadyExisted,
}

impl From<TransferItemError> for SendItemErrorDto {
    fn from(item: TransferItemError) -> Self {
        match item {
            TransferItemError::FileAlreadyExisted => Self::FileAlreadyExisted,
            TransferItemError::FileNotFound => Self::FileNotFound,
            TransferItemError::PermissionDenied => Self::FileOpenPermissionDenied,
            TransferItemError::FileIOError => Self::FileIOError,
            TransferItemError::OpenFail => Self::OpenFail,
            // those wont ever be translated anyways
            TransferItemError::Terminated => Self::Terminated,
            TransferItemError::StreamError => Self::StreamError,
        }
    }
}
fn init_client() {
    if CLIENT.get().is_none() {
        let client = reqwest::ClientBuilder::new()
            .build()
            .expect("nothing should fail");

        CLIENT.set(client).expect("Failed to set client");
    }
}

pub async fn init_app(
    app_dir: String,
    devices_sink: StreamSink<Vec<DeviceDto>>,
    new_connection_sink: StreamSink<ConnectionRequest>,
    transfers_sink: StreamSink<Vec<TransferMetaDto>>,
    transfer_offer_sink: StreamSink<TransferOfferDto>,
    found_device_nearby_sink: StreamSink<DeviceDto>,
) -> Result<String, String> {
    let e = engine();
    if e.state.initialized() {
        return Err("App already initalized".to_string());
    }

    init_client();

    let storage = e.storage.clone();
    let path = std::path::PathBuf::from(app_dir);
    let devices = Devices::new(Some(path)).unwrap();

    let signing_key = get_signing_key(&storage);
    let device_id = signing_key.verifying_key();
    let (tx, rx) = tokio::sync::mpsc::channel(128);

    let username = storage
        .username()
        .unwrap_or(Username::new("Default").unwrap());
    let userinfo = UserInfo::new(username);

    let state = setup_core(tx, device_id, userinfo, signing_key, devices, storage)
        .await
        .unwrap();

    e.state.set(state).expect("Failed to set state!");

    start_event_listener(
        rx,
        devices_sink,
        new_connection_sink,
        transfer_offer_sink,
        transfers_sink,
        found_device_nearby_sink,
    );

    log::info!("🟦 App initialized.");

    Ok("Ok".to_string())
}

pub fn get_all_transfers() -> Vec<TransferMetaDto> {
    Vec::new()
    // state()
    //     .transfers_pool
    //     .get_transfers()
    //     .iter()
    //     .map(|t| TransferMetaDto {
    //         id: t.0.to_string(),
    //         name: t.1.name.clone(),
    //         direction: t.1.direction.into(),
    //         remote_device_id: device_id_to_base64(&t.1.remote_device),
    //         remote_device_name: t.1.remote_device_name.clone(),
    //         total_size: t.1.total_size,
    //         entires_num: t.1.entries_num,
    //         status: t.1.state.into(),
    //         sent_bytes: 0, // only availiable in pending
    //         speed_bps: 0,  // populated by the transfers controller
    //         failed_entries: t.1.failed_entries,
    //     })
    //     .collect()
}
/// ???
pub struct ConnectionRequest {
    pub username: String,
}

pub async fn get_failed_transfer_items(
    transfer_id: String,
    offset: u64,
    limit: u64,
) -> Vec<TransferItemDto> {
    Vec::new()
    // let transfer_id = Uuid::parse_str(&transfer_id).expect("Invalid UUID");
    // let s = state();
    // let Some(meta) = s.transfers_pool.get_transfer_meta(transfer_id) else {
    //     return Vec::new();
    // };
    // let parent = meta.root_path;
    // s.transfers_pool
    //     .get_failed_items_range(transfer_id, offset, limit)
    //     .iter()
    //     .map(|i| {
    //         // todo  impl from for tranfer dto
    //         let path = format!("{}/{}", parent, i.path); // this shit is probably needed!!
    //         let error = i.err.map(SendItemErrorDto::from);
    //         TransferItemDto {
    //             id: i.id,
    //             name: Path::new(&i.path)
    //                 .file_name()
    //                 .unwrap_or_default()
    //                 .to_string_lossy()
    //                 .to_string(),
    //             path,
    //             size: i.file_size,
    //             sent_bytes: i.sent_bytes,
    //             error,
    //         }
    //     })
    //     .collect()
}

pub async fn get_transfer_items(
    transfer_id: String,
    offset: u64,
    limit: u64,
) -> Vec<TransferItemDto> {
    Vec::new()
    // let transfer_id = Uuid::parse_str(&transfer_id).expect("Invalid UUID");
    // let s = state();
    // let Some(meta) = s.transfers_pool.get_transfer_meta(transfer_id) else {
    //     return Vec::new();
    // };
    // let parent = meta.root_path;
    // s.transfers_pool
    //     .get_items_range(transfer_id, offset, limit)
    //     .iter()
    //     .map(|i| {
    //         // todo  impl from for tranfer dto
    //         let path = format!("{}/{}", parent, i.path);
    //         let error = i.err.map(SendItemErrorDto::from);
    //         TransferItemDto {
    //             id: i.id,
    //             name: Path::new(&i.path)
    //                 .file_name()
    //                 .unwrap_or_default()
    //                 .to_string_lossy()
    //                 .to_string(),
    //             path,
    //             size: i.file_size,
    //             sent_bytes: i.sent_bytes,
    //             error,
    //         }
    //     })
    //     .collect()
}

pub struct TransferOfferDto {
    pub sender_device_name: String,
    pub transfer_id: String,
    pub name: String,
    pub size: u64,
    pub files_num: u32,
    /// Filename and size
    pub files: Vec<(String, u64, bool)>,
}

pub async fn respond_to_transfer_offer(accept: bool, destination: String) {
    let r = match accept {
        true => UiResponse::AcceptOffer {
            destination: PathBuf::from(destination),
        },
        false => UiResponse::Reject { reason: None },
    };
    state().respond_transfer_offer(r).await;
}

pub async fn issue_code(code: String) -> Result<String, String> {
    nectan_core::code_lookup::issue_code(&state(), code)
        .await
        .map_err(|e| e.to_string())
}

pub async fn lookup_code(code: String) -> Result<String, String> {
    let ep_id = nectan_core::code_lookup::lookup_code(code, &state())
        .await
        .map_err(|e| e.to_string())?;
    *engine().lookup_addr_placeholder.lock().await = Some(ep_id);
    Ok("Code found.".to_string())
}

/// flutter_rust_bridge:ignore
fn start_event_listener(
    mut event_rx: tokio::sync::mpsc::Receiver<AppEvent>,
    devices_sink: StreamSink<Vec<DeviceDto>>,
    new_connection_sink: StreamSink<ConnectionRequest>,
    transfer_offer_sink: StreamSink<TransferOfferDto>,
    transfers_sink: StreamSink<Vec<TransferMetaDto>>,
    found_device_nearby_sink: StreamSink<DeviceDto>,
) {
    flutter_rust_bridge::spawn(async move {
        while let Some(ev) = event_rx.recv().await {
            match ev {
                AppEvent::TransfersUpdated => {
                    // update_transfers(&w);
                    log::info!("Transfers changed.");
                }
                AppEvent::IncomingTransferOffer { offer } => {
                    log::info!("New transfer offer.");

                    let files = offer
                        .inner
                        .entries
                        .iter()
                        .map(|e| (e.path.to_string_lossy().to_string(), e.size, e.is_dir))
                        .collect();
                    let new_offer = TransferOfferDto {
                        sender_device_name: offer.sender_name,
                        transfer_id: offer.inner.transfer_id.to_string(),
                        name: offer.inner.transfer_name,
                        size: offer.inner.total_size,
                        files_num: offer.inner.entries_num,
                        files,
                    };
                    let _ = transfer_offer_sink.add(new_offer);
                }
                AppEvent::ConnectionOffer { offer } => {
                    log::info!("New connection offer.");

                    let conn_req = ConnectionRequest {
                        username: offer.username.as_string(),
                    };

                    if let Err(e) = new_connection_sink.add(conn_req) {
                        log::error!("‼️ Critical new connection request error: {}", e);
                    };
                }
                AppEvent::FoundNearby => {
                    log::info!("Found nearby device");
                    update_devices_sink(&devices_sink);
                }
                AppEvent::Connected { .. } => {
                    log::info!("Connected to device");
                    update_devices_sink(&devices_sink);
                }
                AppEvent::TransferOfferDelivered => {}
                AppEvent::DeviceWentOffline { .. } => {
                    log::info!("Device went offline");
                    update_devices_sink(&devices_sink);
                }
            }
        }
    });
}

async fn sync_accumulator(engine: &Engine) -> Vec<TransferMetaDto> {
    let fresh = get_all_transfers();
    let mut acc = engine.transfers_accumulator.lock().await;

    for fresh_t in &fresh {
        if let Some(existing) = acc.iter_mut().find(|t| t.id == fresh_t.id) {
            existing.status = fresh_t.status;
            existing.name = fresh_t.name.clone();
            existing.remote_device_id = fresh_t.remote_device_id.clone();
            existing.remote_device_name = fresh_t.remote_device_name.clone();
            existing.direction = fresh_t.direction;
            existing.total_size = fresh_t.total_size;
            existing.entires_num = fresh_t.entires_num;
        } else {
            acc.push(fresh_t.clone());
        }
    }
    acc.retain(|t| fresh.iter().any(|f| f.id == t.id));
    acc.clone()
}

pub async fn respond_to_connection_req(accept: bool) {
    let r = match accept {
        true => UiResponse::AcceptConnection,
        false => UiResponse::Reject { reason: None },
    };
    state().respond_connection_offer(r).await;
    log::info!("🟦 Responded to connection request.");
}
#[derive(Debug, Clone)]
pub enum AddRemoteDeviceEvent {
    WaitingForResponse,
    Connected,
    Rejected,
    ConnectionFailed,
    Error(String),
}

pub async fn pause_resume_transfer(transfer_id: String) {
    // let transfer_id = Uuid::from_str(&transfer_id).unwrap();
    //
    // if let Some(meta) = state().transfers_pool.get_transfer_meta(transfer_id) {
    //     if meta.state == TransferState::Suspended {
    //         log::info!("Transfer was suspended. PROCESSING.");
    //         let _ = state()
    //             .transfers_pool
    //             .pause_resume_transfer(transfer_id, false, true)
    //             .await;
    //     } else if meta.state == TransferState::Processing {
    //         log::info!("Transfer was processing. SUSPENDING.");
    //
    //         let _ = state()
    //             .transfers_pool
    //             .pause_resume_transfer(transfer_id, true, true)
    //             .await;
    //     }
    // };
}

/// Called after obtaining remote device endpoint id
/// Will attempt to connect to remote device
pub async fn add_remote_device(sink: StreamSink<AddRemoteDeviceEvent>) {
    log::info!("🟦 Adding remote device.");

    let state = state();
    let e = engine();
    let guard = e.lookup_addr_placeholder.lock().await;

    let Some(endpoint_addr) = guard.clone() else {
        let _ = sink.add_error("Failed".to_string());
        log::error!("🟥 Endpoint Addr placeholder was empty.");
        return;
    };

    // let connection_handle = flutter_rust_bridge::spawn(connect(state, endpoint_addr));
    //
    // match connection_handle.await {
    //     Ok(Ok(())) => {
    //         log::info!("🟩 Connection successful.");
    //         let _ = sink.add(AddRemoteDeviceEvent::Connected);
    //     }
    //     Ok(Err(e)) => {
    //         log::error!("🟥 Connection failed");
    //         let err = match e {
    //             ConnectError::Connection => AddRemoteDeviceEvent::ConnectionFailed,
    //             ConnectError::UserRejected => AddRemoteDeviceEvent::Rejected,
    //             _ => AddRemoteDeviceEvent::ConnectionFailed,
    //         };
    //
    //         // NectanProtocolError::AlreadyConnecting { .. }
    //         // | NectanProtocolError::AlreadyConnected { .. } => AddRemoteDeviceEvent::Connected,
    //         // NectanProtocolError::UserRejected { .. } => AddRemoteDeviceEvent::Rejected,
    //         // other => {
    //         //     log::error!("Unhandled error {}", other);
    //         //     AddRemoteDeviceEvent::Error(other.to_string())
    //         // }
    //         let _ = sink.add(err);
    //     }
    //     Err(e) => {
    //         log::error!("🟥 Add remote device error. Join error. {}", e);
    //         let _ = sink.add_error("Connection error. Please try again.".to_string());
    //     }
    // }
}

pub enum DeviceStatus {
    Online,
    LocalNetwork,
    Offline,
}

pub struct DeviceDto {
    pub id: String, // base64 encoded
    pub username: String,
    pub status: DeviceStatus,
    pub deleted: bool,
}

/// flutter_rust_bridge:ignore
fn update_devices_sink(sink: &StreamSink<Vec<DeviceDto>>) {
    let _ = sink.add(get_dto_devices());
}

// pub fn get_local_devices_list() -> Vec<String> {
//     // state()
//     //     .devices_pool
//     //     .get_all_local()
//     //     .iter()
//     //     .map(|d| device_id_to_base64(&d.id))
//     //     .collect()
//     Vec::new()
// }

pub fn get_dto_devices() -> Vec<DeviceDto> {
    state()
        .devices
        .get_all()
        .into_iter()
        .filter(|d| !d.deleted)
        .map(DeviceDto::from)
        .collect()
}

impl From<Device> for DeviceDto {
    fn from(d: Device) -> DeviceDto {
        let status = match d.status {
            devices::DeviceStatus::Online => DeviceStatus::Online,
            devices::DeviceStatus::Nearby => DeviceStatus::LocalNetwork,
            devices::DeviceStatus::Offline => DeviceStatus::Offline,
        };
        DeviceDto {
            id: device_id_to_base64(&d.id),
            status,
            username: d.username.as_string(),
            deleted: d.deleted,
        }
    }
}

pub async fn delete_device(device_id: String) -> Result<(), String> {
    // let s = state();
    // if let Ok(id) = device_id_from_base64(&device_id) {
    //     s.devices_pool.remove(&id).map_err(|e| e.to_string())?;
    //     log::info!("🟦 Device delted.");
    //     Ok(())
    // } else {
    //     log::info!("🟥 Delete device. Invalid device id.");
    //     Err("Delete device. Invalid id.".to_string())
    // }
    Ok(())
}

/// flutter_rust_bridge:ignore
#[derive(Debug, Default)]
struct FileSize();

impl ClientState for FileSize {
    type ReadDirState = ();
    type DirEntryState = u64;
}

pub async fn cancel_walk() {
    if let Some(walker) = &*engine().walker.lock().await {
        walker.stop()
    }
}

pub async fn delete_transfer(transfer_id: String) {
    let transfer_id = Uuid::parse_str(&transfer_id).unwrap();

    // let _ = state().transfers_pool.delete_transfer(transfer_id).await;
}

pub async fn init_transfer() -> String {
    Uuid::new_v4().to_string()
}

pub async fn load_transfer_items_list(
    device_id: String,
    transfer_id: String,
    start: u64,
    end: u64,
) -> Vec<TransferItemDto> {
    Vec::new()
    // let transfer_id = Uuid::parse_str(&transfer_id).unwrap();
    // let device_id = device_id_from_base64(&device_id).unwrap();
    //
    // let Some(d) = state().devices_pool.get(&device_id) else {
    //     return Vec::new();
    // };
    // let Some(conn) = d.connection.map(|c| c.inner) else {
    //     return Vec::new();
    // };
    // let Ok((mut tx, mut rx)) = conn.open_bi().await else {
    //     return Vec::new();
    // };
    //
    // if write_message(
    //     &mut tx,
    //     &MessagePayload::GetItemsList {
    //         transfer_id,
    //         end,
    //         start,
    //     },
    // )
    // .await
    // .is_err()
    // {
    //     return Vec::new();
    // }
    //
    // let Ok(res) = read_message(&mut rx).await else {
    //     return Vec::new();
    // };
    // let MessagePayload::ItemsList { items } = res else {
    //     return Vec::new();
    // };
    //
    // log::info!("Received list");
    // items
    //     .iter()
    //     .map(|i| TransferItemDto {
    //         id: i.id,
    //         name: Path::new(&i.path)
    //             .file_name()
    //             .unwrap_or_default()
    //             .to_string_lossy()
    //             .to_string(),
    //         path: "".to_string(),
    //         size: i.file_size,
    //         sent_bytes: i.sent_bytes,
    //         error: i.err.map(SendItemErrorDto::from),
    //     })
    //     .collect()
}

pub struct WalkInfo {
    pub total_size: u64,
    pub files: u32,
    pub state: SendState,
}
pub enum SendState {
    Scanning,
    Sending,
    Accepted,
    Failed,
    Rejected,
}
pub async fn send(
    remote_device_id: String,
    transfer_name: String,
    paths: Vec<String>,
    sink: StreamSink<WalkInfo>,
) {
    let emit = |total_size: u64, files: u32, state: SendState| {
        let _ = sink.add(WalkInfo {
            total_size,
            files,
            state,
        });
    };

    let Ok(target) = device_id_from_base64(&remote_device_id) else {
        emit(0, 0, SendState::Failed);
        return;
    };

    let paths = paths.iter().map(PathBuf::from).collect();
    let walker = Walker::new(paths);
    let handle = walker.walk();

    while !walker.finished() && !walker.should_abort() {
        emit(
            walker.total_size(),
            walker.total_entries(),
            SendState::Scanning,
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let _ = handle.join();

    let Some(entries) = walker.take_entries() else {
        emit(
            walker.total_size(),
            walker.total_entries(),
            SendState::Failed,
        );
        return;
    };

    let entries_num = walker.total_entries();
    let total_size = walker.total_size();

    emit(total_size, entries_num, SendState::Sending);

    let offer = TransferOffer {
        transfer_id: Uuid::new_v4(),
        transfer_name,
        entries_num,
        total_size,
        entries: Arc::new(entries),
    };

    let final_state = match send_contents(&state(), target, offer, walker.root_path.clone()).await {
        Ok(_) => SendState::Accepted,
        Err(e) if e == TransferOfferError::TransferRejected => SendState::Rejected,
        Err(_) => SendState::Failed,
    };
    emit(total_size, entries_num, final_state);
}

// pub async fn send(
//     remote_device_id: String,
//     transfer_name: String,
//     paths: Vec<String>,
//     sink: StreamSink<WalkInfo>,
// ) {
//     let transfer_id = Uuid::new_v4();
//     let target = device_id_from_base64(&remote_device_id).unwrap();
//     let paths = paths.iter().map(|p| PathBuf::from(p)).collect();
//     let walker = Walker::new(paths);
//
//     walker.walk().join().unwrap();
//
//     loop {
//         if walker.finished() || walker.should_abort() {
//             break;
//         }
//
//         let total_size = walker.total_size();
//         let files = walker.total_entries();
//
//         let info = WalkInfo {
//             total_size,
//             files,
//             state: SendState::Scanning,
//         };
//         let _ = sink.add(info);
//     }
//
//     let Some(entries) = walker.take_entries() else {
//         return;
//     };
//
//     let entries_num = walker.total_entries();
//     let total_size = walker.total_size();
//     let entries = Arc::new(entries);
//
//     let offer = TransferOffer {
//         transfer_id,
//         transfer_name,
//         entries_num,
//         total_size,
//         entries,
//     };
//
//     let _ = send_contents(&state(), target, offer, walker.root_path.clone()).await;
// }

pub fn gen_transfer_name() -> String {
    nectan_core::common::gen_transfer_name()
}

pub async fn wipe_transfers() {
    // if state().transfers_pool.wipe().await.is_ok() {
    //     log::info!("🟧 Cleared transfers.");
    // } else {
    //     log::info!("🟧 Failed to clear transfers.");
    // };
    todo!()
}

pub fn get_ticket() -> Option<String> {
    state().endpoint_ticket()
}
