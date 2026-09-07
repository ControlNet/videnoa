//! Worker iroh lifecycle. Configuration and password mutations share settings_lock.
use std::sync::Arc;

use axum::{extract::State, Json};
use serde::Serialize;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use videnoa_transport::{Authorizer, Identity, PeerMap, Server};

use super::{api_router, AppError, AppState};

#[derive(Default)]
pub(super) struct Runtime {
    inner: Mutex<RuntimeInner>,
    pub peers: PeerMap,
}
#[derive(Default)]
struct RuntimeInner {
    identity: Option<Identity>,
    server: Option<Server>,
    api_stop: Option<CancellationToken>,
    api_task: Option<tokio::task::JoinHandle<()>>,
    error: Option<String>,
}
impl RuntimeInner {
    async fn shutdown(&mut self) {
        if let Some(stop) = self.api_stop.take() {
            stop.cancel();
        }
        if let Some(server) = self.server.take() {
            server.shutdown().await;
        }
        if let Some(task) = self.api_task.take() {
            let _ = task.await;
        }
        self.identity = None;
        self.error = None;
    }
}
impl Drop for RuntimeInner {
    fn drop(&mut self) {
        if let Some(stop) = &self.api_stop {
            stop.cancel();
        }
    }
}

#[derive(Serialize)]
pub struct IrohStatus {
    enabled: bool,
    running: bool,
    endpoint_id: Option<String>,
    error: Option<String>,
}

impl AppState {
    #[cfg(test)]
    pub(super) async fn iroh_addr(&self) -> Option<videnoa_transport::EndpointAddr> {
        self.inner
            .iroh
            .inner
            .lock()
            .await
            .server
            .as_ref()
            .map(Server::addr)
    }

    pub(super) fn iroh_password_enabled(&self) -> Result<bool, AppError> {
        self.inner
            .auth
            .as_ref()
            .map_err(|_| AppError::Internal("Authentication unavailable".into()))?
            .password_enabled()
            .map_err(|_| AppError::Internal("Authentication unavailable".into()))
    }

    /// Reconciles the internal API listener and iroh endpoint with stored settings.
    /// Call while serializing settings/password changes, or before accepting requests.
    pub async fn reconcile_iroh(&self) -> anyhow::Result<()> {
        let enabled = self.inner.config.read().await.iroh.enabled;
        let mut runtime = self.inner.iroh.inner.lock().await;
        if !enabled {
            runtime.shutdown().await;
            return Ok(());
        }
        let auth = self
            .inner
            .auth
            .as_ref()
            .map_err(|_| anyhow::anyhow!("Authentication unavailable"))?
            .clone();
        if !auth.password_enabled()? {
            runtime.error = Some("Set a worker password before enabling iroh".into());
            anyhow::bail!("Set a worker password before enabling iroh");
        }
        if runtime.server.is_some() {
            return Ok(());
        }
        let result = async {
            if runtime.identity.is_none() {
                runtime.identity = Some(Identity::open(&self.inner.data_dir)?);
            }
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let target = listener.local_addr()?;
            let authorize: Authorizer = Arc::new(move |peer, password| {
                let auth = auth.clone();
                Box::pin(async move { auth.verify_tunnel(peer, password).await })
            });
            let identity = runtime.identity.as_ref().expect("identity was initialized");
            let server = Server::start(identity, target, authorize, self.inner.iroh.peers.clone()).await?;
            let stop = CancellationToken::new();
            let stopped = stop.clone();
            let router = api_router(self.clone());
            let task = tokio::spawn(async move {
                tokio::select! {
                    _ = stopped.cancelled() => {},
                    _ = axum::serve(listener, router.into_make_service_with_connect_info::<std::net::SocketAddr>()) => {},
                }
            });
            tracing::info!(endpoint_id = %server.id(), "iroh enabled");
            runtime.server = Some(server);
            runtime.api_stop = Some(stop);
            runtime.api_task = Some(task);
            anyhow::Ok(())
        }.await;
        runtime.error = result.as_ref().err().map(ToString::to_string);
        result
    }

    pub async fn shutdown_iroh(&self) {
        let mut runtime = self.inner.iroh.inner.lock().await;
        runtime.shutdown().await;
    }

    pub(super) async fn disable_iroh_persisted(&self) -> Result<(), AppError> {
        // Persist the disabled flag before deleting the password, so crashes fail closed.
        let mut config = self.inner.config.write().await;
        if config.iroh.enabled {
            let mut disabled = config.clone();
            disabled.iroh.enabled = false;
            disabled.save_to_path(&self.inner.config_path)?;
            *config = disabled;
        }
        drop(config);
        self.shutdown_iroh().await;
        Ok(())
    }
}

pub(super) async fn status(State(state): State<AppState>) -> Result<Json<IrohStatus>, AppError> {
    let enabled = state.inner.config.read().await.iroh.enabled;
    if !enabled {
        return Ok(Json(IrohStatus {
            enabled: false,
            running: false,
            endpoint_id: None,
            error: None,
        }));
    }
    // Status reads never initialize transport or access identity files.
    let runtime = state.inner.iroh.inner.lock().await;
    Ok(Json(IrohStatus {
        enabled,
        running: runtime.server.is_some(),
        endpoint_id: runtime.identity.as_ref().map(|id| id.id().to_string()),
        error: runtime.error.clone(),
    }))
}
