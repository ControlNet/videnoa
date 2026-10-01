//! HTTP accept loop that bounds how long a peer may take to send a request head.
//!
//! `axum::serve` builds hyper's connection builder without a timer, so hyper's
//! HTTP/1 header read timeout never fires and a peer can hold a socket open by
//! dribbling header bytes forever. This module mirrors the `axum::serve` loop
//! (axum 0.8.8, `src/serve/mod.rs`) with a Tokio timer and an explicit header
//! read timeout. Only the request head is bounded: request bodies, handlers,
//! long-lived responses such as SSE, and upgraded WebSocket connections are not.
//!
//! Connections speak HTTP/1 only. hyper-util's auto builder would also accept
//! cleartext HTTP/2 (h2c), which no client of ours uses (browsers use HTTP/2
//! only over TLS) and which the request-head deadline does not cover.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::pin;
use std::time::Duration;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use axum::Router;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tower::ServiceExt as _;

/// Longest time a connection may spend sending one HTTP/1 request head,
/// including the head of the next request on an idle keep-alive connection.
pub const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Pause after a non-transient accept error such as `EMFILE`, matching axum.
const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_secs(1);

/// Serves a router on a TCP listener with a bounded request-head read.
///
/// Every request carries [`ConnectInfo<SocketAddr>`] for the peer address, so
/// `ConnectInfo` extractors work as with
/// `Router::into_make_service_with_connect_info::<SocketAddr>()`.
pub struct HttpServer {
    listener: TcpListener,
    router: Router,
    header_read_timeout: Duration,
}

impl HttpServer {
    #[must_use]
    pub fn new(listener: TcpListener, router: Router) -> Self {
        Self {
            listener,
            router,
            header_read_timeout: HEADER_READ_TIMEOUT,
        }
    }

    /// Overrides [`HEADER_READ_TIMEOUT`], for example to keep tests fast.
    #[must_use]
    pub fn header_read_timeout(mut self, timeout: Duration) -> Self {
        self.header_read_timeout = timeout;
        self
    }

    /// Serves until the task is dropped.
    ///
    /// # Errors
    /// Never returns an error; accept errors are logged and retried like
    /// `axum::serve`. The `io::Result` keeps call sites interchangeable.
    pub async fn serve(self) -> io::Result<()> {
        self.run(std::future::pending()).await;
        Ok(())
    }

    /// Serves until `signal` completes, then stops accepting and waits for
    /// in-flight connections to finish after asking them to close gracefully.
    ///
    /// # Errors
    /// Never returns an error; accept errors are logged and retried like
    /// `axum::serve`. The `io::Result` keeps call sites interchangeable.
    pub async fn serve_with_graceful_shutdown<F>(self, signal: F) -> io::Result<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.run(signal).await;
        Ok(())
    }

    async fn run<F>(self, signal: F)
    where
        F: Future<Output = ()>,
    {
        let Self {
            listener,
            router,
            header_read_timeout,
        } = self;
        let mut builder = http1::Builder::new();
        builder
            .timer(TokioTimer::new())
            .header_read_timeout(header_read_timeout);
        // Like axum's graceful serve: `shutdown_tx` tells connections to close,
        // and every connection holds a `close_rx` clone, so `close_tx.closed()`
        // resolves once the last one has finished.
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let (close_tx, close_rx) = watch::channel(());
        let mut signal = pin!(signal);

        loop {
            let (stream, remote_addr) = tokio::select! {
                accepted = accept(&listener) => accepted,
                () = &mut signal => {
                    tracing::trace!("signal received, not accepting new connections");
                    break;
                }
            };
            tracing::trace!("connection {remote_addr} accepted");

            let router = router.clone();
            let service = hyper::service::service_fn(move |mut request: Request<Incoming>| {
                request.extensions_mut().insert(ConnectInfo(remote_addr));
                router.clone().oneshot(request.map(Body::new))
            });
            let connection = builder
                .serve_connection(TokioIo::new(stream), service)
                .with_upgrades();
            let mut shutdown_rx = shutdown_rx.clone();
            let close_rx = close_rx.clone();
            tokio::spawn(async move {
                let mut connection = pin!(connection);
                let result = tokio::select! {
                    result = connection.as_mut() => result,
                    _ = shutdown_rx.changed() => {
                        connection.as_mut().graceful_shutdown();
                        connection.as_mut().await
                    }
                };
                if let Err(error) = result {
                    tracing::trace!("failed to serve connection: {error:#}");
                }
                drop(close_rx);
            });
        }

        drop(listener);
        drop(close_rx);
        let _ = shutdown_tx.send(());
        tracing::trace!(
            "waiting for {} connection(s) to finish",
            close_tx.receiver_count()
        );
        close_tx.closed().await;
    }
}

async fn accept(listener: &TcpListener) -> (TcpStream, SocketAddr) {
    loop {
        match listener.accept().await {
            Ok(accepted) => return accepted,
            Err(error) => handle_accept_error(error).await,
        }
    }
}

async fn handle_accept_error(error: io::Error) {
    if matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
    ) {
        return;
    }
    // The process may have hit its open-file limit; give connections time to close.
    tracing::error!(%error, "accept error");
    tokio::time::sleep(ACCEPT_ERROR_BACKOFF).await;
}
