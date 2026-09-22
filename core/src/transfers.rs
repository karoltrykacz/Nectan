use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::fmt::format;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::Relaxed;

use anyhow::Result;
use anyhow::bail;
use fixedbitset::FixedBitSet;
use futures::Stream;
use iroh::endpoint::Connection;
use redb::ReadableTable;
use redb::TableDefinition;
use redb::TypeName;
use redb::Value;
use serde::Deserialize;
use serde::Serialize;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncSeekExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::Notify;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tokio::sync::SemaphorePermit;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::error;
use tracing::info;
use uuid::Uuid;

use crate::devices::DeviceId;
use crate::devices::Devices;
use crate::devices::Username;
use crate::messages::AppEvent;
use crate::messages::NetMessage;
use crate::messages::StreamableMessage;
use crate::path_tree::CompressedPathTree;
use crate::path_tree::PathTree;
use crate::protocol::NectanState;
use crate::protocol::TransferOffer;
use crate::stream::StreamPair;

#[derive(Debug, Serialize, Deserialize)]
struct TransferItemHeader {
    file_size: u64,
    sent_bytes: u64,
    is_file: bool,
    path: PathBuf,
}

// impl<'a> TransferItemHeader<'a> {
//     pub const MAX_PATH_LEN: usize = 32_767;
//     pub const FIXED_HEADER_SIZE: usize = 19;
//
//     pub fn to_bytes(&self) -> Vec<u8> {
//         let total_len = 8 + 8 + 1 + 2 + self.path_bytes.len();
//         let mut bytes = Vec::with_capacity(total_len);
//
//         bytes.extend_from_slice(&self.file_size.to_be_bytes());
//         bytes.extend_from_slice(&self.sent_bytes.to_be_bytes());
//         bytes.push(self.is_file as u8);
//         bytes.extend_from_slice(&self.path_bytes_len.to_be_bytes());
//         bytes.extend_from_slice(self.path_bytes);
//
//         bytes
//     }
//
//     pub fn from_bytes(mut bytes: &'a [u8]) -> anyhow::Result<Self> {
//         if bytes.len() < Self::FIXED_HEADER_SIZE {
//             bail!("Header too short.")
//         }
//         let file_size = u64::from_be_bytes(bytes[..8].try_into().unwrap());
//         bytes = &bytes[8..];
//
//         let sent_bytes = u64::from_be_bytes(bytes[..8].try_into().unwrap());
//         bytes = &bytes[8..];
//
//         let is_file = bytes[0] != 0;
//         bytes = &bytes[1..];
//
//         let path_bytes_len = u16::from_be_bytes(bytes[..2].try_into().unwrap());
//         bytes = &bytes[2..];
//
//         let path_len = path_bytes_len as usize;
//
//         if bytes.len() < path_len {
//             bail!("Invalid path len.");
//         } else if bytes.len() > path_len {
//             bail!("Input buffer contains trailing unparsed bytes.");
//         }
//
//         let path_bytes = &bytes[..path_len];
//         Ok(Self {
//             file_size,
//             sent_bytes,
//             path_bytes_len,
//             is_file,
//             path_bytes,
//         })
//     }
//     pub async fn read_bytes_from_stream<R: AsyncRead + Unpin>(stream: &mut R) -> Result<Vec<u8>> {
//         let mut fixed_buf = [0u8; Self::FIXED_HEADER_SIZE];
//         stream.read_exact(&mut fixed_buf).await?;
//
//         let path_len = u16::from_be_bytes(fixed_buf[17..19].try_into().unwrap()) as usize;
//         if path_len > Self::MAX_PATH_LEN {
//             bail!("Path too large.");
//         }
//
//         let mut full_buf = vec![0u8; Self::FIXED_HEADER_SIZE + path_len];
//         full_buf[..Self::FIXED_HEADER_SIZE].copy_from_slice(&fixed_buf);
//
//         stream
//             .read_exact(&mut full_buf[Self::FIXED_HEADER_SIZE..])
//             .await?;
//
//         Ok(full_buf)
//     }
// }

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TransferItem {
    pub id: u32,
    pub size: u64,
    pub sent_bytes: u64,
    pub is_file: bool,
    /// Relative path (starting from the transfer root path)
    pub path: PathBuf,
    pub err: Option<TransferItemError>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
pub enum TransferItemError {
    FileIOError,
    FileAlreadyExisted,
    FileOpenPermissionDenied,
    FileNotFound,
    StreamError,
    OpenFail,
    Terminated,
}
impl Eq for TransferItemError {}

#[derive(Debug, Serialize)]
#[serde(tag = "kind")]
pub enum TransferOfferError {
    DeviceOffline,
    InvalidDestination,
    TransferRejected,
    UnexpectedResponse,
    ConnectionFailed,
}

impl std::fmt::Display for TransferOfferError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Send transfer offer to remote device
/// If the offer is accepted start processing the transfer
pub async fn send_contents(
    state: &NectanState,
    target: DeviceId,
    offer: TransferOffer<CompressedPathTree>,
) -> Result<(), TransferOfferError> {
    // Get device
    let Some(device) = state.devices.get(&target) else {
        return Err(TransferOfferError::InvalidDestination);
    };

    // Get connection
    let Some(connection) = device.connection.clone() else {
        return Err(TransferOfferError::DeviceOffline);
    };

    // Open stream
    let Ok((mut tx, mut rx)) = connection.open_bi().await else {
        return Err(TransferOfferError::ConnectionFailed);
    };

    // Write offer
    let msg = NetMessage::TransferOfferMsg { offer };
    if msg.write(&mut tx).await.is_err() {
        return Err(TransferOfferError::ConnectionFailed);
    }

    // Propagate app event (changes the send modal state)
    state.emit(AppEvent::TransferOfferDelivered).await;

    // Read remote device response
    let Ok(offer_response) = NetMessage::read_async(&mut rx).await else {
        return Err(TransferOfferError::ConnectionFailed);
    };

    if let NetMessage::Rejected { reason } = &offer_response {
        info!("Outcoming transfer offer rejected. {:?}", reason);
        return Err(TransferOfferError::TransferRejected);
    }

    let NetMessage::Accepted = offer_response else {
        return Err(TransferOfferError::UnexpectedResponse);
    };

    // Transfer accepted
    // Create transfer object

    // // Finally start processing the transfer
    // state
    //     .transfers_pool
    //     .start_processing_transfer(transfer_id, TransferDirection::Outcoming, destination)
    //     .await;
    // state.transfers_pool.notify_workers();

    Ok(())
}

#[derive(Clone, Copy)]
enum TransferDirection {
    Outcoming,
    Incoming,
}

const DUMB_ITEMS: TableDefinition<u32, TransferItem> = TableDefinition::new("items");

// #[derive(Clone)]
pub struct PendingTransfer {
    id: Uuid,
    // remote_device: DeviceId,
    direction: TransferDirection,
    items_queue: Arc<Mutex<FixedBitSet>>,
    total_sent: Arc<AtomicU64>,
    failed: Arc<AtomicU32>,
    db: Arc<redb::Database>,
    /// When sending, common parent of all items
    /// When receiving, target directory
    root_path: PathBuf,
}

impl PendingTransfer {
    pub fn from_offer(direction: TransferDirection, offer: TransferOffer<PathTree>) -> Self {
        let id = offer.transfer_id;
        let root_path = offer.tree.common_parent();

        let db_dir = dirs::data_dir().unwrap_or_else(|| dirs::runtime_dir().unwrap());
        let db_dir = db_dir.join(format!("t-{id}"));
        let db = redb::Database::create(db_dir).expect("Failed to create database");

        let txn = db.begin_write().unwrap();
        let mut counter = 0u32;
        {
            let mut table = txn.open_table(DUMB_ITEMS).unwrap();
            for (path, is_file, file_size) in &*offer.tree {
                let item = TransferItem {
                    path,
                    is_file,
                    id: counter,
                    sent_bytes: 0,
                    size: file_size,
                    err: None,
                };
                table.insert(counter, item);
                counter += 1;
            }
        }

        PendingTransfer {
            id,
            direction,
            failed: Arc::new(AtomicU32::new(0)),
            total_sent: Arc::new(AtomicU64::new(0)),
            items_queue: Arc::new(Mutex::new(FixedBitSet::with_capacity(
                offer.entries_num as usize,
            ))),
            db: Arc::new(db),
            root_path,
        }
    }
    pub fn new(
        id: Uuid,
        root_path: PathBuf,
        total_items: u32,
        direction: TransferDirection,
    ) -> Self {
        let db_dir = dirs::data_dir().unwrap_or_else(|| dirs::runtime_dir().unwrap());
        let db_dir = db_dir.join(format!("t-{id}"));
        let db = redb::Database::create(db_dir).expect("Failed to create database");

        PendingTransfer {
            id,
            direction,
            failed: Arc::new(AtomicU32::new(0)),
            total_sent: Arc::new(AtomicU64::new(0)),
            items_queue: Arc::new(Mutex::new(FixedBitSet::with_capacity(total_items as usize))),
            db: Arc::new(db),
            root_path,
        }
    }
    fn item_finished(&self, item_id: u32) {
        self.items_queue.lock().unwrap().set(item_id as usize, true);
    }
    fn next_item(&self) -> Option<TransferItem> {
        // let item = self.items_queue.
        // let item = self.items_queue.lock().unwrap().iter().next().cloned();
        None
    }
    pub async fn start_sending(
        self: Arc<Self>,
        event_tx: tokio::sync::mpsc::Sender<AppEvent>,
        conn: Connection,
    ) -> Result<()> {
        // Limit max concurrent items
        let sem = Arc::new(Semaphore::new(4));

        loop {
            let Some(mut item) = self.next_item() else {
                // All transfers processed, wait until sem is free and exit
                let _ = sem.acquire_many(4).await;
                info!("Transfer finished.");
                return Ok(());
            };

            let stream = StreamPair::open(&conn).await?;
            let permit = sem.clone().acquire_owned().await.unwrap();
            let s = self.clone();

            tokio::spawn(async move {
                match s.send_item(&mut item, stream, permit).await {
                    Ok(()) => s.item_finished(item.id),
                    Err(e) => s.handle_item_err(item, e),
                }
            });
        }
    }

    async fn recieve_item(&self, mut stream: StreamPair) -> Result<(), TransferItemError> {
        let header: TransferItemHeader = stream
            .read()
            .await
            .map_err(|_| TransferItemError::StreamError)?;

        let file_size = header.file_size;
        let output_dir = Path::new(&self.root_path).join(header.path);

        if std::fs::exists(&output_dir).map_err(|_| TransferItemError::OpenFail)? {
            return Err(TransferItemError::FileAlreadyExisted);
        }

        let full_path = PathBuf::from(format!("{}.NectanLock", output_dir.display()));

        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|_| TransferItemError::FileIOError)?;
        }

        let mut out_file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&full_path)
            .await
            .map_err(|_| TransferItemError::FileIOError)?;

        out_file
            .seek(tokio::io::SeekFrom::Start(header.sent_bytes))
            .await
            .map_err(|_| TransferItemError::FileIOError)?;

        const CHUNK_SIZE: usize = 128 * 1024;
        let mut total_written = header.sent_bytes;

        loop {
            let Some(chunk) = stream
                .rx()
                .read_chunk(CHUNK_SIZE)
                .await
                .map_err(|_| TransferItemError::StreamError)?
            else {
                if total_written < file_size {
                    // Stream end too soon
                    out_file
                        .flush()
                        .await
                        .map_err(|_| TransferItemError::FileIOError)?;
                    return Err(TransferItemError::StreamError);
                }
                break;
            };

            out_file
                .write_all(&chunk)
                .await
                .map_err(|_| TransferItemError::FileIOError)?;

            let len = chunk.len() as u64;
            total_written += len;
        }

        out_file
            .flush()
            .await
            .map_err(|_| TransferItemError::StreamError)?;

        stream
            .tx()
            .write_all(&total_written.to_be_bytes())
            .await
            .map_err(|_| TransferItemError::StreamError)?;

        let final_destination = {
            let s = full_path.to_string_lossy();
            let stripped = s.strip_suffix(".NectanLock").unwrap_or(&s);
            PathBuf::from(stripped)
        };

        std::fs::rename(&full_path, &final_destination)
            .map_err(|_| TransferItemError::FileIOError)?;

        Ok(())
    }
    pub async fn start_receiving(
        self: Arc<Self>,
        event_tx: tokio::sync::mpsc::Sender<AppEvent>,
        conn: Connection,
    ) -> Result<()> {
        // Limit max concurrent items
        let sem = Arc::new(Semaphore::new(4));

        loop {
            let Some(mut item) = self.next_item() else {
                // All transfers processed, wait until sem is free and exit
                let _ = sem.acquire_many(4).await;
                info!("Transfer finished.");
                return Ok(());
            };

            let stream = StreamPair::open(&conn).await?;
            let permit = sem.clone().acquire_owned().await.unwrap();
            let s = self.clone();

            tokio::spawn(async move {
                match s.send_item(&mut item, stream, permit).await {
                    Ok(()) => s.item_finished(item.id),
                    Err(e) => s.handle_item_err(item, e),
                }
            });
        }
    }

    async fn send_item(
        &self,
        item: &mut TransferItem,
        mut stream: StreamPair,
        _permit: OwnedSemaphorePermit,
    ) -> Result<(), TransferItemError> {
        let full_path = Path::new(&self.root_path).join(&item.path);

        info!("Sending {full_path:?}");

        let mut file = match tokio::fs::File::open(&full_path).await {
            Ok(file) => file,
            Err(e) => match e.kind() {
                ErrorKind::PermissionDenied => {
                    error!("Failed to open file {full_path:?}");
                    return Err(TransferItemError::FileOpenPermissionDenied);
                }
                ErrorKind::NotFound => {
                    error!("File not found {full_path:?}");
                    return Err(TransferItemError::FileNotFound);
                }
                _ => {
                    error!("Unhandled send item error - {e}");
                    return Err(TransferItemError::OpenFail);
                }
            },
        };
        stream
            .write(&TransferItemHeader {
                file_size: item.size,
                sent_bytes: item.sent_bytes,
                is_file: item.is_file,
                path: item.path.clone(),
            })
            .await
            .map_err(|_| TransferItemError::StreamError)?;

        let mut buf = Vec::with_capacity(64 * 1024);

        loop {
            let n = file
                .read_buf(&mut buf)
                .await
                .map_err(|_| TransferItemError::FileIOError)?;

            if n == 0 {
                break;
            }

            item.sent_bytes += n as u64;
            let chunk = std::mem::replace(&mut buf, Vec::with_capacity(64 * 1024));
            stream
                .tx()
                .write_chunk(chunk.into())
                .await
                .map_err(|_| TransferItemError::StreamError)?;
        }

        Ok(())
    }

    pub fn handle_item_err(&self, item: TransferItem, e: TransferItemError) {
        self.failed.fetch_add(1, Relaxed);
        match e {
            // Unrecoverable errors
            TransferItemError::FileOpenPermissionDenied
            | TransferItemError::OpenFail
            | TransferItemError::FileAlreadyExisted
            | TransferItemError::FileNotFound
            | TransferItemError::FileIOError => {
                self.failed.fetch_add(1, Relaxed);
                let _ = self.set_item_error(item.id, e);
                self.item_finished(item.id);
            }
            // Recoverable (Network) errors - nop
            TransferItemError::StreamError | TransferItemError::Terminated => {}
        }
    }
    pub fn set_item_progress(&self, item_id: u32, sent_bytes: u64) -> Result<(), redb::Error> {
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(DUMB_ITEMS)?;
            let Some(mut meta) = table.get(item_id)?.map(|g| g.value()) else {
                error!("Item not found.");
                return Ok(());
            };
            meta.sent_bytes = sent_bytes;
            table.insert(item_id, &meta)?;
        }
        txn.commit()?;
        Ok(())
    }
    pub fn set_item_error(&self, item_id: u32, e: TransferItemError) -> Result<(), redb::Error> {
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(DUMB_ITEMS)?;
            let Some(mut meta) = table.get(item_id)?.map(|g| g.value()) else {
                error!("Item not found.");
                return Ok(());
            };
            meta.err = Some(e);
            table.insert(item_id, &meta)?;
        }
        txn.commit()?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct PendingTransfers {
    inner: Arc<std::sync::RwLock<HashMap<Uuid, PendingTransfer>>>,
    // /// Master token for all pending transfers
    // /// Used to restart/cancel workers upon connection change
    // cancel_token: CancellationToken,
}

