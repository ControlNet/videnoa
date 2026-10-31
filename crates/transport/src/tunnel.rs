use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use iroh::{endpoint::Connection, Endpoint, EndpointAddr, EndpointId};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::net::{TcpListener, TcpSocket};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::{Identity, Network};

const ALPN: &[u8] = iroh_proxy_utils::ALPN;
const TARGET: &str = "videnoa.internal:80";
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_HEADERS: usize = 8192;

pub type Authorizer = Arc<
    dyn Fn(EndpointId, String) -> Pin<Box<dyn Future<Output = Result<(), TunnelError>> + Send>>
        + Send
        + Sync,
>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TunnelError {
    Unauthorized,
    RateLimited,
    Unavailable,
    Protocol,
}
impl std::fmt::Display for TunnelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unauthorized => "worker tunnel password is missing or incorrect",
            Self::RateLimited => "worker tunnel authentication is rate limited",
            Self::Unavailable => "worker tunnel is unavailable",
            Self::Protocol => "worker tunnel protocol is incompatible",
        })
    }
}
impl std::error::Error for TunnelError {}

/// Metadata is populated by the dialer before connecting to the private API listener.
#[derive(Clone, Default)]
pub struct PeerMap(Arc<Mutex<HashMap<SocketAddr, EndpointId>>>);
impl PeerMap {
    pub fn get(&self, address: SocketAddr) -> Option<EndpointId> {
        self.0.lock().ok()?.get(&address).copied()
    }
}
struct PeerGuard(PeerMap, SocketAddr);
impl Drop for PeerGuard {
    fn drop(&mut self) {
        if let Ok(mut peers) = self.0 .0.lock() {
            peers.remove(&self.1);
        }
    }
}

pub struct Server {
    endpoint: Endpoint,
    stop: CancellationToken,
    tasks: TaskTracker,
}
impl Server {
    pub async fn start(
        identity: &Identity,
        target: SocketAddr,
        auth: Authorizer,
        peers: PeerMap,
        network: &Network,
    ) -> Result<Self> {
        let endpoint = network
            .builder()
            .secret_key(identity.key.clone())
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await?;
        Self::with_endpoint(endpoint, target, auth, peers)
    }

    fn with_endpoint(
        endpoint: Endpoint,
        target: SocketAddr,
        auth: Authorizer,
        peers: PeerMap,
    ) -> Result<Self> {
        anyhow::ensure!(
            target.ip().is_loopback(),
            "iroh target must be the private loopback API"
        );
        endpoint.set_alpns(vec![ALPN.to_vec()]);
        let stop = CancellationToken::new();
        let tasks = TaskTracker::new();
        let endpoint_task = endpoint.clone();
        let stop_task = stop.clone();
        let tracker = tasks.clone();
        let connections = Arc::new(Semaphore::new(64));
        let streams = Arc::new(Semaphore::new(256));
        tasks.spawn(async move {
            loop {
                let incoming = tokio::select! {
                    _ = stop_task.cancelled() => break,
                    incoming = endpoint_task.accept() => match incoming { Some(v) => v, None => break },
                };
                let Ok(permit) = connections.clone().try_acquire_owned() else { incoming.refuse(); continue; };
                let auth = auth.clone();
                let peers = peers.clone();
                let stop = stop_task.clone();
                let streams = streams.clone();
                let tracker_inner = tracker.clone();
                tracker.spawn(async move {
                    let _permit = permit;
                    let connected = tokio::select! {
                        _ = stop.cancelled() => return,
                        result = tokio::time::timeout(HANDSHAKE_TIMEOUT, incoming) => result,
                    };
                    let Ok(Ok(conn)) = connected else { return; };
                    loop {
                        let stream = tokio::select! {
                            _ = stop.cancelled() => break,
                            stream = conn.accept_bi() => match stream { Ok(v) => v, Err(_) => break },
                        };
                        let Ok(permit) = streams.clone().try_acquire_owned() else { drop(stream); continue; };
                        let auth = auth.clone();
                        let peers = peers.clone();
                        let stop = stop.clone();
                        let remote = conn.remote_id();
                        tracker_inner.spawn(async move {
                            let _permit = permit;
                            tokio::select! {
                                _ = stop.cancelled() => {},
                                _ = serve_stream(stream, remote, target, auth, peers) => {},
                            }
                        });
                    }
                    conn.close(0u32.into(), b"service stopped");
                });
            }
        });
        Ok(Self {
            endpoint,
            stop,
            tasks,
        })
    }

