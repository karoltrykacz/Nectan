use anyhow::Result;
use bytes::Bytes;
use iroh::endpoint::{Connection, ReadExactError, VarInt};
use serde::Serialize;
use tokio::io;

pub trait SendStream: Send {
    /// Send bytes to the stream. This takes a `Bytes` because iroh can directly use them.
    ///
    /// This method is not cancellation safe. Even if this does not resolve, some bytes may have been written when previously polled.
    fn send_bytes(&mut self, bytes: Bytes) -> impl Future<Output = io::Result<()>> + Send;
    /// Send that sends a fixed sized buffer.
    fn send(&mut self, buf: &[u8]) -> impl Future<Output = io::Result<()>> + Send;
    /// Sync the stream. Not needed for iroh, but needed for intermediate buffered streams such as compression.
    fn sync(&mut self) -> impl Future<Output = io::Result<()>> + Send;
    /// Reset the stream with the given error code.
    fn reset(&mut self, code: VarInt) -> io::Result<()>;
    /// Wait for the stream to be stopped, returning the error code if it was.
    fn stopped(&mut self) -> impl Future<Output = io::Result<Option<VarInt>>> + Send;
    /// Get the stream id.
    fn id(&self) -> u64;

    fn finish(&mut self) -> io::Result<()>;
}

/// An abstract `iroh::endpoint::RecvStream`.
pub trait RecvStream: Send {
    /// Receive up to `len` bytes from the stream, directly into a `Bytes`.
    fn recv_bytes(&mut self, len: usize) -> impl Future<Output = io::Result<Bytes>> + Send;
    /// Receive exactly `len` bytes from the stream, directly into a `Bytes`.
    ///
    /// This will return an error if the stream ends before `len` bytes are read.
    ///
    /// Note that this is different from `recv_bytes`, which will return fewer bytes if the stream ends.
    fn recv_bytes_exact(&mut self, len: usize) -> impl Future<Output = io::Result<Bytes>> + Send;
    /// Receive exactly `target.len()` bytes from the stream.
    fn recv_exact(&mut self, target: &mut [u8]) -> impl Future<Output = io::Result<()>> + Send;
    /// Stop the stream with the given error code.
    fn stop(&mut self, code: VarInt) -> io::Result<()>;
    /// Get the stream id.
    fn id(&self) -> u64;
}

impl RecvStream for iroh::endpoint::RecvStream {
    async fn recv_bytes(&mut self, len: usize) -> io::Result<Bytes> {
        let mut buf = vec![0; len];
        match self.read_exact(&mut buf).await {
            Err(ReadExactError::FinishedEarly(n)) => {
                buf.truncate(n);
            }
            Err(ReadExactError::ReadError(e)) => {
                return Err(e.into());
            }
            Ok(()) => {}
        };
        Ok(buf.into())
    }

    async fn recv_bytes_exact(&mut self, len: usize) -> io::Result<Bytes> {
        let mut buf = vec![0; len];
        self.read_exact(&mut buf).await.map_err(|e| match e {
            ReadExactError::FinishedEarly(0) => io::Error::new(io::ErrorKind::UnexpectedEof, ""),
            ReadExactError::FinishedEarly(_) => io::Error::new(io::ErrorKind::InvalidData, ""),
            ReadExactError::ReadError(e) => e.into(),
        })?;
        Ok(buf.into())
    }

    async fn recv_exact(&mut self, target: &mut [u8]) -> io::Result<()> {
        self.read_exact(target).await.map_err(|e| match e {
            ReadExactError::FinishedEarly(0) => io::Error::new(io::ErrorKind::UnexpectedEof, ""),
            ReadExactError::FinishedEarly(_) => io::Error::new(io::ErrorKind::InvalidData, ""),
            ReadExactError::ReadError(e) => e.into(),
        })
    }

    fn id(&self) -> u64 {
        self.id().index()
    }

    fn stop(&mut self, code: VarInt) -> io::Result<()> {
        Ok(self.stop(code)?)
    }
}
impl SendStream for iroh::endpoint::SendStream {
    fn id(&self) -> u64 {
        self.id().index()
    }

    async fn send_bytes(&mut self, bytes: Bytes) -> io::Result<()> {
        Ok(self.write_chunk(bytes).await?)
    }

    async fn send(&mut self, buf: &[u8]) -> io::Result<()> {
        Ok(self.write_all(buf).await?)
    }

    async fn sync(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn reset(&mut self, code: VarInt) -> io::Result<()> {
        Ok(self.reset(code)?)
    }

    async fn stopped(&mut self) -> io::Result<Option<VarInt>> {
        Ok(iroh::endpoint::SendStream::stopped(self).await?)
    }

    fn finish(&mut self) -> io::Result<()> {
        Ok(self.finish()?)
    }
}

pub trait StreamableMessage: Sized {
    fn read_async<R: crate::stream::RecvStream>(
        rx: &mut R,
    ) -> impl Future<Output = Result<Self>> + Send;
    fn write<T: crate::stream::SendStream>(
        &self,
        tx: &mut T,
    ) -> impl Future<Output = Result<()>> + Send;
}

impl<M: Serialize + serde::de::DeserializeOwned + Sync> StreamableMessage for M {
    async fn read_async<R: crate::stream::RecvStream>(rx: &mut R) -> Result<Self> {
        let mut len_buf = [0u8; 4];
        rx.recv_exact(&mut len_buf).await?;

        let len = u32::from_be_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];

        rx.recv_exact(&mut buf).await?;
        Ok(postcard::from_bytes(&buf)?)
    }

    async fn write<T: crate::stream::SendStream>(&self, tx: &mut T) -> Result<()> {
        let raw_msg = postcard::to_allocvec(self)?;
        let len = raw_msg.len() as u32;
        tx.send(&len.to_be_bytes()).await?;
        tx.send(&raw_msg).await?;
        Ok(())
    }
}

pub type DefaultReader = iroh::endpoint::RecvStream;
pub type DefaultWriter = iroh::endpoint::SendStream;

pub struct StreamPair<R: RecvStream = DefaultReader, W: SendStream = DefaultWriter> {
    reader: R,
    writer: W,
}

impl StreamPair {
    pub async fn accept(conn: &Connection) -> Result<Self> {
        let (writer, reader) = conn.accept_bi().await?;
        Ok(Self::new(reader, writer))
    }

    pub async fn open(conn: &Connection) -> Result<Self> {
        let (writer, reader) = conn.open_bi().await?;
        Ok(Self::new(reader, writer))
    }
}

impl<R: RecvStream, W: SendStream> StreamPair<R, W> {
    pub fn stream_id(&self) -> u64 {
        self.reader.id()
    }

    pub fn into_split(self) -> (R, W) {
        (self.reader, self.writer)
    }

    pub fn new(reader: R, writer: W) -> Self {
        Self { reader, writer }
    }

    pub fn tx(&mut self) -> &mut W {
        &mut self.writer
    }

    pub fn rx(&mut self) -> &mut R {
        &mut self.reader
    }

    pub async fn read<M: StreamableMessage>(&mut self) -> Result<M> {
        M::read_async(&mut self.reader).await
    }

    pub async fn write<M: StreamableMessage>(&mut self, msg: &M) -> Result<()> {
        msg.write(&mut self.writer).await
    }
}
