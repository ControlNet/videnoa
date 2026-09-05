use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU16;
use std::path::Path;
use std::time::Duration;

use clap::Parser;
use videnoa_controller::auth::AuthService;
use videnoa_controller::config::{
    listener_channel, serve_reconfigurable, ConfigBootstrap, ControllerConfig, PreparedListener,
    ServerOverride,
};
use videnoa_controller::operations::{EventHub, OperationsDependencies, OperationsState};
use videnoa_controller::orchestration::Orchestrator;
use videnoa_controller::paths::PathCapabilities;
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};
use videnoa_controller::recovery::{Reconciler, RecoveryConfig, ShutdownCoordinator};
use videnoa_controller::remote::{PayloadLimits, RemoteTimeouts};
use videnoa_controller::scheduler::{
    Scheduler, TransferConfig, TransferExecutor, TransferResources,
};
use videnoa_controller::tasks::TaskService;
use videnoa_controller::workers::WorkerHealthService;
use videnoa_controller::{controller_app_router, FrontendAssets, StartupError};

mod termination;

use termination::{shutdown_signal, RuntimeError, RuntimeExit};

const RECOVERY_JSON_LIMIT: usize = 1024 * 1024;
const RECOVERY_TRANSFER_CHUNK: usize = 64 * 1024;
const SHUTDOWN_DRAIN_BOUND: Duration = Duration::from_secs(30);

#[derive(Debug, Parser)]
#[command(
    name = "videnoa-controller",
    version,
    about = "GPU-free Videnoa coordination service"
)]
struct Cli {
    #[arg(long)]
    host: Option<IpAddr>,
    #[arg(long)]
    port: Option<NonZeroU16>,
}