    pub fn id(&self) -> EndpointId {
        self.endpoint.id()
    }
    pub fn addr(&self) -> EndpointAddr {
        self.endpoint.addr()
    }
    pub async fn shutdown(&self) {
        self.stop.cancel();
        self.endpoint.close().await;
        self.tasks.close();
        self.tasks.wait().await;
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

async fn read_headers(reader: &mut (impl AsyncRead + Unpin)) -> Result<Vec<u8>> {
    // Read exactly the header boundary; never consume application bytes after CONNECT.
    let mut bytes = Vec::with_capacity(256);
    while bytes.len() < MAX_HEADERS {
        bytes.push(reader.read_u8().await?);
        if bytes.ends_with(b"\r\n\r\n") {
            return Ok(bytes);
        }
    }
    anyhow::bail!("tunnel header limit exceeded")
}

async fn serve_stream(
    (mut send, mut recv): (iroh::endpoint::SendStream, iroh::endpoint::RecvStream),
    remote: EndpointId,
    target: SocketAddr,
    auth: Authorizer,
    peers: PeerMap,
) -> Result<()> {
    let admission = tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
        let bytes = read_headers(&mut recv)
            .await
            .map_err(|_| TunnelError::Protocol)?;
        let request = iroh_proxy_utils::HttpRequest::parse(&bytes)
            .map_err(|_| TunnelError::Protocol)?
            .ok_or(TunnelError::Protocol)?;
        if request.method != http::Method::CONNECT
            || request.uri != TARGET
            || request
                .headers
                .contains_key(http::header::TRANSFER_ENCODING)
            || request.headers.contains_key(http::header::CONTENT_LENGTH)
        {
            return Err(TunnelError::Protocol);
        }
        let values = request.headers.get_all(http::header::PROXY_AUTHORIZATION);
        if values.iter().count() != 1 {
            return Err(TunnelError::Unauthorized);
        }
        let password = values
            .iter()
            .next()
            .and_then(|v| std::str::from_utf8(v.as_bytes()).ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .filter(|v| !v.is_empty() && v.len() <= 1024 && !v.chars().any(char::is_control))
            .ok_or(TunnelError::Unauthorized)?;
        auth(remote, password.to_owned()).await
    })
    .await
    .unwrap_or(Err(TunnelError::Unavailable));
    if let Err(error) = admission {
        let status = match error {
            TunnelError::Unauthorized => 403,
            TunnelError::RateLimited => 429,
            TunnelError::Protocol => 400,
            TunnelError::Unavailable => 503,
        };
        let response = format!("HTTP/1.1 {status} Tunnel rejected\r\nContent-Length: 0\r\n\r\n");
        respond(&mut send, response.as_bytes()).await?;
        send.finish()?;
        return Ok(());
    }
    let socket = if target.is_ipv4() {
        TcpSocket::new_v4()?
    } else {
        TcpSocket::new_v6()?
    };
    socket.bind(SocketAddr::new(target.ip(), 0))?;
    let local = socket.local_addr()?;
    peers
        .0
        .lock()
        .map_err(|_| anyhow::anyhow!("peer map unavailable"))?
        .insert(local, remote);
    let _guard = PeerGuard(peers, local);
    let mut tcp = tokio::time::timeout(HANDSHAKE_TIMEOUT, socket.connect(target)).await??;
    respond(&mut send, b"HTTP/1.1 200 Connection Established\r\n\r\n").await?;
    let mut stream = tokio::io::join(recv, send);
    tokio::io::copy_bidirectional(&mut stream, &mut tcp).await?;
    Ok(())
}

/// Writes a CONNECT response within the handshake deadline. A peer that grants
/// no stream credit would otherwise stall the write, and the stream permit with
/// it, forever; on timeout the stream is reset so the task can end.
async fn respond(send: &mut iroh::endpoint::SendStream, response: &[u8]) -> Result<()> {
    match tokio::time::timeout(HANDSHAKE_TIMEOUT, send.write_all(response)).await {
        Ok(written) => Ok(written?),
        Err(_) => {
            let _ = send.reset(0u32.into());
            anyhow::bail!("tunnel peer did not accept the CONNECT response")
        }
    }
}

/// Whether a QUIC handshake failed because the peer runs another tunnel
/// protocol: it refused the tunnel ALPN, or shares no QUIC version.
fn incompatible_handshake(error: &iroh::endpoint::ConnectError) -> bool {
    use iroh::endpoint::{ConnectError, ConnectingError, ConnectionError, TransportErrorCode};
    let connection = match error {
        ConnectError::Connecting {
            source: ConnectingError::ConnectionError { source, .. },
            ..
        }
        | ConnectError::Connection { source, .. } => source,
        _ => return false,
    };
    match connection {
        ConnectionError::VersionMismatch => true,
        // TLS alert 120, `no_application_protocol`: the ALPN was refused.
        ConnectionError::ConnectionClosed(close) => {
            close.error_code == TransportErrorCode::crypto(120)
        }
        _ => false,
    }
}

/// Shared controller endpoint; one persistent identity and connection per peer.
type ConnectionSlot = Arc<tokio::sync::Mutex<Option<Connection>>>;

pub struct Client {
    endpoint: Endpoint,
    connections: tokio::sync::Mutex<HashMap<EndpointId, ConnectionSlot>>,
    network: Network,
    _identity: Identity,
}
impl Client {
    pub async fn open(root: &std::path::Path, network: &Network) -> Result<Self> {
        let identity = Identity::open(root)?;
        let endpoint = network
            .builder()
            .secret_key(identity.key.clone())
            .bind()
            .await?;
        Ok(Self {
            endpoint,
            connections: Default::default(),
            network: network.clone(),
            _identity: identity,
        })
    }
    pub async fn shutdown(&self) {
        self.endpoint.close().await;
    }
    pub async fn tunnel(
        &self,
        peer: impl Into<EndpointAddr>,
        password: &str,
    ) -> Result<impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static> {
        let peer = self.network.dial_address(peer.into());
        let slot = {
            let mut connections = self.connections.lock().await;
            connections.entry(peer.id).or_default().clone()
        };
        let conn = {
            let mut stored = slot.lock().await;
            if let Some(conn) = stored.as_ref().filter(|c| c.close_reason().is_none()) {
                conn.clone()
            } else {
                let conn =
                    tokio::time::timeout(HANDSHAKE_TIMEOUT, self.endpoint.connect(peer, ALPN))
                        .await
                        .context("iroh connection timeout")?
                        .map_err(|error| {
                            let incompatible = incompatible_handshake(&error);
                            let error = anyhow::Error::from(error);
                            if incompatible {
                                error.context(TunnelError::Protocol)
                            } else {
                                error
                            }
                        })?;
                *stored = Some(conn.clone());
                conn
            }
        };
        tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
            let value = http::HeaderValue::from_bytes(format!("Bearer {password}").as_bytes())
                .map_err(|_| TunnelError::Unauthorized)?;
            let (mut send, mut recv) = conn.open_bi().await?;
            send.write_all(
                format!("CONNECT {TARGET} HTTP/1.1\r\nHost: {TARGET}\r\nProxy-Authorization: ")
                    .as_bytes(),
            )
            .await?;
            send.write_all(value.as_bytes()).await?;
            send.write_all(b"\r\n\r\n").await?;
            let bytes = read_headers(&mut recv).await?;
            let response =
                iroh_proxy_utils::HttpResponse::parse(&bytes)?.ok_or(TunnelError::Protocol)?;
            match response.status.as_u16() {
                200 => Ok(tokio::io::join(recv, send)),
                // The worker could not parse this CONNECT: another tunnel dialect.
                400 => Err(TunnelError::Protocol.into()),
                401 | 403 => Err(TunnelError::Unauthorized.into()),
                429 => Err(TunnelError::RateLimited.into()),
                _ => Err(TunnelError::Unavailable.into()),
            }
        })
        .await
        .context("iroh authentication timeout")?
    }

    /// Binds a local listener without placing credentials in URLs or logs.
    pub async fn forward(
        self: Arc<Self>,
        listener: TcpListener,
        peer: EndpointId,
        password: String,
        stop: CancellationToken,
        failure: Arc<Mutex<Option<TunnelError>>>,
    ) {
        let tasks = TaskTracker::new();
        let slots = Arc::new(Semaphore::new(64));
        loop {
            let accepted = tokio::select! { _ = stop.cancelled() => break, value = listener.accept() => value };
            let Ok((mut tcp, _)) = accepted else {
                break;
            };
            let Ok(permit) = slots.clone().try_acquire_owned() else {
                continue;
            };
            let failure = failure.clone();
            let client = self.clone();
            let password = password.clone();
            let stop = stop.clone();
            tasks.spawn(async move {
                let _permit = permit;
                tokio::select! {
                    _ = stop.cancelled() => {},
                    _ = async {
                        match client.tunnel(peer, &password).await {
                            Ok(mut stream) => {
                                if let Ok(mut last) = failure.lock() { *last = None; }
                                let _ = tokio::io::copy_bidirectional(&mut stream, &mut tcp).await;
                            }
                            Err(error) => {
                                if let Ok(mut last) = failure.lock() {
                                    *last = Some(error.downcast_ref::<TunnelError>().copied().unwrap_or(TunnelError::Unavailable));
                                }
                            }
                        }
                    } => {},
                }
            });
        }
        tasks.close();
        tasks.wait().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn authentication_rotation_and_shutdown_preserve_tcp_semantics() -> Result<()> {
        // These credentials and payloads are synthetic test fixtures only.
        let current = Arc::new(Mutex::new(String::from("test transport credential one")));
        let verifier = current.clone();
        let auth: Authorizer = Arc::new(move |_, supplied| {
            let valid = verifier.lock().unwrap().as_str() == supplied;
            Box::pin(async move {
                if valid {
                    Ok(())
                } else {
                    Err(TunnelError::Unauthorized)
                }
            })
        });
        let tcp = TcpListener::bind("127.0.0.1:0").await?;
        let target = tcp.local_addr()?;
        let endpoint = Endpoint::bind(iroh::endpoint::presets::Minimal).await?;
        let server = Server::with_endpoint(endpoint, target, auth, PeerMap::default())?;
        let root = tempfile::tempdir()?;
        let identity = Identity::open(root.path())?;
        let client = Client {
            endpoint: Endpoint::bind(iroh::endpoint::presets::Minimal).await?,
            connections: Default::default(),
            network: Network::default(),
            _identity: identity,
        };
        let address = server.addr();
        let rejected = client
            .tunnel(address.clone(), "wrong test credential")
            .await;
        assert!(matches!(
            rejected
                .err()
                .and_then(|e| e.downcast::<TunnelError>().ok()),
            Some(TunnelError::Unauthorized)
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(50), tcp.accept())
                .await
                .is_err()
        );
        let echo = tokio::spawn(async move {
            let (mut tcp, _) = tcp.accept().await?;
            let (mut read, mut write) = tcp.split();
            tokio::io::copy(&mut read, &mut write).await
        });
        let mut stream = client
            .tunnel(address.clone(), "test transport credential one")
            .await?;
        stream.write_all(b"first").await?;
        let mut result = [0; 5];
        stream.read_exact(&mut result).await?;
        assert_eq!(&result, b"first");
        *current.lock().unwrap() = "test transport credential two".into();
        assert!(client
            .tunnel(address, "test transport credential one")
            .await
            .is_err());
        stream.write_all(b"later").await?;
        stream.read_exact(&mut result).await?;
        assert_eq!(&result, b"later");
        server.shutdown().await;
        let ended = tokio::time::timeout(Duration::from_secs(2), stream.read_u8()).await?;
        assert!(ended.is_err());
        client.shutdown().await;
        let _ = echo.await?;
        Ok(())
    }
}

