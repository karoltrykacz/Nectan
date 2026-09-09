use std::collections::HashMap;
use std::io;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use anyhow::bail;
use serde::Deserialize;
use serde::Serialize;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncSeekExt;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::messages::NetMessage;
use crate::stream::StreamPair;

#[derive(Debug)]
struct FileTransferHeader<'a> {
    file_size: u64,
    sent_bytes: u64,
    is_file: bool,
    path_bytes_len: u16,
    path_bytes: &'a [u8],
}

impl<'a> FileTransferHeader<'a> {
    pub const MAX_PATH_LEN: usize = 32_767;
    pub const FIXED_HEADER_SIZE: usize = 19;

    pub fn to_bytes(&self) -> Vec<u8> {
        let total_len = 8 + 8 + 1 + 2 + self.path_bytes.len();
        let mut bytes = Vec::with_capacity(total_len);

        bytes.extend_from_slice(&self.file_size.to_be_bytes());
        bytes.extend_from_slice(&self.sent_bytes.to_be_bytes());
        bytes.push(self.is_file as u8);
        bytes.extend_from_slice(&self.path_bytes_len.to_be_bytes());
        bytes.extend_from_slice(self.path_bytes);

        bytes
    }

    pub fn from_bytes(mut bytes: &'a [u8]) -> anyhow::Result<Self> {
        if bytes.len() < Self::FIXED_HEADER_SIZE {
            bail!("Header too short.")
        }
        let file_size = u64::from_be_bytes(bytes[..8].try_into().unwrap());
        bytes = &bytes[8..];

        let sent_bytes = u64::from_be_bytes(bytes[..8].try_into().unwrap());
        bytes = &bytes[8..];

        let is_file = bytes[0] != 0;
        bytes = &bytes[1..];

        let path_bytes_len = u16::from_be_bytes(bytes[..2].try_into().unwrap());
        bytes = &bytes[2..];

        let path_len = path_bytes_len as usize;

        if bytes.len() < path_len {
            bail!("Invalid path len.");
        } else if bytes.len() > path_len {
            bail!("Input buffer contains trailing unparsed bytes.");
        }

        let path_bytes = &bytes[..path_len];
        Ok(Self {
            file_size,
            sent_bytes,
            path_bytes_len,
            is_file,
            path_bytes,
        })
    }
    pub async fn read_bytes_from_stream<R: AsyncRead + Unpin>(stream: &mut R) -> Result<Vec<u8>> {
        let mut fixed_buf = [0u8; Self::FIXED_HEADER_SIZE];
        stream.read_exact(&mut fixed_buf).await?;

        let path_len = u16::from_be_bytes(fixed_buf[17..19].try_into().unwrap()) as usize;
        if path_len > Self::MAX_PATH_LEN {
            bail!("Path too large.");
        }

        let mut full_buf = vec![0u8; Self::FIXED_HEADER_SIZE + path_len];
        full_buf[..Self::FIXED_HEADER_SIZE].copy_from_slice(&fixed_buf);

        stream
            .read_exact(&mut full_buf[Self::FIXED_HEADER_SIZE..])
            .await?;

        Ok(full_buf)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TransferItem {
    pub id: u64,
    pub file_size: u64,
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
    BadHeader,
    OpenFail,
    Terminated,
}
impl Eq for TransferItemError {}

pub async fn recieve_item(
    mut stream_tx: iroh::endpoint::SendStream,
    mut stream_rx: iroh::endpoint::RecvStream,
) -> Result<(), TransferItemError> {
    let root_path = "/home/karol/Videos/Destination";
    let raw_bytes = FileTransferHeader::read_bytes_from_stream(&mut stream_rx)
        .await
        .map_err(|_| TransferItemError::StreamError)?;

    let header =
        FileTransferHeader::from_bytes(&raw_bytes).map_err(|_| TransferItemError::BadHeader)?;

    let file_size = header.file_size;
    let path_str = String::from_utf8_lossy(header.path_bytes);
    let original_file = Path::new(&root_path).join(&*path_str);

    if std::fs::exists(&original_file).map_err(|_| TransferItemError::OpenFail)? {
        return Err(TransferItemError::FileAlreadyExisted);
    }

    let full_path = PathBuf::from(format!("{}.NectanLock", original_file.display()));

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

    // let _ = out_file.lock();

    const CHUNK_SIZE: usize = 128 * 1024;
    let mut total_written = header.sent_bytes;

    loop {
        let Some(chunk) = stream_rx
            .read_chunk(CHUNK_SIZE)
            .await
            .map_err(|_| TransferItemError::StreamError)?
        else {
            // Stream dead
            if total_written < file_size {
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

    tracing::info!("Finished writing the file. {total_written} {file_size}");

    out_file
        .flush()
        .await
        .map_err(|_| TransferItemError::StreamError)?;

    stream_tx
        .write_all(&total_written.to_be_bytes())
        .await
        .map_err(|_| TransferItemError::StreamError)?;

    let _ = stream_tx.finish();
    stream_tx.stopped();

    let final_destination = {
        let s = full_path.to_string_lossy();
        let stripped = s.strip_suffix(".NectanLock").unwrap_or(&s);
        PathBuf::from(stripped)
    };

    std::fs::rename(&full_path, &final_destination).map_err(|_| TransferItemError::FileIOError)?;

    Ok(())
}

pub async fn send_item(
    transfer_id: Uuid,
    mut item: TransferItem,
    mut stream: StreamPair,
) -> Result<(), TransferItemError> {
    let root_path = "/home/karol/Videos/Source";
    let full_path = Path::new(&root_path).join(&item.path);

    println!("Sending {full_path:?}");

    let mut file = match tokio::fs::File::open(&full_path).await {
        Ok(file) => file,
        Err(e) => match e.kind() {
            ErrorKind::PermissionDenied => {
                tracing::error!("Failed to open file {full_path:?}");
                return Err(TransferItemError::FileOpenPermissionDenied);
            }
            ErrorKind::NotFound => {
                tracing::error!("File not found {full_path:?}");
                return Err(TransferItemError::FileNotFound);
            }
            _ => {
                tracing::error!("Unhandled send item error - {e}");
                return Err(TransferItemError::OpenFail);
            }
        },
    };
    let path_str = item.path.to_string_lossy();
    let path_bytes = path_str.as_bytes();

    NetMessage::TransferStream { transfer_id }
        .write(&mut stream.tx())
        .await
        .map_err(|_| TransferItemError::StreamError)?;

    let header = FileTransferHeader {
        file_size: item.file_size,
        sent_bytes: item.sent_bytes,
        is_file: item.is_file,
        path_bytes_len: path_bytes.len() as u16,
        path_bytes,
    };

    // NetMessage::TransferStream { transfer_id }
    //     .write(&mut stream.tx())
    //     .await
    //     .map_err(|_| TransferItemError::StreamError)?;

    stream
        .tx()
        .write_all(&header.to_bytes())
        .await
        .map_err(|_| TransferItemError::StreamError)?;

    let mut buf = Vec::with_capacity(64 * 1024);
    // let mut file_eof = false;

    // let mut buf_a = Vec::with_capacity(64 * 1024);
    // let mut buf_b = Vec::with_capacity(64 * 1024);
    // let mut use_a = true;

    loop {
        // if file_eof && item.sent_bytes == item.file_size {
        //     break;
        // }

        tokio::select! {
            result = file.read_buf(&mut buf) => {
                let n = result.map_err(|_| TransferItemError::FileIOError)?;

                if n == 0 {
                    break;
                }

                item.sent_bytes += n as u64;
                let chunk = std::mem::replace(&mut buf, Vec::with_capacity(64 * 1024));
                stream.tx().write_chunk(chunk.into()).await.map_err(|_| TransferItemError::StreamError)?;
            }
        }
    }

    stream.tx();

    Ok(())
}

#[derive(Clone, Copy)]
enum TransferDirection {
    Outcoming,
    Incoming,
}

#[derive(Clone)]
pub struct PendingTransfer {
    id: Uuid,
    direction: TransferDirection,
}

#[derive(Clone)]
pub struct PendingTransfers {
    inner: Arc<std::sync::RwLock<HashMap<Uuid, PendingTransfer>>>,
}

impl PendingTransfers {
    pub fn new() -> Self {
        PendingTransfers {
            inner: Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }
    pub fn get(&self, id: Uuid) -> Option<PendingTransfer> {
        self.inner.read().unwrap().get(&id).cloned()
    }
}
