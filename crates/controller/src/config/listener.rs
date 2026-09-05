use std::io;
use std::net::SocketAddr;

use axum::Router;
use futures_util::StreamExt;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

pub struct PreparedListener {
    listener: TcpListener,
    address: SocketAddr,
}

impl PreparedListener {
    /// Prebinds a TCP listener for a configuration handoff.
    ///
    /// # Errors
    /// Returns an error when the address cannot be bound or inspected.
    pub async fn bind(address: SocketAddr) -> io::Result<Self> {
        let listener = TcpListener::bind(address).await?;
        let address = listener.local_addr()?;
        Ok(Self { listener, address })
    }

    #[must_use]
    pub const fn address(&self) -> SocketAddr {
        self.address
    }
}

struct Rebind {
    prepared: PreparedListener,
    applied: oneshot::Sender<()>,
}

#[derive(Clone)]
pub struct ListenerHandle {
    sender: mpsc::Sender<Rebind>,
}

pub struct ListenerReceiver {
    receiver: mpsc::Receiver<Rebind>,
}

#[must_use]
pub fn listener_channel() -> (ListenerHandle, ListenerReceiver) {
    let (sender, receiver) = mpsc::channel(1);
    (ListenerHandle { sender }, ListenerReceiver { receiver })
}

impl ListenerHandle {
    /// Hands a prebound listener to the running HTTP server generation.
    ///
    /// # Errors
    /// Returns an error when the HTTP listener loop has stopped.
    pub async fn handoff(&self, prepared: PreparedListener) -> io::Result<()> {
        let (applied, waiting) = oneshot::channel();
        self.sender
            .send(Rebind { prepared, applied })
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "HTTP listener stopped"))?;
        waiting
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "HTTP listener stopped"))
    }
}

/// Serves one router while accepting prebound listener replacements.
///
/// # Errors
/// Returns an error when a server generation fails or all listeners exit unexpectedly.
pub async fn serve_reconfigurable(
    initial: PreparedListener,
    router: Router,
    mut rebinds: ListenerReceiver,
    shutdown: CancellationToken,
) -> io::Result<()> {
    let mut servers = JoinSet::new();
    let mut generation = CancellationToken::new();
    spawn_server(&mut servers, initial, router.clone(), generation.clone());
    loop {
        tokio::select! {
            biased;
            () = shutdown.cancelled() => {
                generation.cancel();
                while let Some(result) = servers.join_next().await {
                    result.map_err(io::Error::other)??;
                }
                return Ok(());
            }
            Some(rebind) = rebinds.receiver.recv() => {
                generation.cancel();
                generation = CancellationToken::new();
                spawn_server(&mut servers, rebind.prepared, router.clone(), generation.clone());
                let _ = rebind.applied.send(());
            }
            Some(result) = servers.join_next() => {
                result.map_err(io::Error::other)??;
                if servers.is_empty() {
                    return Err(io::Error::new(io::ErrorKind::BrokenPipe, "HTTP listener exited"));
                }
            }
        }
    }
}

fn spawn_server(
    servers: &mut JoinSet<io::Result<()>>,
    prepared: PreparedListener,
    router: Router,
    shutdown: CancellationToken,
) {
    servers.spawn(async move {
        let stream_shutdown = shutdown.clone();
        let router = router.layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let stream_shutdown = stream_shutdown.clone();
                async move {
                    let response = next.run(request).await;
                    if response.headers().get(axum::http::header::CONTENT_TYPE)
                        == Some(&axum::http::HeaderValue::from_static("text/event-stream"))
                    {
                        let (parts, body) = response.into_parts();
                        let stream = body.into_data_stream()
                            .take_until(stream_shutdown.cancelled_owned());
                        axum::response::Response::from_parts(parts, axum::body::Body::from_stream(stream))
                    } else {
                        response
                    }
                }
            },
        ));
        axum::serve(
            prepared.listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown.cancelled_owned())
        .await
    });
}