#[cfg(test)]
mod streaming_tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn sse_is_delivered_incrementally_and_arbitrary_targets_are_rejected() -> Result<()> {
        // Test-only SSE origin, fixed password and events.
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let target = listener.local_addr()?;
        let auth: Authorizer = Arc::new(|_, password| {
            Box::pin(async move {
                if password == "sse test credential" {
                    Ok(())
                } else {
                    Err(TunnelError::Unauthorized)
                }
            })
        });
        let server = Server::with_endpoint(
            Endpoint::bind(iroh::endpoint::presets::Minimal).await?,
            target,
            auth,
            PeerMap::default(),
        )?;
        let root = tempfile::tempdir()?;
        let client = Client {
            endpoint: Endpoint::bind(iroh::endpoint::presets::Minimal).await?,
            connections: Default::default(),
            network: Network::default(),
            _identity: Identity::open(root.path())?,
        };
        let conn = client.endpoint.connect(server.addr(), ALPN).await?;
        let (mut send, mut recv) = conn.open_bi().await?;
        send.write_all(b"CONNECT forbidden.internal:80 HTTP/1.1\r\nHost: forbidden.internal\r\nProxy-Authorization: Bearer sse test credential\r\n\r\n").await?;
        assert!(read_headers(&mut recv).await?.starts_with(b"HTTP/1.1 400"));
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
        let proceed = Arc::new(tokio::sync::Notify::new());
        let notified = proceed.clone();
        let origin = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let _ = read_headers(&mut stream).await?;
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: first\n\n").await?;
            notified.notified().await;
            stream.write_all(b"data: second\n\n").await?;
            anyhow::Ok(())
        });
        let mut stream = client.tunnel(server.addr(), "sse test credential").await?;
        stream
            .write_all(b"GET /events HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await?;
        let _ = read_headers(&mut stream).await?;
        let mut event = [0; 13];
        tokio::time::timeout(Duration::from_secs(2), stream.read_exact(&mut event)).await??;
        assert_eq!(&event, b"data: first\n\n");
        // Origin is still open: receiving this event proves no whole-response buffering.
        proceed.notify_one();
        let mut rest = Vec::new();
        stream.read_to_end(&mut rest).await?;
        assert_eq!(&rest, b"data: second\n\n");
        origin.await??;
        client.shutdown().await;
        server.shutdown().await;
        Ok(())
    }
}

