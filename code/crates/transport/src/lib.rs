//! Transport abstraction (F09, E1): a framed, ordered link between two peers that carries bytes,
//! whatever the medium underneath. An operation (a Work Unit, a weight slice) is serialized once
//! and handed to `send`; the medium is chosen where the link is opened.
//!
//! Two media today, both over tokio streams: TCP (`TransportKind::Tcp`) and a Unix domain socket
//! (`TransportKind::Unix`, intra-host, no network stack). Shared memory proper (E3) is still
//! pending; the Unix socket is the step before it and the same trait will carry it.
//!
//! The cluster itself still moves everything over gRPC. This crate is the layer the gRPC clients
//! will move behind; until then it is exercised by tests and `examples/bench.rs`.

use std::io;
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Tcp,
    /// Unix domain socket: same host, no network stack.
    Unix,
    SharedMemory,
    Usb,
}

/// The largest frame a peer may announce. A length above it is a broken or hostile peer, not a
/// big message, and is refused before any buffer is allocated.
pub const MAX_FRAME_BYTES: usize = 256 * 1024 * 1024;

const DATA: u8 = 0;
const PING: u8 = 1;
const PONG: u8 = 2;

/// `send`, `receive`, `probe`. A `stream` is repeated `send` / `receive` on the same link, since
/// frames are ordered and reliable. The futures are `Send` so a link can live in a spawned task.
///
/// Not cancel-safe: dropping a `receive`, `send` or `probe` future halfway (a `select!` branch, a
/// `timeout`) can leave a frame half read or half written, and the link must then be discarded.
pub trait Transport: Send {
    fn kind(&self) -> TransportKind;

    /// Sends one payload as one frame.
    fn send(&mut self, payload: &[u8]) -> impl Future<Output = io::Result<()>> + Send;

    /// Waits for the next payload. A ping from the peer is answered on the way and skipped, so the
    /// caller only ever sees data. `Ok(None)` is a clean close.
    fn receive(&mut self) -> impl Future<Output = io::Result<Option<Vec<u8>>>> + Send;

    /// Round trip of an empty frame to a peer that is inside `receive`.
    fn probe(&mut self) -> impl Future<Output = io::Result<Duration>> + Send;
}

