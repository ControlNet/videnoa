use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::{JoinHandle, JoinSet};
use tower::ServiceExt;

use super::routes::{self, DROP_RESPONSE_HEADER};
use super::state::{HarnessError, SharedState};

pub(super) struct ServerRuntime {
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<Result<(), HarnessError>>,
}

impl ServerRuntime {
    pub(super) async fn stop(self) -> Result<(), HarnessError> {
        let _ = self.shutdown.send(());
        let joined = tokio::time::timeout(Duration::from_secs(5), self.task)
            .await
            .map_err(|_| HarnessError::ShutdownTimeout)?;
        joined??;
        Ok(())
    }

    pub(super) fn abort(self) {
        self.task.abort();
    }
}

pub(super) fn spawn_runtime(listener: TcpListener, state: Arc<SharedState>) -> ServerRuntime {
    let (shutdown, receiver) = oneshot::channel();
    let task = tokio::spawn(serve(listener, state, receiver));
    ServerRuntime { shutdown, task }
}

async fn serve(
    listener: TcpListener,
    state: Arc<SharedState>,
    mut shutdown: oneshot::Receiver<()>,
) -> Result<(), HarnessError> {
    let router = routes::router(Arc::clone(&state));
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                connections.spawn(serve_connection(stream, router.clone(), Arc::clone(&state)));
            }
        }
    }
    connections.abort_all();
    while connections.join_next().await.is_some() {}
    Ok(())
}

async fn serve_connection(stream: TcpStream, router: axum::Router, state: Arc<SharedState>) {
    let service = service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
        let router = router.clone();
        let state = Arc::clone(&state);
        async move {
            if state.take_disconnect().await {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    "MockDisconnectBeforeAccept",
                ));
            }
            if state.service_unavailable().await {
                return Ok(StatusCode::SERVICE_UNAVAILABLE.into_response());
            }
            let request = request.map(Body::new);
            let mut response = match router.oneshot(request).await {
                Ok(response) => response,
                Err(error) => match error {},
            };
            if response
                .headers_mut()
                .remove(DROP_RESPONSE_HEADER)
                .is_some()
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    "MockAcceptThenDropRunResponse",
                ));
            }
            Ok(response)
        }
    });
    let _ = http1::Builder::new()
        .serve_connection(TokioIo::new(stream), service)
        .await;
}