#[cfg(test)]
mod public_network_tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    #[ignore = "requires outbound access to public N0 discovery and relays"]
    async fn n0_endpoint_id_only_connection() -> Result<()> {
        // Ephemeral test identities and test-only password; no production peers.
        let worker_root = tempfile::tempdir()?;
        let controller_root = tempfile::tempdir()?;
        let identity = Identity::open(worker_root.path())?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let target = listener.local_addr()?;
        let auth: Authorizer = Arc::new(|_, value| {
            Box::pin(async move {
                if value == "public network test credential" {
                    Ok(())
                } else {
                    Err(TunnelError::Unauthorized)
                }
            })
        });
        let server = Server::start(
            &identity,
            target,
            auth,
            PeerMap::default(),
            &Network::default(),
        )
        .await?;
        tokio::time::timeout(Duration::from_secs(30), server.endpoint.online()).await?;
        let client = Client::open(controller_root.path(), &Network::default()).await?;
        let origin = tokio::spawn(async move {
            let (mut tcp, _) = listener.accept().await?;
            tcp.write_all(b"n0-test").await?;
            anyhow::Ok(())
        });
        // Pass only the public ID: no relay URL or direct address hints.
        let mut stream = client
            .tunnel(server.id(), "public network test credential")
            .await?;
        let mut value = [0; 7];
        stream.read_exact(&mut value).await?;
        assert_eq!(&value, b"n0-test");
        origin.await??;
        client.shutdown().await;
        server.shutdown().await;
        Ok(())
    }
}

