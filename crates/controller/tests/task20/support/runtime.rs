use std::fs;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::oneshot;
use videnoa_controller::auth::AuthService;
use videnoa_controller::config::{AuthConfig, ControllerConfig, PathConfig};
use videnoa_controller::operations::{EventHub, OperationsDependencies, OperationsState};
use videnoa_controller::orchestration::Orchestrator;
use videnoa_controller::paths::PathCapabilities;
use videnoa_controller::persistence::{SettingsUpdate, Store};
use videnoa_controller::recovery::{Reconciler, RecoveryConfig, ShutdownCoordinator};
use videnoa_controller::remote::{PayloadLimits, RemoteTimeouts};
use videnoa_controller::scheduler::{
    Scheduler, TransferCheckpointObserver, TransferConfig, TransferExecutor, TransferResources,
};
use videnoa_controller::tasks::TaskService;
use videnoa_controller::workers::WorkerHealthService;
use videnoa_controller::{controller_app_router, FrontendAssets};

use super::TestResult;

pub(super) struct ControllerRuntime {
    pub(super) address: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<Result<(), std::io::Error>>,
    orchestration:
        tokio::task::JoinHandle<Result<(), videnoa_controller::orchestration::OrchestrationError>>,
    worker_health:
        tokio::task::JoinHandle<Result<(), videnoa_controller::workers::WorkerHealthError>>,
    coordinator: ShutdownCoordinator,
}

pub(super) async fn start_runtime(
    root: &Path,
    store: &Store,
    path_config: &PathConfig,
    auth_config: &AuthConfig,
    checkpoint_observer: Option<Arc<dyn TransferCheckpointObserver>>,
) -> TestResult<ControllerRuntime> {
    let auth = AuthService::new(auth_config.clone(), store.clone())?;
    let paths = PathCapabilities::open(path_config)?;
    let scheduler = Scheduler::load(store.clone()).await?;
    let shutdown = ShutdownCoordinator::new();
    let config = ControllerConfig {
        server: videnoa_controller::config::ServerConfig {
            host: Ipv4Addr::LOCALHOST.into(),
            port: 0,
        },
        paths: path_config.clone(),
        auth: auth_config.clone(),
        ..ControllerConfig::default()
    };
    let settings = store.settings().await?;
    let mut runtime_timeouts = settings.timeouts;
    runtime_timeouts.health_seconds = 1;
    runtime_timeouts.poll_seconds = 1;
    scheduler
        .update_settings(SettingsUpdate {
            expected_version: settings.version,
            scheduler: settings.scheduler,
            timeouts: runtime_timeouts,
            retry: settings.retry,
            updated_at: chrono::Utc::now(),
        })
        .await?;
    let events = EventHub::new();
    let payload_limits = PayloadLimits::new(1024 * 1024, 4096)?;
    let remote_timeouts = RemoteTimeouts::new(
        config.timeouts.health,
        config.timeouts.poll,
        config.timeouts.transfer,
    )?;
    let mut transfers = TransferExecutor::new(
        TransferResources {
            store: store.clone(),
            paths: paths.clone(),
            coordinator: scheduler.transfers().clone(),
        },
        TransferConfig {
            payload_limits,
            runtime_settings: scheduler.runtime_settings().clone(),
        },
    );
    if let Some(observer) = checkpoint_observer.clone() {
        transfers = transfers.with_checkpoint_observer(observer);
    }
    let mut reconciler = Reconciler::new(
        store.clone(),
        RecoveryConfig::new(
            paths.clone(),
            remote_timeouts,
            payload_limits,
            config.retry.initial,
            config.retry.maximum,
            config.retry.max_attempts.get(),
        ),
        shutdown.clone(),
    );
    if let Some(observer) = checkpoint_observer {
        reconciler = reconciler.with_checkpoint_observer(observer);
    }
    let orchestrator = Orchestrator::new(
        store.clone(),
        scheduler.clone(),
        reconciler,
        transfers,
        shutdown.clone(),
        &events,
    );
    let worker_health = WorkerHealthService::new(
        store.clone(),
        scheduler.runtime_settings().clone(),
        payload_limits,
        shutdown.clone(),
        &events,
    );
    let operations = OperationsState::new(OperationsDependencies {
        auth: auth.clone(),
        store: store.clone(),
        scheduler,
        paths: paths.clone(),
        config,
        events,
        payload_limits,
    });
    let tasks = TaskService::new(store.clone(), paths);
    let router = controller_app_router(&assets(root)?, auth, tasks, operations);
    ControllerRuntime::start(router, orchestrator, worker_health, shutdown).await
}

impl ControllerRuntime {
    async fn start(
        router: axum::Router,
        orchestrator: Orchestrator,
        worker_health: WorkerHealthService,
        coordinator: ShutdownCoordinator,
    ) -> TestResult<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let (shutdown, receiver) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                let _ = receiver.await;
            })
            .await
        });
        Ok(Self {
            address,
            shutdown: Some(shutdown),
            task,
            orchestration: tokio::spawn(orchestrator.run()),
            worker_health: tokio::spawn(worker_health.run()),
            coordinator,
        })
    }

    pub(super) async fn crash(mut self) {
        self.orchestration.abort();
        self.worker_health.abort();
        self.task.abort();
        let _ = (&mut self.orchestration).await;
        let _ = (&mut self.worker_health).await;
        let _ = (&mut self.task).await;
    }

    pub(super) async fn stop(mut self) -> TestResult {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.coordinator.stop_stage_intake();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            (&mut self.task).await??;
            (&mut self.orchestration).await??;
            (&mut self.worker_health).await??;
            TestResult::Ok(())
        })
        .await
        .map_err(|_| std::io::Error::other("Controller runtime did not stop"))??;
        Ok(())
    }

    pub(super) async fn wait_for_orchestration_error(
        &mut self,
    ) -> TestResult<videnoa_controller::orchestration::OrchestrationError> {
        let joined =
            tokio::time::timeout(std::time::Duration::from_secs(5), &mut self.orchestration)
                .await
                .map_err(|_| std::io::Error::other("orchestration did not terminate"))??;
        joined
            .err()
            .ok_or_else(|| std::io::Error::other("orchestration exited successfully").into())
    }
}

impl Drop for ControllerRuntime {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.coordinator.stop_stage_intake();
        self.orchestration.abort();
        self.worker_health.abort();
        self.task.abort();
    }
}

#[cfg(debug_assertions)]
fn assets(root: &Path) -> TestResult<FrontendAssets> {
    let directory = root.join("assets");
    fs::create_dir_all(&directory)?;
    fs::write(directory.join("index.html"), "<main>controller</main>")?;
    Ok(FrontendAssets::from_dist(directory)?)
}

#[cfg(not(debug_assertions))]
fn assets(_: &Path) -> TestResult<FrontendAssets> {
    Ok(FrontendAssets::embedded()?)
}
