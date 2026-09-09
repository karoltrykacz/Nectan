use anyhow::Result;
use bytes::{Buf, Bytes, BytesMut};
use iroh::endpoint::{self, ReadExactError, VarInt};
use serde::de::DeserializeOwned;
use std::{
    future::Future,
    io::{self, Cursor},
    ops::{Deref, DerefMut},
    os::unix::fs::MetadataExt,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use crate::messages::NetMessage;

type DefaultReader = iroh::endpoint::RecvStream;
type DefaultWriter = iroh::endpoint::SendStream;

#[derive(Debug)]
pub struct StreamPair<R: RecvStream = DefaultReader, W: SendStream = DefaultWriter> {
    reader: R,
    writer: W,
}

impl StreamPair {
    pub async fn accept(
        conn: &endpoint::Connection,
        // events: EventSender,
    ) -> Result<Self> {
        let (writer, reader) = conn.accept_bi().await?;
        Ok(Self::new(reader, writer))
    }
}

impl<R: RecvStream, W: SendStream> StreamPair<R, W> {
    pub fn stream_id(&self) -> u64 {
        self.reader.id()
    }

    pub fn new(reader: R, writer: W) -> Self {
        Self { reader, writer }
    }

    pub fn tx(&mut self) -> &mut W {
        &mut self.writer
    }

    pub async fn read_request(&mut self) -> Result<NetMessage> {
        Ok(NetMessage::read_async(&mut self.reader).await?)
        // self.other_bytes_read += size as u64;
    }
}

/// An abstract `iroh::endpoint::SendStream`.
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

impl SendStream for iroh::endpoint::SendStream {
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

    fn id(&self) -> u64 {
        self.id().index()
    }

    fn finish(&mut self) -> io::Result<()> {
        Ok(iroh::endpoint::SendStream::finish(self)?)
    }
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

    async fn recv_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.read_exact(buf).await.map_err(|e| match e {
            ReadExactError::FinishedEarly(0) => io::Error::new(io::ErrorKind::UnexpectedEof, ""),
            ReadExactError::FinishedEarly(_) => io::Error::new(io::ErrorKind::InvalidData, ""),
            ReadExactError::ReadError(e) => e.into(),
        })
    }

    fn stop(&mut self, code: VarInt) -> io::Result<()> {
        Ok(self.stop(code)?)
    }

    fn id(&self) -> u64 {
        self.id().index()
    }
}

impl<R: RecvStream> RecvStream for &mut R {
    async fn recv_bytes(&mut self, len: usize) -> io::Result<Bytes> {
        self.deref_mut().recv_bytes(len).await
    }

    async fn recv_bytes_exact(&mut self, len: usize) -> io::Result<Bytes> {
        self.deref_mut().recv_bytes_exact(len).await
    }

    async fn recv_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.deref_mut().recv_exact(buf).await
    }

    fn stop(&mut self, code: VarInt) -> io::Result<()> {
        self.deref_mut().stop(code)
    }

    fn id(&self) -> u64 {
        self.deref().id()
    }
}

impl<W: SendStream> SendStream for &mut W {
    async fn send_bytes(&mut self, bytes: Bytes) -> io::Result<()> {
        self.deref_mut().send_bytes(bytes).await
    }

    async fn send(&mut self, buf: &[u8]) -> io::Result<()> {
        self.deref_mut().send(buf).await
    }

    async fn sync(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn reset(&mut self, code: VarInt) -> io::Result<()> {
        self.deref_mut().reset(code)
    }

    async fn stopped(&mut self) -> io::Result<Option<VarInt>> {
        self.deref_mut().stopped().await
    }

    fn id(&self) -> u64 {
        self.deref().id()
    }

    fn finish(&mut self) -> io::Result<()> {
        self.deref_mut().finish()
    }
}