#[cfg(test)]
mod permit_exhaustion_tests {
    use super::*;
    use iroh::endpoint::{presets, QuicTransportConfig, VarInt};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn rejected_connects_to_a_zero_window_peer_release_their_stream_permits() -> Result<()> {
        // Given: a worker tunnel whose loopback target echoes, and a synthetic
        // test credential.
        const PASSWORD: &str = "permit exhaustion test credential";
        let auth: Authorizer = Arc::new(|_, supplied| {
            Box::pin(async move {
                if supplied == PASSWORD {
                    Ok(())
                } else {
                    Err(TunnelError::Unauthorized)
                }
            })
        });
        let target_listener = TcpListener::bind("127.0.0.1:0").await?;
        let target = target_listener.local_addr()?;
        tokio::spawn(async move {
            while let Ok((mut tcp, _)) = target_listener.accept().await {
                tokio::spawn(async move {
                    let (mut r, mut w) = tcp.split();
                    let _ = tokio::io::copy(&mut r, &mut w).await;
                });
            }
        });
        let server = Server::with_endpoint(
            Endpoint::bind(presets::Minimal).await?,
            target,
            auth,
            PeerMap::default(),
        )?;
        let server_addr = server.addr();

        // When: a peer without the password advertises a zero stream receive
        // window, so no rejection can ever reach it, and sends more
        // unauthenticated CONNECTs than there are stream permits. Its handles
        // stay alive, so the rejection writes stay blocked.
        let attacker = Endpoint::builder(presets::Minimal)
            .transport_config(
                QuicTransportConfig::builder()
                    .stream_receive_window(VarInt::from_u32(0))
                    .build(),
            )
            .bind()
            .await?;
        let mut conns = Vec::new();
        let mut streams = Vec::new();
        for _ in 0..4 {
            let conn = attacker.connect(server_addr.clone(), ALPN).await?;
            for _ in 0..80 {
                let (mut send, recv) = conn.open_bi().await?;
                send.write_all(
                    format!("CONNECT {TARGET} HTTP/1.1\r\nHost: {TARGET}\r\n\r\n").as_bytes(),
                )
                .await?;
                streams.push((send, recv));
            }
            conns.push(conn);
        }

        // Then: once the stalled rejections time out, a correctly
        // authenticated tunnel works again while the attacker is still
        // connected.
        let root = tempfile::tempdir()?;
        let legit = Client {
            endpoint: Endpoint::bind(presets::Minimal).await?,
            connections: Default::default(),
            network: Network::default(),
            _identity: Identity::open(root.path())?,
        };
        let deadline = tokio::time::Instant::now() + HANDSHAKE_TIMEOUT * 3;
        let mut stream = loop {
            match legit.tunnel(server_addr.clone(), PASSWORD).await {
                Ok(stream) => break stream,
                Err(_) if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                Err(error) => {
                    return Err(error.context("stream permits stayed exhausted"));
                }
            }
        };
        stream.write_all(b"pong!").await?;
        let mut buf = [0u8; 5];
        stream.read_exact(&mut buf).await?;
        assert_eq!(&buf, b"pong!");

        drop(streams);
        drop(conns);
        attacker.close().await;
        legit.shutdown().await;
        server.shutdown().await;
        Ok(())
    }
}

