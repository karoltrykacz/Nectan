use anyhow::Result;
use anyhow::bail;
use fixedbitset::FixedBitSet;
use futures::FutureExt;
use futures::Stream;
use iroh::endpoint::Connection;
use redb::ReadableTable;
use redb::TableDefinition;
use redb::TypeName;
use redb::Value;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::fmt::Pointer;
use std::fmt::format;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::Relaxed;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncSeekExt;
use tokio::io::AsyncWriteExt;
use tokio::spawn;
use tokio::stream;
use tokio::sync::Notify;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::RwLock;
use tokio::sync::Semaphore;
use tokio::sync::SemaphorePermit;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio_util::sync::WaitForCancellationFuture;
use tracing::error;
use tracing::info;
use tracing::instrument::WithSubscriber;
use tracing::warn;
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
    id: u32,
    file_size: u64,
    sent_bytes: u64,
    is_file: bool,
    path: PathBuf,
}

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

pub struct PendingTransfer {
    id: Uuid,
    peer: DeviceId,
    direction: TransferDirection,
    items_queue: Mutex<FixedBitSet>,
    total_sent: AtomicU64,
    failed: AtomicU32,
    processed: AtomicU32,
    db: redb::Database,
    /// When sending, common parent of all items
    /// When receiving, target directory
    root_path: PathBuf,
    cancel_token: CancellationToken,
    event_tx: tokio::sync::mpsc::Sender<AppEvent>,
    sem: Arc<Semaphore>,
}

/// from_offer -> normal way of constructing
/// from saved -> recover transfer

