use crate::{
    devices::{DeviceId, Devices, Username},
    messages::{AppEvent, NetMessage, StreamableMessage},
    protocol::{NectanState, TransferOffer},
    stream::StreamPair,
    transfers::TransferDirection::Outcoming,
};
use anyhow::{Result, bail, ensure};
use ed25519_dalek::VerifyingKey;
use fixedbitset::FixedBitSet;
use iroh::endpoint::Connection;
use redb::{
    ReadableDatabase, ReadableTable, TableDefinition, TypeName, Value,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::ErrorKind,
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{
            AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed,
        },
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    sync::{OwnedSemaphorePermit, Semaphore, mpsc},
};
use tokio_util::sync::{
    CancellationToken, WaitForCancellationFuture,
};
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TransferItem {
    pub idx: u32,
    pub size: u64,
    pub sent_bytes: u64,
    pub is_dir: bool,
    /// Relative path (starting from the transfer root path)
    pub path: PathBuf,
    pub err: Option<TransferItemError>,
}

impl TransferItem {
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
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

    fn as_bytes<'a, 'b: 'a>(
        value: &'a Self::SelfType<'b>,
    ) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        postcard::to_allocvec(value).unwrap()
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Clone, Copy)]
pub enum TransferItemError {
    FileIOError,
    FileAlreadyExisted,
    PermissionDenied,
    FileNotFound,
    StreamError,
    OpenFail,
    Terminated,
}
impl Eq for TransferItemError {}

#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "kind")]
pub enum TransferOfferError {
    DeviceOffline,
    InvalidDestination,
    TransferRejected,
    UnexpectedResponse,
    ConnectionFailed,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransferMeta {
    pub id: Uuid,
    pub total_size: u64,
    pub sent_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct TransferItemHeader {
    id: u32,
    file_size: u64,
    sent_bytes: u64,
    is_dir: bool,
    path: PathBuf,
}

/// Send transfer offer to remote device
/// If the offer is accepted start processing the transfer
pub async fn send_contents(
    state: &NectanState,
    target: DeviceId,
    offer: TransferOffer,
    root_path: PathBuf,
    delivered_offer: tokio::sync::oneshot::Sender<()>,
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

    // Get root path (will be needed later)
    // Write compressed offer
    let offer = Box::new(offer);
    let msg = NetMessage::TransferOfferMsg {
        offer: offer.clone(),
    };
    if msg.write(&mut tx).await.is_err() {
        return Err(TransferOfferError::ConnectionFailed);
    }

    // // Propagate app event (changes the send modal state)
    // state.emit(AppEvent::TransferOfferDelivered).await;
    delivered_offer.send(());

    // Read remote device response
    let Ok(offer_response) = NetMessage::read_async(&mut rx).await
    else {
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

    // Transfer accepted, start processing
    state
        .transfers
        .add_transfer(
            target,
            device.username,
            Outcoming,
            root_path,
            *offer,
        )
        .await;

    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TransferDirection {
    Outcoming,
    Incoming,
}

impl TransferDirection {
    pub fn is_incoming(self) -> bool {
        matches!(self, Self::Incoming)
    }
}

const DUMB_ITEMS: TableDefinition<u32, TransferItem> =
    TableDefinition::new("items");

#[derive(Clone, Copy)]
pub enum TransferStatus {
    Downloading,
    Finished,
    /// Explicit state so user will know what happened
    CancelledByRemote,
}

impl TransferStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Downloading => "Downloading",
            Self::CancelledByRemote => "Cancelled",
            Self::Finished => "Finished",
        }
    }
}

pub struct PendingTransfer {
    pub id: Uuid,
    pub target_name: Username,
    target: DeviceId,
    pub direction: TransferDirection,
    items_queue: Mutex<FixedBitSet>,
    pub name: String,
    /// Contains folders symlinks etc
    pub total_items: u32,
    /// Counter for sent (unrecoverably failed items count as sent)
    pub processed_items: AtomicU32,
    pub failed: AtomicU32,
    pub total_size: u64,
    pub sent_bytes: AtomicU64,
    pub speed: AtomicU64,
    db: redb::Database,
    /// When sending, common parent of all items
    /// When receiving, target directory
    pub root_path: PathBuf,
    cancel_token: CancellationToken,
    sem: Arc<Semaphore>,
    status: Mutex<TransferStatus>, // Can be coerced to simple atomic bool
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum TransferControl {
    /// Acknowledges received bytes
    Ack(u64),
    /// Informs peer about error
    Error(TransferItemError),
}

impl PendingTransfer {
    pub fn test(total_items: u32) -> Self {
        let id = Uuid::new_v4();
        let db = redb::Database::create(
            std::env::temp_dir().join(id.to_string()),
        )
        .unwrap();
        let txn = db.begin_write().unwrap();
        {
            let mut table = txn.open_table(DUMB_ITEMS).unwrap();
            for i in 0..total_items {
                table
                    .insert(
                        i,
                        TransferItem {
                            idx: i,
                            sent_bytes: (i * 123) as u64,
                            path: PathBuf::from(format!(
                                "/media/karol/file-{}",
                                i
                            )),
                            size: 123 * (i * i) as u64,
                            is_dir: true,
                            err: None,
                        },
                    )
                    .unwrap();
            }
        }
        txn.commit().unwrap();

        PendingTransfer {
            target_name: Username::default(),
            name: "Default".into(),
            processed_items: AtomicU32::new(123),
            total_items: 123123,
            total_size: 1231233312,
            sent_bytes: AtomicU64::new(123432),
            speed: AtomicU64::new(100000000),
            id,
            target: VerifyingKey::default(),
            direction: TransferDirection::Incoming,
            items_queue: Mutex::new(FixedBitSet::new()),
            failed: AtomicU32::new(10),
            db,
            root_path: PathBuf::from("/media/karol/example"),
            cancel_token: CancellationToken::new(),
            sem: Arc::new(Semaphore::new(69)),
            status: Mutex::new(TransferStatus::Downloading),
        }
    }