#[cfg(test)]
mod incompatibility_tests {
    use super::*;
    use iroh::endpoint::presets;

    // Ephemeral identities and a test-only credential; no production peers.
    const PASSWORD: &str = "incompatibility test credential";

    async fn client(root: &std::path::Path) -> Result<Client> {
        Ok(Client {
            endpoint: Endpoint::bind(presets::Minimal).await?,
            connections: Default::default(),
            network: Network::default(),
            _identity: Identity::open(root)?,
        })
    }

    #[tokio::test]
    async fn a_worker_that_rejects_the_connect_request_is_incompatible() -> Result<()> {
        // Given: a peer on the tunnel ALPN that answers every CONNECT with
        // 400, as a worker with a different tunnel dialect does.
        let worker = Endpoint::builder(presets::Minimal)
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await?;
        let address = worker.addr();
        let acceptor = worker.clone();
        tokio::spawn(async move {
            while let Some(incoming) = acceptor.accept().await {
                let Ok(conn) = incoming.await else { continue };
                while let Ok((mut send, mut recv)) = conn.accept_bi().await {
                    let _ = read_headers(&mut recv).await;
                    let _ = send
                        .write_all(b"HTTP/1.1 400 Tunnel rejected\r\nContent-Length: 0\r\n\r\n")
                        .await;
                    let _ = send.finish();
                }
            }
        });
        let root = tempfile::tempdir()?;
        let client = client(root.path()).await?;

        // When: the Controller opens a tunnel.
        let error = client.tunnel(address, PASSWORD).await.err();

        // Then: the rejection is reported as incompatibility, not an outage.
        assert_eq!(
            error.as_ref().and_then(|e| e.downcast_ref::<TunnelError>()),
            Some(&TunnelError::Protocol),
            "{error:?}"
        );
        client.shutdown().await;
        worker.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn a_worker_without_the_tunnel_alpn_is_incompatible() -> Result<()> {
        // Given: a reachable peer that accepts only another ALPN, as a worker
        // built against a different tunnel protocol does.
        let worker = Endpoint::builder(presets::Minimal)
            .alpns(vec![b"videnoa/test-other-tunnel".to_vec()])
            .bind()
            .await?;
        let address = worker.addr();
        let acceptor = worker.clone();
        tokio::spawn(async move {
            while let Some(incoming) = acceptor.accept().await {
                let _ = incoming.await;
            }
        });
        let root = tempfile::tempdir()?;
        let client = client(root.path()).await?;

        // When: the Controller opens a tunnel.
        let error = client.tunnel(address, PASSWORD).await.err();

        // Then: the failed ALPN negotiation is reported as incompatibility.
        assert_eq!(
            error.as_ref().and_then(|e| e.downcast_ref::<TunnelError>()),
            Some(&TunnelError::Protocol),
            "{error:?}"
        );
        client.shutdown().await;
        worker.close().await;
        Ok(())
    }
}

#[cfg(test)]
mod relay_tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn self_hosted_relay_connects_by_endpoint_id_without_public_services() -> Result<()> {
        // Given: a local relay with a self-signed test certificate, and
        // ephemeral test identities that know only that relay.
        let (_relay_map, relay_url, _relay) = iroh::test_utils::run_relay_server().await?;
        let network = test_network(&[relay_url.as_str()])?;