fn frontend_assets() -> Result<FrontendAssets, StartupError> {
    #[cfg(debug_assertions)]
    {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../controller-web/dist");
        FrontendAssets::from_dist(directory)
    }

    #[cfg(not(debug_assertions))]
    {
        FrontendAssets::embedded()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    Box::pin(run_controller(cli)).await
}

async fn run_controller(cli: Cli) -> anyhow::Result<()> {
    let workspace = std::env::current_dir()?.canonicalize()?;
    let data_root = ConfigBootstrap::prepare_data_root(&workspace)?;
    let database =
        Database::open(DatabaseOptions::new(data_root.join("controller.sqlite3"))).await?;
    let store = Store::new(database);
    let config = load_configuration(&workspace, &store, &cli).await?;
    let address = SocketAddr::new(config.server.host, config.server.port);
    let paths = PathCapabilities::open(&config.paths)?;
    let recovery = recovery_runtime(&config, &store, &paths).await?;
    let scheduler = recovery.scheduler;
    let shutdown = recovery.shutdown;
    let payload_limits = recovery.payload_limits;
    let auth = AuthService::new(config.auth.clone(), store.clone())?;
    let events = EventHub::new();
    let orchestration = Orchestrator::new(
        store.clone(),
        scheduler.clone(),
        recovery.reconciler,
        recovery.transfers,
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
    let runtime = async move {
        let orchestration = async { orchestration.run().await.map_err(RuntimeError::from) };
        let worker_health = async { worker_health.run().await.map_err(RuntimeError::from) };
        tokio::try_join!(orchestration, worker_health)?;
        Ok::<(), RuntimeError>(())
    };
    let tasks = TaskService::with_events(store.clone(), paths.clone(), events.clone());
    warn_runtime_config(&config);
    let assets = frontend_assets()?;
    let http_shutdown = shutdown.cancellation_token();
    let (listener, rebinds) = listener_channel();
    let operations = OperationsState::new(OperationsDependencies {
        auth: auth.clone(),
        store: store.clone(),
        scheduler: scheduler.clone(),
        paths: paths.clone(),
        config: config.clone(),
        events: events.clone(),
        payload_limits,
    })
    .with_configuration_listener(listener, workspace);
    let router = controller_app_router(
        &assets,
        auth,
        tasks,
        operations.with_shutdown(http_shutdown.child_token()),
    );
    let prepared = PreparedListener::bind(address)
        .await
        .map_err(|source| StartupError::Bind { address, source })?;
    let server = async {
        serve_reconfigurable(prepared, router, rebinds, http_shutdown.child_token())
            .await
            .map_err(StartupError::Serve)
    };
    tokio::pin!(server);
    tokio::pin!(runtime);
    let exit = tokio::select! {
        result = &mut server => RuntimeExit::Server(result),
        result = &mut runtime => RuntimeExit::Runtime(result),
        signal = shutdown_signal() => RuntimeExit::Signal(signal),
    };
    http_shutdown.cancel();
    let shutdown_result = shutdown
        .shutdown(&scheduler, chrono::Utc::now(), SHUTDOWN_DRAIN_BOUND)
        .await;
    match exit {
        RuntimeExit::Server(primary) => {
            let runtime_result = runtime.await;
            primary?;
            shutdown_result?;
            runtime_result?;
        }
        RuntimeExit::Runtime(primary) => {
            let server_result = server.await;
            primary?;
            shutdown_result?;
            server_result?;
        }
        RuntimeExit::Signal(primary) => {
            let (server_result, runtime_result) = tokio::join!(server, runtime);
            primary?;
            shutdown_result?;
            server_result?;
            runtime_result?;
        }
    }
    Ok(())
}

async fn load_configuration(
    workspace: &Path,
    store: &Store,
    cli: &Cli,
) -> anyhow::Result<ControllerConfig> {
    let settings = store.settings().await?;
    if let Some(document) = settings.pending_config_document {
        ConfigBootstrap::repair_projection(workspace, &document)?;
        store.complete_config_projection(settings.version).await?;
    }
    let bootstrap = ConfigBootstrap::open(workspace)?;
    bootstrap
        .reconcile_with_server_override(
            store,
            &ServerOverride {
                host: cli.host,
                port: cli.port,
            },
        )
        .await
        .map_err(Into::into)
}

fn warn_runtime_config(config: &ControllerConfig) {
    if !config.auth.secure_cookie {
        eprintln!("warning: session cookies are running without Secure; use only on trusted HTTP networks");
    }
    if !config.server.host.is_loopback() {
        eprintln!(
            "warning: Controller is exposed beyond localhost; the first administrator setup request claims instance ownership"
        );
    }
}

struct RecoveryRuntime {
    scheduler: Scheduler,
    transfers: TransferExecutor,
    reconciler: Reconciler,
    shutdown: ShutdownCoordinator,
    payload_limits: PayloadLimits,
}

async fn recovery_runtime(
    config: &ControllerConfig,
    store: &Store,
    paths: &PathCapabilities,
) -> anyhow::Result<RecoveryRuntime> {
    let shutdown = ShutdownCoordinator::new();
    let remote_timeouts = RemoteTimeouts::new(
        config.timeouts.health,
        config.timeouts.poll,
        config.timeouts.transfer,
    )?;
    let payload_limits = PayloadLimits::new(RECOVERY_JSON_LIMIT, RECOVERY_TRANSFER_CHUNK)?;
    let scheduler = Scheduler::load(store.clone()).await?;
    let transfers = TransferExecutor::new(
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
    let reconciler = Reconciler::new(
        store.clone(),
        RecoveryConfig::new(
            paths.clone(),
            remote_timeouts,
            payload_limits,
            config.retry.initial,
            config.retry.maximum,
            config.retry.max_attempts.get(),
        )
        .with_runtime_settings(scheduler.runtime_settings().clone()),
        shutdown.clone(),
    );
    Ok(RecoveryRuntime {
        scheduler,
        transfers,
        reconciler,
        shutdown,
        payload_limits,
    })
}