    pub fn from_offer(
        target: DeviceId,
        target_name: Username,
        root_path: PathBuf,
        direction: TransferDirection,
        offer: TransferOffer,
    ) -> Self {
        info!("New transfer from offer.");

        let id = offer.transfer_id;
        println!("Transfer root_path {root_path:?}");

        let db_dir = dirs::data_dir()
            .unwrap_or_else(|| dirs::runtime_dir().unwrap());
        let dir = match direction {
            TransferDirection::Outcoming => "out",
            TransferDirection::Incoming => "in",
        };
        let db_dir = db_dir.join(format!("Nectan/t-{id}-{}", dir));
        let db = redb::Database::create(db_dir)
            .expect("Failed to create database");

        let txn = db.begin_write().unwrap();
        {
            let mut table = txn.open_table(DUMB_ITEMS).unwrap();
            for (idx, entry) in offer.entries.iter().enumerate() {
                let item = TransferItem {
                    path: entry.path.clone(),
                    is_dir: entry.is_dir,
                    idx: idx as u32,
                    sent_bytes: 0,
                    size: entry.size,
                    err: None,
                };
                let _ = table.insert(idx as u32, item);
            }
        }
        let _ = txn.commit();

        PendingTransfer {
            id,
            name: offer.transfer_name,
            direction,
            failed: AtomicU32::new(0),
            processed_items: AtomicU32::new(0),
            sent_bytes: AtomicU64::new(0),
            items_queue: Mutex::new(FixedBitSet::with_capacity(
                offer.entries_num as usize,
            )),
            speed: AtomicU64::new(0),
            db,
            total_items: offer.entries_num as u32,
            total_size: offer.total_size,
            root_path,
            cancel_token: CancellationToken::new(),
            target,
            target_name,
            sem: Arc::new(Semaphore::new(4)),
            status: Mutex::new(TransferStatus::Downloading),
        }
    }

    pub fn set_status(&self, s: TransferStatus) {
        *self.status.lock().unwrap() = s;
    }

    pub fn status(&self) -> TransferStatus {
        *self.status.lock().unwrap()
    }

    fn cancelled(&self) -> WaitForCancellationFuture<'_> {
        self.cancel_token.cancelled()
    }