        // When/Then: the Controller side dials by Endpoint ID alone, with no
        // address lookup configured, and reaches the worker through the relay.
        relayed_round_trip(&network, &network).await
    }

    #[tokio::test]
    async fn dialing_reaches_a_worker_on_any_listed_relay() -> Result<()> {
        // Given: a worker homed on relay A, and a Controller that lists
        // another relay first, as with a self-hosted relay plus public ones.
        let (_map_a, relay_a, _relay_a) = iroh::test_utils::run_relay_server().await?;
        let (_map_b, relay_b, _relay_b) = iroh::test_utils::run_relay_server().await?;
        let worker = test_network(&[relay_a.as_str()])?;
        let controller = test_network(&[relay_b.as_str(), relay_a.as_str()])?;

        // When/Then: every listed relay is a dialing hint, so the worker's
        // relay is found even though it is not the Controller's first.
        relayed_round_trip(&worker, &controller).await
    }

    #[tokio::test]
    async fn an_unreachable_relay_falls_back_to_the_next_listed_one() -> Result<()> {
        // Given: both sides list a relay that is down (nothing listens on the
        // test-only discard port) ahead of a working one.
        let (_relay_map, relay_url, _relay) = iroh::test_utils::run_relay_server().await?;
        let network = test_network(&["http://127.0.0.1:9", relay_url.as_str()])?;

        // When/Then: the endpoints home on the working relay and connect.
        relayed_round_trip(&network, &network).await
    }

    fn test_network(urls: &[&str]) -> Result<Network> {
        Ok(Network::with_relays(urls)?.with_insecure_test_relay_tls())
    }

    /// Starts a worker on `worker_network`, dials it by Endpoint ID from a
    /// client on `controller_network`, and reads bytes from the worker's target.
    async fn relayed_round_trip(
        worker_network: &Network,
        controller_network: &Network,
    ) -> Result<()> {
        let worker_root = tempfile::tempdir()?;
        let controller_root = tempfile::tempdir()?;
        let identity = Identity::open(worker_root.path())?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let target = listener.local_addr()?;
        let auth: Authorizer = Arc::new(|_, value| {
            Box::pin(async move {
                if value == "relay test credential" {
                    Ok(())
                } else {
                    Err(TunnelError::Unauthorized)
                }
            })
        });
        let server =
            Server::start(&identity, target, auth, PeerMap::default(), worker_network).await?;
        tokio::time::timeout(Duration::from_secs(10), server.endpoint.online()).await?;
        assert!(server
            .addr()
            .relay_urls()
            .all(|url| worker_network.relays().contains(url)));
        let client = Client::open(controller_root.path(), controller_network).await?;
        let origin = tokio::spawn(async move {
            let (mut tcp, _) = listener.accept().await?;
            tcp.write_all(b"relayed").await?;
            anyhow::Ok(())
        });
        let mut stream = tokio::time::timeout(
            Duration::from_secs(20),
            client.tunnel(server.id(), "relay test credential"),
        )
        .await??;
        let mut value = [0; 7];
        stream.read_exact(&mut value).await?;
        assert_eq!(&value, b"relayed");
        origin.await??;
        client.shutdown().await;
        server.shutdown().await;
        Ok(())
    }
}