impl PendingTransfers {
    pub fn new() -> Self {
        PendingTransfers {
            inner: Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }
    /// Manages starting, pausing, resuming and deleting the transfers
    // pub async fn start(&self) {
    // General notification -> controller decides on its own what to do
    // Called on:
    // Conection changed -
    // Add new transfer - easy
    // Seems fucking redundant / ugly
    // 3
    // }

    /// Restarts pending transfers when the connection is replaced ()
    pub fn connection_replaced() {}

    // transfer offer?
    pub fn process_offer(&self, direction: TransferDirection, offer: TransferOffer<PathTree>) {
        self.inner.write().unwrap().insert(
            offer.transfer_id,
            PendingTransfer::from_offer(direction, offer),
        );
    }

    // pub fn get(&self, id: Uuid) -> Option<PendingTransfer> {
    //     self.inner.read().unwrap().get(&id).cloned()
    // }
    // pub fn handle_stream(&self, id: Uuid, stream: StreamPair) {
    //     let lock = self.inner.read().unwrap();
    //     if let Some(p) = lock.get(&id) {
    //         // p.receive_item();
    //     }
    // }
    /// The pending streams should not care about the connetion shit
    /// But they should be aware of it ???
    /// Send items
    /// I dont want fucking state of processing transfers
    /// How to make it work with dangling connections?
    pub fn start_processing(&self) {
        // Needs access to the stream
        // Needs connection
        // Gate on the channel (devices pool lets the )
        // let Some(connection) = self.devices_pool.get_connection(transfer_destination) else {};
        // Send item fuction will fail on connection dead (good)
        // Then it will be called to different connection and resumed in previous place (thanks to acks)
        //
    }
}

// PROTOCOL SPLIT???

/*
 * When connection changes i can just replace it and immediately start routing to it
 * Or just in the devices
 * struct Connections{
 *
 * }
 *
 * impl Connections{
 *
 * }
 *
 *
 *
 */

pub enum TransferCommand {
    GetFile,
}

#[derive(Clone)]
pub struct Transfers {
    pub pending: PendingTransfers,
    // pub event_tx: tokio::sync::broadcast::Sender<Message>,
    // pub send_events_to_ui: AtomicBool,
    db: Arc<redb::Database>,
    tx: tokio::sync::mpsc::Sender<TransferCommand>,
}

const ITEMS: TableDefinition<(u128, u64), TransferItem> = TableDefinition::new("items");
// [(container_id, item_id), TransferItem]

const CONTAINERS: TableDefinition<u128, &[u8]> = TableDefinition::new("transfers");

impl Transfers {
    pub fn build(db_dir: PathBuf) -> Self {
        let db_dir = Path::new(&db_dir).join("db");
        let db = Arc::new(redb::Database::create(db_dir).expect("Failed to create database."));
        let (tx, rx) = mpsc::channel(32);

        // Load pending transfers from disk!

        Transfers {
            pending: PendingTransfers::new(),
            db,
            tx,
        }
    }

    /// After the transfer offer is created,
    /// Sender creates the pending transfer
    /// (load the queue, and start sending offers)

    pub fn start_processing_transfer() {}
    // Where check the permissions on the container??
    // Should each container have each own controller??
    // Or one controller for every mf???
    // non blocking Requests to the controller
    // pub async fn run_controller(self, mut rx: mpsc::Receiver<TransferCommand>) {
    //     while let Some(cmd) = rx.recv().await {
    //         match cmd {
    //             GetFile => {}
    //         }
    //     }
    // }
}

// some controller that will sync the containers
// list of allitems with progress attached (it db)
// QUEUE!!! (store only ids)

impl Value for TransferItem {
    type SelfType<'a> = TransferItem;
    type AsBytes<'a> = Vec<u8>;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn type_name() -> TypeName {
        TypeName::new("transfer_item")
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        postcard::from_bytes(data).unwrap()
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        postcard::to_allocvec(value).unwrap()
    }
}
