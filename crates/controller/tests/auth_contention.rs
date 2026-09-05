//! Production-cost stress coverage, explicitly run by the Controller fault/load CI job.
use std::error::Error;
#[cfg(debug_assertions)]
use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::connect_info::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use tempfile::TempDir;
use tokio::sync::Barrier;
use tower::ServiceExt;
use videnoa_controller::auth::{hash_password, AuthService};
use videnoa_controller::config::ControllerConfig;
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};
use videnoa_controller::{authenticated_app_router, FrontendAssets};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[cfg(debug_assertions)]
fn assets(directory: &std::path::Path) -> TestResult<FrontendAssets> {
    let assets = directory.join("assets");
    fs::create_dir(&assets)?;
    fs::write(assets.join("index.html"), "<main>controller</main>")?;
    Ok(FrontendAssets::from_dist(assets)?)
}

#[cfg(not(debug_assertions))]
fn assets(_: &std::path::Path) -> TestResult<FrontendAssets> {
    Ok(FrontendAssets::embedded()?)
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "expensive production Argon2 stress; run explicitly in controller-fault-load CI"]
async fn concurrent_bearer_verification_does_not_starve_async_executor() -> TestResult {
    let directory = TempDir::new()?;
    // Hash before opening SQLite: synchronous fixture work must not delay pool maintenance.
    // Calibrate the heartbeat bound to this machine's unoptimized production KDF cost.
    let started = Instant::now();
    let hash = hash_password("test-only-contention-password")?;
    let hash_duration = started.elapsed();
    let heartbeat_bound = (hash_duration / 2).max(Duration::from_millis(100));
    // Apply schema migrations before imposing the request-phase acquisition bound.
    let options = DatabaseOptions::new(directory.path().join("controller.sqlite3"));
    Database::open(options.clone()).await?.close().await;
    let database = Database::open(
        options
            .with_max_connections(1)
            .with_busy_timeout(Duration::from_millis(100)),
    )
    .await?;
    let store = Store::new(database);
    store
        .insert_administrator_credential(&hash, chrono::Utc::now())
        .await?;
    let auth = AuthService::new(ControllerConfig::default().auth, store)?;
    let router = authenticated_app_router(&assets(directory.path())?, auth);
    let barrier = Arc::new(Barrier::new(9));
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let router = router.clone();
        let barrier = Arc::clone(&barrier);
        requests.spawn(async move {
            let mut request = Request::builder()
                .uri("/api/auth/session")
                .header(
                    header::AUTHORIZATION,
                    "Bearer test-only-contention-password",
                )
                .body(Body::empty())?;
            request
                .extensions_mut()
                .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40_000))));
            barrier.wait().await;
            let response = router.oneshot(request).await?;
            Ok::<_, Box<dyn Error + Send + Sync>>(response.status())
        });
    }

    let started = Instant::now();
    barrier.wait().await;
    let mut previous_tick = Instant::now();
    let mut maximum_gap = Duration::ZERO;
    let mut ticks = 0;
    let mut completed = 0;
    let mut heartbeat = tokio::time::interval(Duration::from_millis(10));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    while !requests.is_empty() {
        tokio::select! {
            biased;
            _ = heartbeat.tick() => {
                let now = Instant::now();
                maximum_gap = maximum_gap.max(now.duration_since(previous_tick));
                previous_tick = now;
                ticks += 1;
            }
            result = requests.join_next() => {
                let status = result.ok_or("request set unexpectedly empty")???;
                assert_eq!(status, StatusCode::OK);
                completed += 1;
            }
        }
    }
    maximum_gap = maximum_gap.max(previous_tick.elapsed());
    eprintln!("production hash: {hash_duration:?}; eight Bearer requests: {:?}; heartbeat ticks: {ticks}; max gap: {maximum_gap:?}; bound: {heartbeat_bound:?}", started.elapsed());
    assert_eq!(completed, 8);
    assert!(
        ticks > 1,
        "executor heartbeat did not advance during verification"
    );
    assert!(
        maximum_gap < heartbeat_bound,
        "Argon2 blocked the async executor: {maximum_gap:?} >= {heartbeat_bound:?}"
    );
    Ok(())
}
