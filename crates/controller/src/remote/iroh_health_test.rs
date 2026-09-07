//! Health-service regression using the real CONNECT fixture in the parent test.
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use crate::domain::{WorkerApiUrl, WorkerId};
use crate::operations::EventHub;
use crate::persistence::{Database, DatabaseOptions, Store, WorkerRecord};
use crate::recovery::ShutdownCoordinator;
use crate::remote::PayloadLimits;
use crate::scheduler::RuntimeSettings;
use crate::workers::{WorkerHealthService, WorkerRegistry};
use chrono::Utc;
use serde_json::json;

pub(super) async fn wrong_password_health_blocks_until_edit(
    url: WorkerApiUrl,
    password: &str,
    failures: Arc<AtomicUsize>,
) -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let database =
        Database::open(DatabaseOptions::new(root.path().join("controller.sqlite3"))).await?;
    let store = Store::new(database.clone());
    let registry = WorkerRegistry::new(store.clone());
    // Synthetic password and isolated SQLite registration; no production worker is contacted.
    let mut request = json!({"name":"iroh-health", "transport":"iroh",
        "endpoint_id":url.iroh_id().unwrap().to_string(),
        "password":"incorrect health test credential", "enabled":true, "compute_slots":1});
    let worker = registry
        .create(serde_json::from_value(request.clone())?, Utc::now())
        .await?;
    let settings = store.config_manager().settings()?;
    let mut timeouts = settings.timeouts;
    timeouts.health_seconds = 1;
    let mut retry = settings.retry;
    retry.initial_seconds = 1;
    retry.maximum_seconds = 1;
    let runtime = RuntimeSettings::new(&timeouts, &retry)?;
    let shutdown = ShutdownCoordinator::new();
    let events = EventHub::new();
    let service = WorkerHealthService::new(
        store.clone(),
        runtime,
        PayloadLimits::new(4096, 4096)?,
        shutdown.clone(),
        &events,
    );
    let before = failures.load(Ordering::SeqCst);
    let task = tokio::spawn(service.run());
    let failed = wait_for(&store, worker.id, |record| record.last_error.is_some()).await?;
    assert!(!failed.online);
    assert_eq!(
        failed.last_error.as_deref(),
        Some("worker authentication failed; check the saved worker password")
    );
    assert_eq!(failures.load(Ordering::SeqCst), before + 1);
    tokio::time::sleep(Duration::from_millis(3200)).await;
    let blocked = store.worker(worker.id).await?.unwrap();
    assert_eq!(blocked.version, failed.version);
    assert_eq!(failures.load(Ordering::SeqCst), before + 1);
    request["version"] = json!(blocked.version);
    request["password"] = json!(password);
    let edited = registry
        .update(worker.id, serde_json::from_value(request)?, Utc::now())
        .await?;
    assert!(edited.version > blocked.version);
    // A fresh cadence sees the changed version even without a UI wakeup event.
    let online = wait_for(&store, worker.id, |record| record.online).await?;
    assert!(online.version > edited.version);
    assert!(online.last_error.is_none());
    assert_eq!(failures.load(Ordering::SeqCst), before + 1);
    shutdown.stop_stage_intake();
    tokio::time::timeout(Duration::from_secs(5), task).await???;
    database.close().await;
    Ok(())
}

async fn wait_for(
    store: &Store,
    id: WorkerId,
    predicate: impl Fn(&WorkerRecord) -> bool,
) -> anyhow::Result<WorkerRecord> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let worker = store
                .worker(id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("test worker missing"))?;
            if predicate(&worker) {
                return Ok(worker);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?
}

pub(super) fn counted_authorizer(
    password: &'static str,
    failures: Arc<AtomicUsize>,
) -> videnoa_transport::Authorizer {
    Arc::new(move |_, supplied| {
        let failures = failures.clone();
        Box::pin(async move {
            if supplied == password {
                Ok(())
            } else {
                failures.fetch_add(1, Ordering::SeqCst);
                Err(videnoa_transport::TunnelError::Unauthorized)
            }
        })
    })
}
