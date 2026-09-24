use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::ensure;
use fixedbitset::FixedBitSet;
use iroh::endpoint::Connection;
use redb::ReadableDatabase;
use redb::ReadableTable;
use redb::TableDefinition;
use redb::TypeName;
use redb::Value;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::Component;
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
use crate::transfers;
use crate::transfers::TransferDirection::Outcoming;

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
    offer: TransferOffer<PathTree>,
) -> Result<(), TransferOfferError> {
    // Get device
    let Some(device) = state.devices.get(&target) else {
        return Err(TransferOfferError::InvalidDestination);
    };

    // Get connection
    let Some(conn) = device.connection.clone() else {
        return Err(TransferOfferError::DeviceOffline);
    };

    // Open stream
    let Ok((mut tx, mut rx)) = conn.open_bi().await else {
        return Err(TransferOfferError::ConnectionFailed);
    };

    // Write compressed offer
    let c_offer = offer.clone().compress().unwrap();
    let msg = NetMessage::TransferOfferMsg { offer: c_offer };
    if msg.write(&mut tx).await.is_err() {
        return Err(TransferOfferError::ConnectionFailed);
    }

    // Propagate app event (changes the send modal state)
    state.emit(AppEvent::TransferOfferDelivered).await;

    // Read remote device response
    let Ok(offer_response) = NetMessage::read_async(&mut rx).await else {
        error!("Failed to read offer response.");
        return Err(TransferOfferError::ConnectionFailed);
    };

    if let NetMessage::Rejected { reason } = &offer_response {
        warn!("Outcoming transfer offer rejected. {:?}", reason);
        return Err(TransferOfferError::TransferRejected);
    }

    let NetMessage::Accepted = offer_response else {
        return Err(TransferOfferError::UnexpectedResponse);
    };
    info!("Transfer accepted.");

    // Transfer accepted, start processing
    state.transfers.new_offer(target, Outcoming, offer).await;

    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TransferDirection {
    Outcoming,
    Incoming,
}

const DUMB_ITEMS: TableDefinition<u32, TransferItem> = TableDefinition::new("items");

pub struct PendingTransfer {
    id: Uuid,
    target: DeviceId,
    direction: TransferDirection,
    items_queue: Mutex<FixedBitSet>,
    failed: AtomicU32,
    total_items: u32,
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
    // fn run_db_writer() {
    // move the db writer to separate thread
    // }
    pub fn from_offer(
        event_tx: tokio::sync::mpsc::Sender<AppEvent>,
        target: DeviceId,
        direction: TransferDirection,
        offer: TransferOffer<PathTree>,
    ) -> Self {
        info!("New transfer from offer.");

        let id = offer.transfer_id;
        let root_path = offer.tree.common_parent();

        let db_dir = dirs::data_dir().unwrap_or_else(|| dirs::runtime_dir().unwrap());
        let dir = match direction {
            TransferDirection::Outcoming => "out",
            TransferDirection::Incoming => "in",
        };
        let db_dir = db_dir.join(format!("Nectan/t-{id}-{}", dir));
        let db = redb::Database::create(db_dir).expect("Failed to create database");

        let mut c = 0u32;
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
                let _ = table.insert(id, item);
                c += 1;
            }
        }
        let _ = txn.commit();

        assert_eq!(
            offer.entries_num, c,
            "Total entries must equal tree elements."
        );
        let total_items = c;

        PendingTransfer {
            id,
            direction,
            failed: AtomicU32::new(0),
            processed: AtomicU32::new(0),
            items_queue: Mutex::new(FixedBitSet::with_capacity(offer.entries_num as usize)),
            db,
            total_items,
            root_path,
            cancel_token: CancellationToken::new(),
            event_tx,
            target,
            sem: Arc::new(Semaphore::new(4)),
        }
    }
    pub fn cancelled(&self) -> WaitForCancellationFuture<'_> {
        self.cancel_token.cancelled()
    }
    pub fn cancel(&self) {
        self.cancel_token.cancel()
    }
    /// Returns remaining items
    fn item_finished(&self) -> u32 {
        self.processed.fetch_add(1, Relaxed) + 1
    }
    fn get_item(&self, id: u32) -> Result<TransferItem, redb::Error> {
        let txn = self.db.begin_read()?;
        Ok(txn
            .open_table(DUMB_ITEMS)?
            .get(id)?
            .expect("Item must be in database.")
            .value())
    }
    fn unsent_batch(&self) -> Vec<usize> {
        let mut queue = self.items_queue.lock().unwrap();
        let batch: Vec<usize> = queue.zeroes().take(10).collect();
        for &i in &batch {
            queue.set(i, true);
        }
        batch
    }
    async fn start_sending(self: Arc<Self>, conn: Connection) -> Result<()> {
        info!(
            "Starting sending [{}] [{} ITEMS]",
            &self.id.to_string()[0..8],
            self.items_queue.lock().unwrap().zeroes().count()
        );

        // Get first batch of items
        let mut queue = Vec::new();
        queue.extend(self.unsent_batch());

        loop {
            let item = match queue.pop() {
                Some(id) => {
                    // TODO graceful err, file not found???
                    self.get_item(id as u32).expect("Transfer database error.")
                }
                None => {
                    let drained = self.processed.load(Relaxed) == self.total_items;
                    if drained {
                        // All transfers processed, wait until sem is free and exit
                        info!("All transfers drained. Waiting for last items to finish.");
                        let _ = self.sem.acquire_many(4).await;
                        return Ok(());
                    } else {
                        queue.extend(self.unsent_batch());
                        continue;
                    }
                }
            };

            let stream = StreamPair::open(&conn).await?;
            let permit = self.sem.clone().acquire_owned().await.unwrap();
            let s = self.clone();

            tokio::spawn(async move {
                let item_id = item.id;
                let _permit = permit;

                match s.send_item(item, stream).await {
                    Ok(()) => {
                        s.item_finished();
                    }
                    Err(e) => {
                        s.handle_item_err(item_id, e);
                        // Transfer failed -> uncheck it
                        unsafe {
                            s.items_queue
                                .lock()
                                .unwrap()
                                .set_unchecked(item_id as usize, false);
                        }
                    }
                }
            });
        }
    }

    pub async fn process_stream(
        self: Arc<Self>,
        permit: OwnedSemaphorePermit,
        mut stream: StreamPair,
    ) -> Result<()> {
        info!("Processing stream for {}", &self.id.to_string()[0..8]);
        ensure!(self.direction == TransferDirection::Incoming);

        info!("Reading header");
        let header: TransferItemHeader = stream.read().await?;
        info!("Read header {:?}", header);
        let id = header.id;

        let r = match self.recieve_item(header, stream, permit).await {
            Ok(()) => self.item_finished(),
            Err(e) => self.handle_item_err(id, e),
        };

        if r == 0 {
            info!("No more items to receive.");
            self.cancel();
        }

        Ok(())
    }

    async fn recieve_item(
        &self,
        header: TransferItemHeader,
        mut stream: StreamPair,
        _permit: OwnedSemaphorePermit,
    ) -> Result<(), TransferItemError> {
        let file_size = header.file_size;

        let item_path = header.path;
        // Check against shit like ../../
        let item_path: PathBuf = item_path
            .components()
            .filter(|c| matches!(c, Component::Normal(_)))
            .collect();
        let output_dir = Path::new("/home/karol/Documents/NectanTests").join(&item_path);

        if !header.is_file {
            // Just create the folder
            if let Some(parent) = output_dir.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|_| TransferItemError::FileIOError)?;
            }
            return Ok(());
        }

        if std::fs::exists(&output_dir).map_err(|_| TransferItemError::OpenFail)? {
            error!("File [{}] already exsited.", output_dir.display());
            return Err(TransferItemError::FileAlreadyExisted);
        }

        let mut lock_path = output_dir.clone();
        lock_path.as_mut_os_string().push(".NectanLock");

        if let Some(parent) = lock_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|_| TransferItemError::FileIOError)?;
        }

        let mut out_file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
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
                    warn!("Receiving [{}] cancelled.", item_path.display());
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
                            warn!("RxStream ended prematurely [{}]", item_path.display());
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

        info!("Flushing {}", item_path.display());
        out_file
            .flush()
            .await
            .map_err(|_| TransferItemError::FileIOError)?;

        info!("Flushing {}", item_path.display());

        std::fs::rename(&lock_path, &output_dir).map_err(|_| TransferItemError::FileIOError)?;

        // Final ack
        // stream
        //     .tx()
        //     .write_all(&total_written.to_be_bytes())
        //     .await
        //     .map_err(|_| TransferItemError::StreamError)?;

        info!("Received {}", item_path.display());
        Ok(())
    }

    async fn send_item(
        &self,
        mut item: TransferItem,
        mut stream: StreamPair,
    ) -> Result<(), TransferItemError> {
        let full_path = Path::new(&self.root_path).join(&item.path);
        info!("Sending {full_path:?}. Item path {}", item.path.display());

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
            .write(&NetMessage::TransferStream {
                transfer_id: self.id,
            })
            .await
            .map_err(|_| TransferItemError::StreamError)?;

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
        info!("Sending {full_path:?} finished");

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
                let _ = self.set_item_error(id, e);
                let _ = self.item_finished();

                self.failed.fetch_add(1, Relaxed);
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

const TRANSFERS: TableDefinition<u128, &[u8]> = TableDefinition::new("transfers");

// #[derive(Clone)]
pub struct Transfers {
    pub pending_transfers: std::sync::RwLock<HashMap<Uuid, Arc<PendingTransfer>>>,
    devices: Devices,
    event_tx: tokio::sync::mpsc::Sender<AppEvent>,
    // db: redb::Database,
}

impl Transfers {
    pub fn new(devices: Devices, event_tx: tokio::sync::mpsc::Sender<AppEvent>) -> Self {
        // let db_dir = dirs::data_dir().unwrap_or_else(|| dirs::runtime_dir().unwrap());
        // let db_dir = db_dir.join("transfers");
        // let db = redb::Database::create(db_dir).expect("Failed to create transfers database");
        let pending_transfers = std::sync::RwLock::new(HashMap::new());

        Transfers {
            event_tx,
            pending_transfers,
            devices,
        }
    }

    pub async fn new_offer(
        &self,
        target: DeviceId,
        direction: TransferDirection,
        offer: TransferOffer<PathTree>,
    ) {
        let id = offer.transfer_id;
        let t = Arc::new(PendingTransfer::from_offer(
            self.event_tx.clone(),
            target,
            direction,
            offer,
        ));
        if direction == TransferDirection::Outcoming {
            if let Some(conn) = self.devices.get_connection(target) {
                let _ = t.clone().start_sending(conn).await;
            }
        }
        self.pending_transfers.write().unwrap().insert(id, t);
    }

    pub async fn forward_incoming_stream(
        &self,
        transfer_id: Uuid,
        stream: StreamPair,
    ) -> Result<()> {
        let Some(t) = self
            .pending_transfers
            .read()
            .unwrap()
            .get(&transfer_id)
            .cloned()
        else {
            error!("Transfer not found");
            bail!("Transfer not found")
        };
        let permit = t.sem.clone().acquire_owned().await.unwrap();
        t.process_stream(permit, stream).await
    }
}
