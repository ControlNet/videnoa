use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::routing::get;
use axum::Router;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

use super::{apply, OperationsError};
use crate::auth::AuthService;
use crate::config::{listener_channel, serve_reconfigurable, ConfigBootstrap, PreparedListener};
use crate::domain::{ComputeSlots, ConcurrencyLimit, SettingsUpdateRequest};
use crate::operations::{EventHub, OperationsDependencies, OperationsState};
use crate::paths::PathCapabilities;
use crate::persistence::{Database, DatabaseOptions, Store};
use crate::recovery::RecoveryConfig;
use crate::remote::PayloadLimits;
use crate::scheduler::Scheduler;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

async fn assert_hot_runtime(
    workspace: &TempDir,
    store: &Store,
    auth: &AuthService,
    scheduler: &Scheduler,
    recovery_config: &RecoveryConfig,
    new_port: u16,
) -> TestResult {
    assert!(store.settings().await?.pending_config_document.is_none());
    let projected = std::fs::read_to_string(workspace.path().join("data/controller.toml"))?;
    assert!(projected.contains(&format!("port = {new_port}")));
    assert!(auth.secure_cookie());
    assert_eq!(auth.session_absolute_seconds(), 7_200);
    assert_eq!(
        scheduler.runtime_settings().timeout_settings().poll_seconds,
        12
    );
    assert_eq!(
        recovery_config.remote_timeouts(),
        scheduler.runtime_settings().remote_timeouts()
    );
    assert_eq!(
        recovery_config.health_retry(),
        (
            std::time::Duration::from_secs(2),
            std::time::Duration::from_secs(20),
            7,
        )
    );
    assert_eq!(
        reqwest::get(format!("http://127.0.0.1:{new_port}/probe"))
            .await?
            .text()
            .await?,
        "ready"
    );
    Ok(())
}

#[tokio::test]
async fn settings_update_persists_projects_and_hot_applies_every_public_field() -> TestResult {
    // Given: a fully bootstrapped runtime and an authenticated-settings state coordinator.
    let workspace = TempDir::new()?;
    let bootstrap = ConfigBootstrap::open(workspace.path())?;
    let database = Database::open(DatabaseOptions::new(
        workspace.path().join("data/controller.sqlite3"),
    ))
    .await?;
    let store = Store::new(database);
    let config = bootstrap.reconcile(&store).await?;
    let paths = PathCapabilities::open(&config.paths)?;
    let scheduler = Scheduler::load(store.clone()).await?;
    let auth = AuthService::new(config.auth.clone(), store.clone())?;
    let recovery_config = RecoveryConfig::new(
        paths.clone(),
        scheduler.runtime_settings().remote_timeouts(),
        PayloadLimits::new(1024 * 1024, 64 * 1024)?,
        config.retry.initial,
        config.retry.maximum,
        config.retry.max_attempts.get(),
    )
    .with_runtime_settings(scheduler.runtime_settings().clone());
    let (listener, receiver) = listener_channel();
    let shutdown = CancellationToken::new();
    let initial =
        PreparedListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).await?;
    let server = tokio::spawn(serve_reconfigurable(
        initial,
        Router::new().route("/probe", get(|| async { "ready" })),
        receiver,
        shutdown.clone(),
    ));
    let state = OperationsState::new(OperationsDependencies {
        auth: auth.clone(),
        store: store.clone(),
        scheduler: scheduler.clone(),
        paths,
        config,
        events: EventHub::new(),
        payload_limits: PayloadLimits::new(1024 * 1024, 64 * 1024)?,
    })
    .with_configuration_listener(listener, workspace.path().to_path_buf());
    let current = store.settings().await?;
    let reservation = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let new_port = reservation.local_addr()?.port();
    drop(reservation);
    let mut request = SettingsUpdateRequest {
        version: current.version,
        server: current.server,
        auth: current.auth,
        scheduler: current.scheduler,
        timeouts: current.timeouts,
        retry: current.retry,
    };
    request.server.port = new_port;
    request.auth.secure_cookie = true;
    request.auth.session_absolute_seconds = 7_200;
    request.auth.session_idle_seconds = 600;
    request.scheduler.paused = true;
    request.scheduler.default_compute_slots = ComputeSlots::try_from(2)?;
    request.scheduler.prefetch_per_worker = 3;
    request.scheduler.max_concurrent_uploads = ConcurrencyLimit::try_from(4)?;
    request.scheduler.max_concurrent_downloads = ConcurrencyLimit::try_from(5)?;
    request.timeouts.health_seconds = 11;
    request.timeouts.poll_seconds = 12;
    request.timeouts.transfer_seconds = 13;
    request.retry.initial_seconds = 2;
    request.retry.maximum_seconds = 20;
    request.retry.max_attempts = 7;

    // When: Web Settings applies the complete versioned request.
    let response = apply(&state, request)
        .await
        .map_err(|error| std::io::Error::other(format!("settings update failed: {error:?}")))?
        .0;

    // Then: SQLite, TOML, auth, scheduler, and the live listener converge before success.
    assert_eq!(response.server.port, new_port);
    assert!(response.secure_cookie);
    assert_eq!(response.scheduler.prefetch_per_worker, 3);
    assert_eq!(response.timeouts.poll_seconds, 12);
    assert_eq!(response.retry.max_attempts, 7);
    assert_hot_runtime(
        &workspace,
        &store,
        &auth,
        &scheduler,
        &recovery_config,
        new_port,
    )
    .await?;
    shutdown.cancel();
    server.await??;
    Ok(())
}