/// A link over any ordered byte stream.
pub struct Link<S> {
    kind: TransportKind,
    stream: S,
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> Link<S> {
    pub fn new(kind: TransportKind, stream: S) -> Self {
        Self { kind, stream }
    }

    async fn write_frame(&mut self, tag: u8, payload: &[u8]) -> io::Result<()> {
        if payload.len() > MAX_FRAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "payload maior que MAX_FRAME_BYTES",
            ));
        }
        let mut header = [0u8; 5];
        header[0] = tag;
        header[1..].copy_from_slice(&(payload.len() as u32).to_be_bytes());
        self.stream.write_all(&header).await?;
        self.stream.write_all(payload).await?;
        self.stream.flush().await
    }

    /// `Ok(None)` when the peer closed between frames.
    async fn read_frame(&mut self) -> io::Result<Option<(u8, Vec<u8>)>> {
        let mut header = [0u8; 5];
        match self.stream.read_exact(&mut header).await {
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(err) => return Err(err),
        }
        let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
        if len > MAX_FRAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "frame maior que MAX_FRAME_BYTES",
            ));
        }
        // Grow with the bytes that arrive instead of trusting the announced length: a peer that
        // sends only a header must not make this side reserve `len` bytes.
        let mut payload = Vec::new();
        let read = (&mut self.stream)
            .take(len as u64)
            .read_to_end(&mut payload)
            .await?;
        if read != len {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        Ok(Some((header[0], payload)))
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> Transport for Link<S> {
    fn kind(&self) -> TransportKind {
        self.kind
    }

    async fn send(&mut self, payload: &[u8]) -> io::Result<()> {
        self.write_frame(DATA, payload).await
    }

    async fn receive(&mut self) -> io::Result<Option<Vec<u8>>> {
        loop {
            match self.read_frame().await? {
                None => return Ok(None),
                Some((DATA, payload)) => return Ok(Some(payload)),
                Some((PING, _)) => self.write_frame(PONG, &[]).await?,
                // A stray pong or an unknown tag is not data; skipping it keeps the link usable.
                Some(_) => {}
            }
        }
    }

    async fn probe(&mut self) -> io::Result<Duration> {
        let started = Instant::now();
        self.write_frame(PING, &[]).await?;
        loop {
            match self.read_frame().await? {
                None => return Err(io::ErrorKind::UnexpectedEof.into()),
                Some((PONG, _)) => return Ok(started.elapsed()),
                // The peer probing at the same time must not wait for a pong that never comes.
                Some((PING, _)) => self.write_frame(PONG, &[]).await?,
                // Data that arrives while probing is not ours to drop; the caller probes an idle link.
                Some(_) => {}
            }
        }
    }
}

pub type TcpLink = Link<tokio::net::TcpStream>;

pub async fn tcp_connect(addr: impl tokio::net::ToSocketAddrs) -> io::Result<TcpLink> {
    let stream = tokio::net::TcpStream::connect(addr).await?;
    // Small frames (probes, headers) must not wait for Nagle.
    stream.set_nodelay(true)?;
    Ok(Link::new(TransportKind::Tcp, stream))
}

pub async fn tcp_accept(listener: &tokio::net::TcpListener) -> io::Result<TcpLink> {
    let (stream, _) = listener.accept().await?;
    stream.set_nodelay(true)?;
    Ok(Link::new(TransportKind::Tcp, stream))
}

#[cfg(unix)]
pub type UnixLink = Link<tokio::net::UnixStream>;

#[cfg(unix)]
pub async fn unix_connect(path: impl AsRef<std::path::Path>) -> io::Result<UnixLink> {
    Ok(Link::new(
        TransportKind::Unix,
        tokio::net::UnixStream::connect(path).await?,
    ))
}

#[cfg(unix)]
pub async fn unix_accept(listener: &tokio::net::UnixListener) -> io::Result<UnixLink> {
    let (stream, _) = listener.accept().await?;
    Ok(Link::new(TransportKind::Unix, stream))
}

/// Answers every payload with the same payload until the peer closes. The peer for `probe` and
/// for round-trip measurements.
pub async fn echo<T: Transport>(link: &mut T) -> io::Result<()> {
    while let Some(payload) = link.receive().await? {
        link.send(&payload).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opens a connected pair over the given medium and echoes on one side.
    async fn round_trip<A: Transport + 'static, B: Transport + 'static>(
        mut client: A,
        mut server: B,
    ) {
        let task = tokio::spawn(async move { echo(&mut server).await });
        // Data of several sizes, empty included, comes back byte for byte and in order.
        for size in [0usize, 1, 4096, 3 * 1024 * 1024] {
            let payload: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
            client.send(&payload).await.unwrap();
            assert_eq!(client.receive().await.unwrap().unwrap(), payload);
        }
        // The probe is answered while the server sits in `receive`.
        assert!(client.probe().await.unwrap() > Duration::ZERO);
        drop(client);
        // A clean close ends the server loop without an error.
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn tcp_carries_payloads_and_probes() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (client, server) = tokio::join!(tcp_connect(addr), tcp_accept(&listener));
        assert_eq!(client.as_ref().unwrap().kind(), TransportKind::Tcp);
        round_trip(client.unwrap(), server.unwrap()).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_unix_socket_carries_the_same_payloads_and_probes() {
        let path = std::env::temp_dir().join(format!("aic-transport-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let (client, server) = tokio::join!(unix_connect(&path), unix_accept(&listener));
        assert_eq!(client.as_ref().unwrap().kind(), TransportKind::Unix);
        round_trip(client.unwrap(), server.unwrap()).await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn an_oversized_frame_is_refused_before_allocating() {
        let (mut raw, other) = tokio::io::duplex(64);
        let mut link = Link::new(TransportKind::Tcp, other);
        // Header of a DATA frame announcing 4 GiB - 1; nothing follows.
        raw.write_all(&[DATA, 0xFF, 0xFF, 0xFF, 0xFF])
            .await
            .unwrap();
        let err = link.receive().await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn the_close_of_a_peer_between_frames_is_not_an_error() {
        let (raw, other) = tokio::io::duplex(64);
        let mut link = Link::new(TransportKind::Tcp, other);
        drop(raw);
        assert_eq!(link.receive().await.unwrap(), None);
    }
}