impl PendingTransfer {
    pub fn from_offer(
        event_tx: tokio::sync::mpsc::Sender<AppEvent>,
        peer: DeviceId,
        direction: TransferDirection,
        offer: TransferOffer<PathTree>,
    ) -> Self {
        let id = offer.transfer_id;
        let root_path = offer.tree.common_parent();

        let db_dir = dirs::data_dir().unwrap_or_else(|| dirs::runtime_dir().unwrap());
        let db_dir = db_dir.join(format!("t-{id}"));
        let db = redb::Database::create(db_dir).expect("Failed to create database");

        let txn = db.begin_write().unwrap();
        {
            let mut table = txn.open_table(DUMB_ITEMS).unwrap();
            for (path, is_file, file_size, id) in &*offer.tree {
                let item = TransferItem {
                    path,
                    is_file,
                    id,
                    sent_bytes: 0,
                    size: file_size,
                    err: None,
                };
                table.insert(id, item);
            }
        }
        let _ = txn.commit();

        PendingTransfer {
            id,
            direction,
            failed: AtomicU32::new(0),
            processed: AtomicU32::new(0),
            total_sent: AtomicU64::new(0),
            items_queue: Mutex::new(FixedBitSet::with_capacity(offer.entries_num as usize)),
            db,
            root_path,
            cancel_token: CancellationToken::new(),
            event_tx,
            peer,
            sem: Arc::new(Semaphore::new(4)),
        }
    }
    pub fn cancelled(&self) -> WaitForCancellationFuture<'_> {
        self.cancel_token.cancelled()
    }
    pub fn cancel(&self) {
        self.cancel_token.cancel()
    }
    // pub fn new(
    //     id: Uuid,
    //     root_path: PathBuf,
    //     total_items: u32,
    //     direction: TransferDirection,
    // ) -> Self {
    //     let db_dir = dirs::data_dir().unwrap_or_else(|| dirs::runtime_dir().unwrap());
    //     let db_dir = db_dir.join(format!("t-{id}"));
    //     let db = redb::Database::create(db_dir).expect("Failed to create database");
    //
    //     PendingTransfer {
    //         id,
    //         direction,
    //         failed: AtomicU32::new(0)),
    //         total_sent: Arc::new(AtomicU64::new(0)),
    //         items_queue: Arc::new(Mutex::new(FixedBitSet::with_capacity(total_items as usize))),
    //         db: Arc::new(db),
    //         root_path,
    //     }
    // }
    /// Returns remaining items
    fn item_finished(&self, item_id: u32) -> u32 {
        self.items_queue.lock().unwrap().set(item_id as usize, true);
        self.processed.fetch_add(1, Relaxed) + 1
    }
    fn next_unsent_item(&self) -> Option<TransferItem> {
        let idx = {
            let lock = self.items_queue.lock().unwrap();
            lock.zeroes().next()?
        };
        self.get_item(idx as u32)
    }
    fn get_item(&self, id: u32) -> Option<TransferItem> {
        let txn = self.db.begin_read().ok()?;
        txn.open_table(DUMB_ITEMS)
            .ok()?
            .get(id)
            .ok()?
            .map(|i| i.value())
    }

    pub async fn process_stream(
        self: Arc<Self>,
        permit: OwnedSemaphorePermit,
        mut stream: StreamPair,
    ) -> Result<()> {
        match self.direction {
            TransferDirection::Outcoming => {
                let Some(mut item) = self.next_unsent_item() else {
                    // All transfers processed, wait until sem is free and exit
                    info!("All transfers drained. Waiting for last items.");
                    return Ok(());
                };

                let s = self.clone();

                spawn(async move {
                    match s.send_item(&mut item, stream, permit).await {
                        Ok(()) => s.item_finished(item.id),
                        Err(e) => s.handle_item_err(item.id, e),
                    }
                });
            }
            TransferDirection::Incoming => {
                let header: TransferItemHeader = stream.read().await?;
                let id = header.id;

                spawn(async move {
                    let r = match self.recieve_item(header, stream, permit).await {
                        Ok(()) => self.item_finished(id),
                        Err(e) => self.handle_item_err(id, e),
                    };
                    if r == 0 {
                        // TODO
                        info!("No more items to receive.");
                        self.cancel();
                    }
                });
            }
        }

        Ok(())
    }

    // pub async fn start_receiving(self: Arc<Self>) -> Result<()> {
    //     loop {
    //         let s = self.clone();
    //
    //         // mut pair_rx: tokio::sync::mpsc::Receiver<StreamPair>,
    //
    //         tokio::select! {
    //             _ = self.cancelled() => {
    //                 warn!("Receiving transfer [{}] cancelled.", &self.id.to_string()[0..8]);
    //             }
    //             stream = pair_rx.recv() => {
    //                 // Tx will never drop
    //                 let mut stream = stream.unwrap();
    //
    //             }
    //         }
    //     }
    // }
    // async fn start_sending(self: Arc<Self>, conn: Connection) -> Result<()> {
    //     // Limit max concurrent items
    //     let sem = Arc::new(Semaphore::new(4));
    //
    //     loop {
    //         let Some(mut item) = self.next_unsent_item() else {
    //             // All transfers processed, wait until sem is free and exit
    //             info!("All transfers drained. Waiting for last items.");
    //             let _ = sem.acquire_many(4).await;
    //             return Ok(());
    //         };
    //
    //         let stream = StreamPair::open(&conn).await?;
    //         let permit = sem.clone().acquire_owned().await.unwrap();
    //         let s = self.clone();
    //
    //         spawn(async move {
    //             match s.send_item(&mut item, stream, permit).await {
    //                 Ok(()) => s.item_finished(item.id),
    //                 Err(e) => s.handle_item_err(item.id, e),
    //             }
    //         });
    //     }
    // }

    async fn recieve_item(
        &self,
        header: TransferItemHeader,
        mut stream: StreamPair,
        permit: OwnedSemaphorePermit,
    ) -> Result<(), TransferItemError> {
        let file_size = header.file_size;
        let output_dir = Path::new(&self.root_path).join(&header.path);

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
            tokio::select! {
                _ = self.cancelled() =>{
                    warn!("Receiving [{}] cancelled.", header.path.display());
                    return Err(TransferItemError::Terminated);
                }
                r = stream.rx().read_chunk(CHUNK_SIZE) => {
                    let chunk = r.map_err(|_| TransferItemError::StreamError)?;
                    let Some(chunk) = chunk else{
                        if total_written < file_size {
                            // Stream ended prematurely
                            out_file
                                .flush()
                                .await
                                .map_err(|_| TransferItemError::FileIOError)?;
                            warn!("RxStream ended prematurely [{}]", header.path.display());
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
            }
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
                id: item.id,
                file_size: item.size,
                sent_bytes: item.sent_bytes,
                is_file: item.is_file,
                path: item.path.clone(),
            })
            .await
            .map_err(|_| TransferItemError::StreamError)?;

        let mut buf = Vec::with_capacity(64 * 1024);

        loop {
            tokio::select! {
                _ = self.cancelled() => {
                    warn!("Sending item [{}] terminated.", item.path.display());
                    return Err(TransferItemError::Terminated);
                }
                result = file.read_buf(&mut buf)=>{
                    let n = result.map_err(|_| TransferItemError::FileIOError)?;
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
            };
        }

        Ok(())
    }

    /// Return remaining items
    fn handle_item_err(&self, id: u32, e: TransferItemError) -> u32 {
        match e {
            // Unrecoverable errors
            TransferItemError::FileOpenPermissionDenied
            | TransferItemError::OpenFail
            | TransferItemError::FileAlreadyExisted
            | TransferItemError::FileNotFound
            | TransferItemError::FileIOError => {
                self.failed.fetch_add(1, Relaxed);
                let _ = self.set_item_error(id, e);
                self.item_finished(id);
                self.processed.fetch_add(1, Relaxed) + 1
            }
            // Recoverable (Network) errors - nop
            TransferItemError::StreamError | TransferItemError::Terminated => {
                self.processed.load(Relaxed)
            }
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

// How to display list of all transfers???

const TRANSFERS: TableDefinition<u128, &[u8]> = TableDefinition::new("transfers");

// #[derive(Clone)]
pub struct Transfers {
    pub pending_transfers: std::sync::RwLock<HashMap<Uuid, Arc<PendingTransfer>>>,
    devices: Devices,
    event_tx: tokio::sync::mpsc::Sender<AppEvent>,
    db: redb::Database,
    // notify: Notify,
}

impl Transfers {
    pub fn new(devices: Devices, event_tx: tokio::sync::mpsc::Sender<AppEvent>) -> Self {
        let db_dir = dirs::data_dir().unwrap_or_else(|| dirs::runtime_dir().unwrap());
        let db_dir = db_dir.join("transfers");
        let db = redb::Database::create(db_dir).expect("Failed to create transfers database");
        let pending_transfers = hotpath::rw_lock!(
            std::sync::RwLock::new(HashMap::new()),
            label = "pending_transfers"
        );

        Transfers {
            event_tx,
            db,
            pending_transfers,
            notify: Notify::new(),
            devices,
        }
    }

    async fn run(self: Arc<Self>) {
        loop {
            self.notify.notified().await;
        }
    }

    pub fn process_offer(
        &self,
        peer: DeviceId,
        direction: TransferDirection,
        offer: TransferOffer<PathTree>,
    ) {
        let id = offer.transfer_id;
        let t = Arc::new(PendingTransfer::from_offer(
            self.event_tx.clone(),
            peer,
            direction,
            offer,
        ));
        self.pending_transfers.write().unwrap().insert(id, t);
    }

    async fn start_sending(&self, t: Arc<PendingTransfer>) -> Result<()> {
        let Some(conn) = self.devices.get_connection(t.peer) else {
            bail!("Connection not found")
        };
        loop {
            let permit = t.sem.clone().acquire_owned().await.unwrap();
            let stream = StreamPair::open(&conn).await?;
            t.clone().process_stream(permit, stream).await;
        }
    }

    pub async fn forward_incoming_stream(&self, transfer_id: Uuid, stream: StreamPair) {
        let Some(t) = self
            .pending_transfers
            .read()
            .unwrap()
            .get(&transfer_id)
            .cloned()
        else {
            return;
        };
        let permit = t.sem.clone().acquire_owned().await.unwrap();
        t.process_stream(permit, stream).await;
    }
}

pub enum TransferCommand {
    GetFile,
}