#[tokio::test]
async fn committed_projection_failure_keeps_runtime_live_and_repairs_automatically() -> TestResult {
    // Given: a runtime whose projection staging path is blocked after initialization.
    let workspace = TempDir::new()?;
    let bootstrap = ConfigBootstrap::open(workspace.path())?;
    let database = Database::open(DatabaseOptions::new(
        workspace.path().join("data/controller.sqlite3"),
    ))
    .await?;
    let store = Store::new(database);
    let config = bootstrap.reconcile(&store).await?;
    let paths = PathCapabilities::open(&config.paths)?;
    let scheduler = Scheduler::load(store.clone()).await?;
    let auth = AuthService::new(config.auth.clone(), store.clone())?;
    let state = OperationsState::new(OperationsDependencies {
        auth: auth.clone(),
        store: store.clone(),
        scheduler: scheduler.clone(),
        paths,
        config,
        events: EventHub::new(),
        payload_limits: PayloadLimits::new(1024 * 1024, 64 * 1024)?,
    });
    let current = store.settings().await?;
    let mut request = SettingsUpdateRequest {
        version: current.version,
        server: current.server,
        auth: current.auth,
        scheduler: current.scheduler,
        timeouts: current.timeouts,
        retry: current.retry,
    };
    request.auth.secure_cookie = true;
    request.timeouts.poll_seconds = 12;
    std::fs::create_dir(workspace.path().join("data/.controller.toml.pending"))?;

    // When: SQLite commits but the first TOML projection attempt fails.
    let result = apply(&state, request).await;

    // Then: the committed runtime is live, failure is explicit, and repair completes automatically.
    assert!(matches!(result, Err(OperationsError::CommittedDegraded)));
    assert!(auth.secure_cookie());
    assert_eq!(
        scheduler.runtime_settings().timeout_settings().poll_seconds,
        12
    );
    assert!(store.settings().await?.pending_config_document.is_some());
    std::fs::remove_dir(workspace.path().join("data/.controller.toml.pending"))?;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if store.settings().await?.pending_config_document.is_none() {
                return TestResult::Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await??;
    Ok(())
}

#[tokio::test]
async fn server_change_without_listener_capability_is_rejected_before_commit() -> TestResult {
    // Given: an operations fixture without the production listener capability.
    let workspace = TempDir::new()?;
    let bootstrap = ConfigBootstrap::open(workspace.path())?;
    let database = Database::open(DatabaseOptions::new(
        workspace.path().join("data/controller.sqlite3"),
    ))
    .await?;
    let store = Store::new(database);
    let config = bootstrap.reconcile(&store).await?;
    let paths = PathCapabilities::open(&config.paths)?;
    let scheduler = Scheduler::load(store.clone()).await?;
    let auth = AuthService::new(config.auth.clone(), store.clone())?;
    let state = OperationsState::new(OperationsDependencies {
        auth,
        store: store.clone(),
        scheduler,
        paths,
        config,
        events: EventHub::new(),
        payload_limits: PayloadLimits::new(1024 * 1024, 64 * 1024)?,
    });
    let current = store.settings().await?;
    let current_server = current.server.clone();
    let reservation = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let mut request = SettingsUpdateRequest {
        version: current.version,
        server: current.server,
        auth: current.auth,
        scheduler: current.scheduler,
        timeouts: current.timeouts,
        retry: current.retry,
    };
    request.server.port = reservation.local_addr()?.port();
    drop(reservation);

    // When: the fixture requests a listener address change.
    let result = apply(&state, request).await;

    // Then: capability validation fails before durable settings change.
    assert!(matches!(
        result,
        Err(OperationsError::InvalidField("server", _))
    ));
    let unchanged = store.settings().await?;
    assert_eq!(unchanged.version, current.version);
    assert_eq!(unchanged.server, current_server);
    Ok(())
}
