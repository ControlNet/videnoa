//! Shared controller endpoint and per-client loopback tunnel leases.
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;
use videnoa_transport::{Client, EndpointId, TunnelError};

use super::{ClientConfigError, VidenoaClientError};

static ROOT: OnceLock<PathBuf> = OnceLock::new();
static CLIENT: OnceCell<Arc<Client>> = OnceCell::const_new();

/// Selects the existing persistent Controller root before background services start.
/// # Errors
/// Fails if another root was already configured in this process.
pub fn configure_iroh(root: &Path) -> anyhow::Result<()> {
    if let Some(existing) = ROOT.get() {
        anyhow::ensure!(existing == root, "iroh runtime root is already configured");
    } else {
        ROOT.set(root.to_path_buf())
            .map_err(|_| anyhow::anyhow!("iroh root initialization raced"))?;
    }
    Ok(())
}

async fn client() -> Result<Arc<Client>, VidenoaClientError> {
    CLIENT
        .get_or_try_init(|| async {
            let root = ROOT.get().ok_or(VidenoaClientError::Network)?;
            Client::open(root)
                .await
                .map(Arc::new)
                .map_err(|_| VidenoaClientError::Network)
        })
        .await
        .cloned()
}

pub(super) struct Lease {
    peer: EndpointId,
    password: crate::domain::SecretString,
    listener: tokio::sync::Mutex<Option<tokio::net::TcpListener>>,
    ready: OnceCell<()>,
    stop: CancellationToken,
    failure: Arc<std::sync::Mutex<Option<TunnelError>>>,
}
impl Lease {
    pub(super) fn bind(
        peer: EndpointId,
        password: &crate::domain::SecretString,
    ) -> Result<(Arc<Self>, crate::domain::WorkerApiUrl), ClientConfigError> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|_| ClientConfigError::HttpClient)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| ClientConfigError::HttpClient)?;
        let address = listener
            .local_addr()
            .map_err(|_| ClientConfigError::HttpClient)?;
        let listener = tokio::net::TcpListener::from_std(listener)
            .map_err(|_| ClientConfigError::HttpClient)?;
        let url = crate::domain::WorkerApiUrl::parse(&format!("http://{address}/"))
            .map_err(|_| ClientConfigError::HttpClient)?;
        Ok((
            Arc::new(Self {
                peer,
                password: password.clone(),
                listener: tokio::sync::Mutex::new(Some(listener)),
                ready: OnceCell::new(),
                stop: CancellationToken::new(),
                failure: Arc::default(),
            }),
            url,
        ))
    }

    pub(super) fn failure(&self) -> Option<VidenoaClientError> {
        self.failure.lock().ok()?.as_ref().map(|error| match error {
            TunnelError::Unauthorized => VidenoaClientError::ClientStatus { status: 401 },
            TunnelError::RateLimited => VidenoaClientError::RateLimited,
            TunnelError::Protocol => VidenoaClientError::MalformedPayload,
            TunnelError::Unavailable => VidenoaClientError::Network,
        })
    }

    pub(super) async fn ready(&self) -> Result<(), VidenoaClientError> {
        self.ready
            .get_or_try_init(|| async {
                let client = client().await?;
                // Surface authentication errors before reqwest sees only a closed local socket.
                let probe = client
                    .tunnel(self.peer, self.password.expose())
                    .await
                    .map_err(|error| classify(&error))?;
                drop(probe);
                let listener = self
                    .listener
                    .lock()
                    .await
                    .take()
                    .ok_or(VidenoaClientError::Network)?;
                tokio::spawn(client.forward(
                    listener,
                    self.peer,
                    self.password.expose().to_owned(),
                    self.stop.clone(),
                    self.failure.clone(),
                ));
                Ok(())
            })
            .await
            .copied()
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
fn classify(error: &anyhow::Error) -> VidenoaClientError {
    match error.downcast_ref::<TunnelError>() {
        Some(TunnelError::Unauthorized) => VidenoaClientError::ClientStatus { status: 401 },
        Some(TunnelError::RateLimited) => VidenoaClientError::RateLimited,
        Some(TunnelError::Protocol) => VidenoaClientError::MalformedPayload,
        _ => VidenoaClientError::Network,
    }
}

pub async fn shutdown_iroh() {
    if let Some(client) = CLIENT.get() {
        client.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{SecretString, WorkerApiUrl};
    use crate::remote::{FileApiPath, PayloadLimits, RemoteTimeouts, VidenoaClient};
    use axum::{
        body::Bytes,
        extract::State,
        http::{HeaderMap, Method, StatusCode, Uri},
        response::IntoResponse,
        routing::any,
        Json, Router,
    };
    use std::time::Duration;

    #[tokio::test]
    async fn controller_http_client_streams_through_authenticated_iroh() -> anyhow::Result<()> {
        // Synthetic credential, in-memory file and private loopback HTTP origin for this test only.
        const PASSWORD: &str = "controller iroh test credential";
        type File = Arc<tokio::sync::Mutex<Vec<u8>>>;
        async fn origin(
            State(file): State<File>,
            method: Method,
            uri: Uri,
            headers: HeaderMap,
            body: Bytes,
        ) -> axum::response::Response {
            if uri.path() == "/api/health" {
                assert!(!headers.contains_key("authorization"));
                return Json(serde_json::json!({"status":"ok"})).into_response();
            }
            if headers.get("authorization").and_then(|v| v.to_str().ok())
                != Some("Bearer controller iroh test credential")
            {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            match (method, uri.path()) {
                (Method::GET, "/api/workflows" | "/api/presets") => {
                    Json(serde_json::json!([])).into_response()
                }
                (Method::PUT, "/api/files/test.bin") => {
                    *file.lock().await = body.to_vec();
                    Json(serde_json::json!({"path":"test.bin","size":body.len()})).into_response()
                }
                (Method::GET, "/api/files/test.bin") => file.lock().await.clone().into_response(),
                (Method::DELETE, "/api/files/test.bin") => {
                    file.lock().await.clear();
                    StatusCode::NO_CONTENT.into_response()
                }
                _ => StatusCode::NOT_FOUND.into_response(),
            }
        }
        let file: File = Arc::default();
        let http = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let target = http.local_addr()?;
        let router = Router::new().fallback(any(origin)).with_state(file.clone());
        let http_task = tokio::spawn(async move { axum::serve(http, router).await });
        let worker_root = tempfile::tempdir()?;
        let identity = videnoa_transport::Identity::open(worker_root.path())?;
        let auth: videnoa_transport::Authorizer = Arc::new(|_, supplied| {
            Box::pin(async move {
                if supplied == PASSWORD {
                    Ok(())
                } else {
                    Err(TunnelError::Unauthorized)
                }
            })
        });
        let server = videnoa_transport::Server::start(
            &identity,
            target,
            auth,
            videnoa_transport::PeerMap::default(),
        )
        .await?;
        let controller_root = tempfile::tempdir()?;
        configure_iroh(controller_root.path())?;
        // Prime the shared real connection with explicit local addresses: deterministic CI,
        // independent of N0 DNS. The separate opt-in test covers ID-only discovery.
        drop(client().await?.tunnel(server.addr(), PASSWORD).await?);
        let password = SecretString::new(PASSWORD);
        let url = WorkerApiUrl::parse(&format!("iroh://{}/", server.id()))?;
        let limits = PayloadLimits::new(4096, 4096)?;
        let timeouts = RemoteTimeouts::new(
            Duration::from_secs(5),
            Duration::from_secs(5),
            Duration::from_secs(5),
        )?;
        let client =
            VidenoaClient::new_with_password(url.clone(), timeouts, limits, Some(&password))?;
        assert!(client.health().await?.is_healthy());
        assert!(client.workflows().await?.is_empty());
        let path = FileApiPath::parse("test.bin")?;
        let payload = vec![42u8; 256 * 1024];
        let receipt = client
            .upload(
                &path,
                payload.len() as u64,
                std::io::Cursor::new(payload.clone()),
            )
            .await?;
        assert_eq!(receipt.size, payload.len() as u64);
        let mut downloaded = Vec::new();
        client.download(&path, &mut downloaded).await?;
        assert_eq!(downloaded, payload);
        client.delete_file(&path).await?;
        assert!(file.lock().await.is_empty());
        let bad = SecretString::new("incorrect controller test credential");
        let rejected = VidenoaClient::new_with_password(url, timeouts, limits, Some(&bad))?;
        assert_eq!(
            rejected.health().await,
            Err(VidenoaClientError::ClientStatus { status: 401 })
        );
        drop(rejected);
        drop(client);
        server.shutdown().await;
        shutdown_iroh().await;
        http_task.abort();
        Ok(())
    }
}
