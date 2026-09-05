mod events;
#[path = "error.rs"]
mod request_failure;
mod settings;
mod status;
mod tasks;
mod workers;

use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post, put};
use axum::Router;
use chrono::Utc;
use tokio_util::sync::CancellationToken;

use crate::auth::{authenticate, authorize_mutation, peer_ip, AuthService};
use crate::config::{ControllerConfig, ListenerHandle};
use crate::lifecycle::LifecycleService;
use crate::paths::PathCapabilities;
use crate::persistence::{ChangeObserver, Store};
use crate::remote::PayloadLimits;
use crate::scheduler::Scheduler;
use crate::workers::WorkerRegistry;

pub use events::EventHub;
use request_failure::OperationsError;

#[derive(Clone)]
pub struct OperationsDependencies {
    pub auth: AuthService,
    pub store: Store,
    pub scheduler: Scheduler,
    pub paths: PathCapabilities,
    pub config: ControllerConfig,
    pub events: EventHub,
    pub payload_limits: PayloadLimits,
}

#[derive(Clone)]
pub struct OperationsState {
    auth: AuthService,
    store: Store,
    scheduler: Scheduler,
    paths: PathCapabilities,
    config: ControllerConfig,
    workers: WorkerRegistry,
    lifecycle: LifecycleService,
    events: EventHub,
    payload_limits: PayloadLimits,
    listener: Option<ListenerHandle>,
    workspace: std::path::PathBuf,
    settings_lock: std::sync::Arc<tokio::sync::Mutex<()>>,
    shutdown: Option<CancellationToken>,
}

impl OperationsState {
    #[must_use]
    pub fn new(dependencies: OperationsDependencies) -> Self {
        let events = dependencies.events.clone();
        let workspace = dependencies
            .config
            .paths
            .input_roots
            .first()
            .cloned()
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        dependencies
            .store
            .observe_changes(ChangeObserver::new(move |change| {
                events.publish_change(change);
            }));
        Self {
            workers: WorkerRegistry::new(dependencies.store.clone()),
            lifecycle: LifecycleService::new(dependencies.store.clone()),
            events: dependencies.events,
            auth: dependencies.auth,
            store: dependencies.store,
            scheduler: dependencies.scheduler,
            paths: dependencies.paths,
            config: dependencies.config,
            payload_limits: dependencies.payload_limits,
            workspace,
            listener: None,
            settings_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            shutdown: None,
        }
    }

    #[must_use]
    pub fn with_configuration_listener(
        mut self,
        listener: ListenerHandle,
        workspace: std::path::PathBuf,
    ) -> Self {
        self.listener = Some(listener);
        self.workspace = workspace;
        self
    }

    #[must_use]
    pub fn with_shutdown(mut self, shutdown: CancellationToken) -> Self {
        self.shutdown = Some(shutdown);
        self
    }

    pub(crate) fn event_hub(&self) -> EventHub {
        self.events.clone()
    }
}

pub(crate) fn router(state: OperationsState) -> Router {
    let reads = Router::new()
        .route("/api/workers", get(workers::list))
        .route("/api/settings", get(settings::get))
        .route("/api/status-counts", get(status::counts))
        .route("/api/events", get(events::stream))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth));
    let readiness = Router::new().route("/api/readiness", get(status::readiness));
    let writes = Router::new()
        .route("/api/workers", post(workers::create))
        .route(
            "/api/workers/{id}",
            put(workers::update).delete(workers::delete),
        )
        .route("/api/workers/{id}/enable", post(workers::enable))
        .route("/api/workers/{id}/disable", post(workers::disable))
        .route("/api/settings", put(settings::update))
        .route("/api/scheduler/pause", post(settings::pause))
        .route("/api/scheduler/resume", post(settings::resume))
        .route("/api/tasks/{id}/cancel", post(tasks::cancel))
        .route("/api/tasks/{id}/retry", post(tasks::retry))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_mutation,
        ));
    reads.merge(readiness).merge(writes).with_state(state)
}

async fn require_auth(
    State(state): State<OperationsState>,
    request: Request,
    next: Next,
) -> Result<Response, OperationsError> {
    authenticate(&state.auth, peer_ip(&request)?, request.headers(), Utc::now())
        .await
        .map_err(|error| OperationsError::from_auth(&error))?;
    Ok(next.run(request).await)
}

async fn require_mutation(
    State(state): State<OperationsState>,
    request: Request,
    next: Next,
) -> Result<Response, OperationsError> {
    authorize_mutation(&state.auth, peer_ip(&request)?, request.headers(), Utc::now())
        .await
        .map_err(|error| OperationsError::from_auth(&error))?;
    Ok(next.run(request).await)
}
