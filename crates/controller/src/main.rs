use std::error::Error;
use std::io::{BufRead, IsTerminal};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

#[cfg(debug_assertions)]
use std::path::Path;

use clap::{Parser, Subcommand};
use videnoa_controller::auth::{hash_password, AuthService};
use videnoa_controller::config::ControllerConfig;
use videnoa_controller::lifecycle::JitterSample;
use videnoa_controller::operations::{EventHub, OperationsDependencies, OperationsState};
use videnoa_controller::paths::PathCapabilities;
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};
use videnoa_controller::recovery::{Reconciler, RecoveryConfig, ShutdownCoordinator};
use videnoa_controller::remote::{PayloadLimits, RemoteTimeouts};
use videnoa_controller::scheduler::{
    Scheduler, TransferConfig, TransferExecutor, TransferResources,
};
use videnoa_controller::tasks::TaskService;
use videnoa_controller::{serve_controller, FrontendAssets, StartupError};

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
    config: Option<PathBuf>,
    #[arg(long)]
    host: Option<IpAddr>,
    #[arg(long)]
    port: Option<u16>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    HashPassword,
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
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    if matches!(cli.command, Some(Command::HashPassword)) {
        let password = if std::io::stdin().is_terminal() {
            eprint!("Password: ");
            rpassword::read_password()?
        } else {
            let mut password = String::new();
            std::io::stdin().lock().read_line(&mut password)?;
            password.truncate(password.trim_end_matches(['\r', '\n']).len());
            password
        };
        println!("{}", hash_password(&password)?);
        return Ok(());
    }
    let mut config = ControllerConfig::load(cli.config.as_deref())?;
    if let Some(host) = cli.host {
        config.server.host = host;
    }
    if let Some(port) = cli.port {
        config.server.port = port;
    }
    let address = SocketAddr::new(config.server.host, config.server.port);
    let paths = PathCapabilities::open(&config.paths)?;
    let database = Database::open(DatabaseOptions::new(
        config.paths.data_root.join("controller.sqlite3"),
    ))
    .await?;
    let store = Store::new(database);
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
            temp_root: config.paths.temp_root.clone(),
            payload_limits,
            runtime_settings: scheduler.runtime_settings().clone(),
        },
    );
    let startup_at = chrono::Utc::now();
    let reconciler = Reconciler::new(
        store.clone(),
        RecoveryConfig::new(
            remote_timeouts,
            payload_limits,
            config.retry.initial,
            config.retry.maximum,
            config.retry.max_attempts.get(),
        ),
        shutdown.clone(),
    );
    let recovery = reconciler.reconcile_startup(startup_at).await?;
    let advanced = transfers
        .dispatch_recovery(&recovery, startup_at, JitterSample::default())
        .await?;
    for task_id in advanced {
        reconciler
            .reconcile_task_id(task_id, chrono::Utc::now())
            .await?;
    }
    let auth = AuthService::new(config.auth.clone(), store.clone())?;
    let events = EventHub::new();
    let operations = OperationsState::new(OperationsDependencies {
        auth: auth.clone(),
        store: store.clone(),
        scheduler: scheduler.clone(),
        paths: paths.clone(),
        config: config.clone(),
        events: events.clone(),
        payload_limits,
    });
    let tasks = TaskService::with_events(store.clone(), paths, events);
    if !config.auth.secure_cookie {
        eprintln!("warning: session cookies are running without Secure; use only on trusted HTTP networks");
    }
    let assets = frontend_assets()?;
    let server = serve_controller(address, &assets, auth, tasks, operations);
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => result?,
        signal = shutdown_signal() => {
            signal?;
            shutdown
                .shutdown(&scheduler, chrono::Utc::now(), SHUTDOWN_DRAIN_BOUND)
                .await?;
        }
    }
    Ok(())
}

#[cfg(unix)]
async fn shutdown_signal() -> Result<(), std::io::Error> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result,
        _ = terminate.recv() => Ok(()),
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() -> Result<(), std::io::Error> {
    tokio::signal::ctrl_c().await
}