    fn is_cancelled(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    fn cancel(&self) {
        self.cancel_token.cancel()
        // Two things need to happen here - the stream forwarder must know that the transfer is cancelled
        // Also if the transfer if incoming - stop the motherfucker
    }

    /// Returns remaining items
    fn item_finished(&self, id: u32) -> u32 {
        let left = self.processed_items.fetch_add(1, Relaxed) + 1;
        let Ok(tx) = self.db.begin_write() else {
            return left;
        };
        {
            let Ok(mut table) = tx.open_table(DUMB_ITEMS) else {
                return left;
            };
            let mut item = match table.get(id) {
                Ok(Some(guard)) => guard.value(),
                _ => return left,
            };
            item.sent_bytes = item.size;
            if table.insert(id, item).is_err() {
                return left;
            }
        }
        let _ = tx.commit();
        left
    }

    fn ack_sent_bytes(&self, bytes: u64) -> u64 {
        self.sent_bytes.fetch_add(bytes, Relaxed) + bytes
    }

    fn ack_sent_bytes_db(&self, id: u32, bytes: u64) {
        let Ok(tx) = self.db.begin_write() else {
            return;
        };
        {
            let Ok(mut table) = tx.open_table(DUMB_ITEMS) else {
                return;
            };
            let mut item = match table.get(id) {
                Ok(Some(guard)) => guard.value(),
                _ => return,
            };
            item.sent_bytes = bytes;
            if table.insert(id, item).is_err() {
                return;
            }
        }
        let _ = tx.commit();
    }

    pub fn get_item(
        &self,
        id: u32,
    ) -> Result<TransferItem, redb::Error> {
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

    fn start_speed_updater(s: Arc<Self>) {
        std::thread::spawn(move || {
            let mut last_sent = s.sent_bytes.load(Relaxed);

            // Temporal resolution
            let tick = std::time::Duration::from_millis(100);

            // Bytes per second
            let mut speed = 0.0;

            loop {
                if s.is_cancelled() {
                    return;
                }
                std::thread::sleep(tick);
                let sent = s.sent_bytes.load(Relaxed);
                // Convert to bits
                let diff = (sent - last_sent) * 8;
                let bps = diff as f32 / tick.as_secs_f32();
                let diff = (speed - bps) * 0.01;

                speed -= diff;
                last_sent = sent;

                s.speed.store(speed as u64, Relaxed);
            }
        });
    }

    /// Todo - make this know about the cancel token!
    async fn start_sending(
        self: Arc<Self>,
        conn: Connection,
    ) -> Result<()> {
        info!(
            "Starting sending [{}] [{} ITEMS]",
            &self.id.to_string()[0..8],
            self.items_queue.lock().unwrap().zeroes().count()
        );

        // Get first batch of items
        let mut queue = Vec::new();
        queue.extend(self.unsent_batch());

        PendingTransfer::start_speed_updater(self.clone());

        loop {
            if self.is_cancelled() {
                return Ok(());
            }
            let item = match queue.pop() {
                Some(id) => {
                    // TODO graceful err, file not found???
                    self.get_item(id as u32)
                        .expect("Transfer database error.")
                }
                None => {
                    let processed =
                        self.processed_items.load(Relaxed);
                    let drained = processed == self.total_items;

                    if drained {
                        // All transfers processed, wait until all finished
                        info!(
                            "All transfers drained. Waiting for last items to finish."
                        );
                        let _ = self.sem.acquire_many(4).await;
                        self.set_status(TransferStatus::Finished);
                        return Ok(());
                    } else {
                        let unsent = self.unsent_batch();
                        let unsent_len = unsent.len();

                        queue.extend(unsent);

                        if unsent_len == 0 {
                            tokio::time::sleep(Duration::from_secs(
                                1,
                            ))
                            .await;
                        }
                        continue;
                    }
                }
            };

            let stream = StreamPair::open(&conn).await?;
            let permit = tokio::select! {
                permit = self.sem.clone().acquire_owned() =>{
                    permit.unwrap()
                }
                _ = self.cancelled() =>{
                    return Ok(())
                }
            };

            let s = self.clone();
            tokio::spawn(async move {
                let item_id = item.idx;
                let _permit = permit;

                match s.send_item(item, stream).await {
                    Ok(()) => {
                        s.item_finished(item_id);
                    }
                    Err(e) => {
                        error!("Sending item failed! {e:?}");
                        s.handle_item_err(item_id, e);
                    }
                }
            });
        }
    }

    pub async fn process_stream(
        self: Arc<Self>,
        mut stream: StreamPair,
    ) -> Result<()> {
        let permit = tokio::select! {
            permit = self.sem.clone().acquire_owned() =>{
                permit.unwrap()
            }
            _ = self.cancelled() => {
                return Ok(())
            }
        };

        println!(
            "Processing stream for {}",
            &self.id.to_string()[0..8]
        );

        ensure!(
            self.direction == TransferDirection::Incoming,
            "Transfer must be incoming."
        );

        let header: TransferItemHeader = stream.read().await?;
        println!("Read header {:#?}", header);

        let id = header.id;

        let r = match self
            .receive_item(header, &mut stream, permit)
            .await
        {
            Ok(()) => self.item_finished(id),
            Err(e) => {
                println!("Failed to receive item [{e:?}]");
                let _ =
                    stream.write(&TransferControl::Error(e)).await;
                self.handle_item_err(id, e)
            }
        };

        if r == 0 {
            info!("No more items to receive.");
            self.cancel();
        }

        Ok(())
    }

    // Todo - make second channel for transfer controls

    async fn receive_item(
        &self,
        header: TransferItemHeader,
        stream: &mut StreamPair,
        _permit: OwnedSemaphorePermit,
    ) -> Result<(), TransferItemError> {
        let file_size = header.file_size;
        let item_path = header.path;

        let item_path: PathBuf = item_path
            .components()
            .filter(|c| matches!(c, Component::Normal(_)))
            .collect();

        let output_dir = Path::new(&self.root_path).join(&item_path);

        println!(
            "Receiving {} to {}",
            item_path.display(),
            output_dir.display()
        );

        // Just create the folder
        if header.is_dir {
            if let Some(parent) = output_dir.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|_| TransferItemError::FileIOError)?;
            }

            return Ok(());
        }

        if std::fs::exists(&output_dir)
            .map_err(|_| TransferItemError::OpenFail)?
        {
            error!(
                "File [{}] already exsited.",
                output_dir.display()
            );
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

        let period = Duration::from_millis(250);
        let mut tick = tokio::time::interval_at(
            tokio::time::Instant::now() + period,
            period,
        );

        loop {
            tokio::select! {
                _ = self.cancelled() =>{
                    warn!("Receiving [{}] cancelled.", item_path.display());
                    return Err(TransferItemError::Terminated);
                }
                _ = tick.tick() => {
                    stream.write(&TransferControl::Ack(total_written))
                        .await
                        .map_err(|_| {
                        error!("Failed to write the ack.");
                        TransferItemError::StreamError
                    })?;
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

        std::fs::rename(&lock_path, &output_dir)
            .map_err(|_| TransferItemError::FileIOError)?;

        // Final ack
        stream
            .write(&TransferControl::Ack(total_written))
            .await
            .map_err(|_| TransferItemError::StreamError)?;

        info!("Received {}", item_path.display());
        Ok(())
    }

    async fn send_item(
        &self,
        mut item: TransferItem,
        mut stream: StreamPair,
    ) -> Result<(), TransferItemError> {
        let path = item.path;
        info!("Sending {}", path.display());

        let mut file = match tokio::fs::File::open(&path).await {
            Ok(file) => file,
            Err(e) => match e.kind() {
                ErrorKind::PermissionDenied => {
                    error!("Failed to open {path:?}");
                    return Err(TransferItemError::PermissionDenied);
                }
                ErrorKind::NotFound => {
                    error!("File not found {path:?}");
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
            .map_err(|_| {
                error!("Failed to open transfer stream");
                TransferItemError::StreamError
            })?;

        let relative_path = match path.strip_prefix(&self.root_path) {
            Ok(path) => path.to_path_buf(),
            Err(e) => {
                error!("Failed to strip path prefix. {e:?}");
                return Err(TransferItemError::FileIOError);
            }
        };

        stream
            .write(&TransferItemHeader {
                id: item.idx,
                file_size: item.size,
                sent_bytes: item.sent_bytes,
                is_dir: item.is_dir,
                path: relative_path,
            })
            .await
            .map_err(|_| {
                error!("Failed to send item header.");
                TransferItemError::StreamError
            })?;

        // If its folder just send the header
        if item.is_dir {
            self.ack_sent_bytes(item.size);
            return Ok(());
        }

        let (mut rx, mut tx) = stream.into_split();

        let (ctrl_tx, mut ctrl_rx) =
            mpsc::channel::<TransferControl>(8);
        let reader = tokio::spawn(async move {
            while let Ok(c) =
                TransferControl::read_async(&mut rx).await
            {
                if ctrl_tx.send(c).await.is_err() {
                    break;
                }
            }
        });

        const BUF_SIZE: usize = 64 * 1024;

        let mut buf = Vec::with_capacity(BUF_SIZE);

        loop {
            tokio::select! {
                _ = self.cancelled() => {
                    warn!("Sending item [{}] terminated.", path.display());
                    reader.abort();
                    return Err(TransferItemError::Terminated);
                }
                Some(ctrl) = ctrl_rx.recv() => {
                    match ctrl {
                        TransferControl::Error(e) => return Err(e),
                        TransferControl::Ack(bytes) => {
                            let diff = bytes - item.sent_bytes;
                            self.ack_sent_bytes(diff);
                            self.ack_sent_bytes_db(item.idx, item.sent_bytes);
                            item.sent_bytes = bytes;
                        }
                    }
                }
                result = file.read_buf(&mut buf)=>{
                    let n = result.map_err(|_| {
                        error!("Failed to read file.");
                        TransferItemError::FileIOError
                    })?;

                    if n == 0 {
                        break;
                    }

                    let chunk = std::mem::replace(&mut buf, Vec::with_capacity(BUF_SIZE));
                    tx
                        .write_chunk(chunk.into())
                        .await
                        .map_err(|_|{
                            error!("Failed to write chunk.");
                            TransferItemError::StreamError
                        })?;
                }
            };
        }
        info!("Sending {} finished", path.display());
        self.ack_sent_bytes(item.size);
        // ctrl_rx.recv()

        Ok(())
    }

    /// Return remaining items
    fn handle_item_err(&self, id: u32, e: TransferItemError) -> u32 {
        match e {
            // Unrecoverable errors
            TransferItemError::PermissionDenied
            | TransferItemError::OpenFail
            | TransferItemError::FileAlreadyExisted
            | TransferItemError::FileNotFound
            | TransferItemError::FileIOError => {
                let _ = self.set_item_error(id, e);
                let _ = self.item_finished(id);

                self.failed.fetch_add(1, Relaxed);
                self.processed_items.fetch_add(1, Relaxed) + 1
            }
            // Recoverable (Network) errors - nop
            TransferItemError::StreamError
            | TransferItemError::Terminated => {
                if self.direction == TransferDirection::Outcoming {
                    // Move item back to the queue
                    unsafe {
                        self.items_queue
                            .lock()
                            .unwrap()
                            .set_unchecked(id as usize, false);
                    }
                }
                self.processed_items.load(Relaxed)
            }
        }
    }
    pub fn set_item_progress(
        &self,
        item_id: u32,
        sent_bytes: u64,
    ) -> Result<(), redb::Error> {
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(DUMB_ITEMS)?;
            let Some(mut meta) =
                table.get(item_id)?.map(|g| g.value())
            else {
                error!("Item not found.");
                return Ok(());
            };
            meta.sent_bytes = sent_bytes;
            table.insert(item_id, &meta)?;
        }
        txn.commit()?;
        Ok(())
    }
    pub fn set_item_error(
        &self,
        item_id: u32,
        e: TransferItemError,
    ) -> Result<(), redb::Error> {
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(DUMB_ITEMS)?;
            let Some(mut meta) =
                table.get(item_id)?.map(|g| g.value())
            else {
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

pub struct PendingTransfers {
    pub inner: std::sync::RwLock<HashMap<Uuid, Arc<PendingTransfer>>>,
}

pub struct Transfers {
    pending_transfers: PendingTransfers,
    devices: Devices,
    event_tx: tokio::sync::mpsc::Sender<AppEvent>,
    // db: redb::Database,
}

impl Transfers {
    pub fn new(
        devices: Devices,
        event_tx: tokio::sync::mpsc::Sender<AppEvent>,
    ) -> Self {
        // let db_dir = dirs::data_dir().unwrap_or_else(|| dirs::runtime_dir().unwrap());
        // let db_dir = db_dir.join("transfers");
        // let db = redb::Database::create(db_dir).expect("Failed to create transfers database");
        let pending_transfers = PendingTransfers {
            inner: std::sync::RwLock::new(HashMap::new()),
        };

        Transfers {
            event_tx,
            pending_transfers,
            devices,
        }
    }
    pub fn pending_transfers(
        &self,
    ) -> &std::sync::RwLock<HashMap<Uuid, Arc<PendingTransfer>>> {
        &self.pending_transfers.inner
    }

    pub fn get_pending_transfers(&self) -> Vec<Arc<PendingTransfer>> {
        self.pending_transfers()
            .read()
            .unwrap()
            .values()
            .cloned()
            .collect()
    }

    // pub fn get_pending_transfers(&self) -> Vec<Arc<PendingTransfer>> {
    //     self.pending_transfers()
    //         .read()
    //         .unwrap()
    //         .values()
    //         .cloned()
    //         .collect()
    // }

    pub fn get_transfers(&self) -> Vec<TransferMeta> {
        self.pending_transfers()
            .read()
            .unwrap()
            .values()
            .map(|t| TransferMeta {
                id: t.id,
                total_size: t.total_size,
                sent_bytes: t.sent_bytes.load(Relaxed),
            })
            .collect()
    }

    pub async fn add_transfer(
        &self,
        target: DeviceId,
        target_name: Username,
        direction: TransferDirection,
        root_path: PathBuf,
        offer: TransferOffer,
    ) {
        let id = offer.transfer_id;

        let t = Arc::new(PendingTransfer::from_offer(
            target,
            target_name,
            root_path,
            direction,
            offer,
        ));

        self.pending_transfers()
            .write()
            .unwrap()
            .insert(id, t.clone());

        if direction == TransferDirection::Outcoming
            && let Some(conn) = self.devices.get_connection(target)
        {
            tokio::spawn(t.start_sending(conn));
        }
        let _ = self.event_tx.send(AppEvent::TransfersUpdated).await;
    }

    pub async fn forward_incoming_stream(
        &self,
        id: Uuid,
        stream: StreamPair,
    ) -> Result<()> {
        let Some(t) = self.get(id) else {
            error!("Transfer not found");
            bail!("Transfer not found")
        };
        t.process_stream(stream).await
    }

    fn get(&self, id: Uuid) -> Option<Arc<PendingTransfer>> {
        self.pending_transfers().read().unwrap().get(&id).cloned()
    }

    /// Called when remote device cancelled the transfer
    pub fn transfer_cancelled(&self, id: Uuid) -> Result<()> {
        let Some(t) = self.get(id) else {
            error!("Transfer not found");
            bail!("Transfer not found")
        };

        t.cancel();
        t.set_status(TransferStatus::CancelledByRemote);

        let _ =
            self.event_tx.blocking_send(AppEvent::TransfersUpdated);

        Ok(())
    }

    pub fn delete_transfer(&self, id: Uuid) -> Result<()> {
        let mut lock = self.pending_transfers().write().unwrap();
        let Some(t) = lock.remove(&id) else {
            bail!("Transfer not found")
        };

        t.cancel();
        info!("Deleted transfer.");

        // Send info to remote device
        // Error while notifying remote device is harmless because
        // transfers are not supposed to survive hard connection change
        // (connection changes like change wifi to mobile will still work)
        // Outgoing Transfer will automatically kill itself if the connection failed
        // Vaults should be used for robust and recoverable transfers

        if let Some(conn) = self.devices.get_connection(t.target) {
            tokio::spawn(async move {
                let mut stream = StreamPair::open(&conn).await?;
                let msg = NetMessage::TransferCancelled {
                    transfer_id: t.id,
                };
                stream.write(&msg).await
            });
        };

        let _ =
            self.event_tx.blocking_send(AppEvent::TransfersUpdated);

        Ok(())
    }
}
