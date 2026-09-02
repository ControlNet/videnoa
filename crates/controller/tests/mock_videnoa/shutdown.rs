use std::error::Error;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use tempfile::TempDir;
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};
use videnoa_controller::recovery::{DrainOutcome, Reconciler, RecoveryConfig, ShutdownCoordinator};
use videnoa_controller::remote::{PayloadLimits, RemoteTimeouts};
use videnoa_controller::scheduler::Scheduler;

use super::mock_videnoa::domain::JobStatus;
use super::mock_videnoa::server::MockVidenoa;
use super::recovery_support::Fixture;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(start_paused = true)]
async fn shutdown_stops_new_stage_work_and_drains_durable_writes() -> TestResult {
    // Given: one admitted stage with an outstanding durable write.
    let coordinator = ShutdownCoordinator::new();
    let stage = coordinator
        .begin_stage()
        .ok_or_else(|| std::io::Error::other("stage gate unexpectedly closed"))?;
    let write = stage.begin_write();

    // When: shutdown begins before the durable write completes.
    coordinator.stop_stage_intake();
    assert!(coordinator.begin_stage().is_none());
    drop(write);
    drop(stage);
    let outcome = coordinator.drain(Duration::from_secs(5)).await;

    // Then: the tracked write drains and stage intake remains closed.
    assert_eq!(outcome, DrainOutcome::Drained);
    assert!(coordinator.begin_stage().is_none());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn shutdown_timeout_leaves_outstanding_work_recoverable() -> TestResult {
    // Given: an admitted stage whose durable write remains outstanding.
    let coordinator = ShutdownCoordinator::new();
    let stage = coordinator
        .begin_stage()
        .ok_or_else(|| std::io::Error::other("stage gate unexpectedly closed"))?;
    let _write = stage.begin_write();

    // When: shutdown drains for a shorter bound than the outstanding write.
    coordinator.stop_stage_intake();
    let outcome = coordinator.drain(Duration::from_secs(1)).await;

    // Then: shutdown reports the bounded timeout without reopening stage intake.
    assert_eq!(
        outcome,
        DrainOutcome::TimedOut {
            outstanding_writes: 1
        }
    );
    assert!(coordinator.begin_stage().is_none());
    Ok(())
}

#[tokio::test]
async fn graceful_shutdown_persists_pause_before_bounded_drain() -> TestResult {
    // Given: a running Controller store with scheduler intake initially unpaused.
    let directory = TempDir::new()?;
    let database = Database::open(DatabaseOptions::new(
        directory.path().join("controller.sqlite3"),
    ))
    .await?;
    let store = Store::new(database);
    let scheduler = Scheduler::load(store.clone()).await?;
    let coordinator = ShutdownCoordinator::new();
    let now = Utc
        .timestamp_opt(1_788_307_200, 0)
        .single()
        .ok_or_else(|| std::io::Error::other("invalid timestamp"))?;

    // When: graceful shutdown stops intake and drains durable writes.
    let outcome = coordinator
        .shutdown(&scheduler, now, Duration::from_secs(5))
        .await?;

    // Then: the durable pause is committed and no new stage work is admitted.
    assert_eq!(outcome, DrainOutcome::Drained);
    assert!(store.settings().await?.scheduler.paused);
    assert!(coordinator.begin_stage().is_none());
    Ok(())
}

#[tokio::test]
async fn shutdown_tracks_a_real_blocked_recovery_write() -> TestResult {
    // Given: completed remote work whose recovery transition is blocked by a SQLite writer.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 2).await?;
    let state = fixture
        .task_at(videnoa_controller::domain::TaskStatus::Processing)
        .await?;
    let attempt = fixture
        .store
        .current_attempt(state.task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("attempt missing"))?;
    let job_id = attempt
        .attempt
        .remote_job_id
        .ok_or_else(|| std::io::Error::other("remote job missing"))?;
    server
        .set_job(&job_id.to_string(), JobStatus::Completed, None)
        .await?;
    let mut lock = fixture.store.database().pool().acquire().await?;
    sqlx::query("BEGIN IMMEDIATE").execute(&mut *lock).await?;
    let coordinator = ShutdownCoordinator::new();
    let reconciler = test_reconciler(&fixture, coordinator.clone());
    let now = fixture.now;

    // When: reconciliation reaches its durable transition while shutdown starts draining.
    let recovery = tokio::spawn(async move { reconciler.reconcile_startup(now).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while coordinator.outstanding_writes() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    coordinator.stop_stage_intake();
    let outcome = coordinator.drain(Duration::from_millis(10)).await;

    // Then: the actual recovery write is counted until SQLite releases it.
    assert_eq!(
        outcome,
        DrainOutcome::TimedOut {
            outstanding_writes: 1
        }
    );
    sqlx::query("ROLLBACK").execute(&mut *lock).await?;
    recovery.await??;
    assert_eq!(coordinator.outstanding_writes(), 0);
    Ok(())
}

fn test_reconciler(fixture: &Fixture, shutdown: ShutdownCoordinator) -> Reconciler {
    Reconciler::new(
        fixture.store.clone(),
        RecoveryConfig::new(
            RemoteTimeouts::new(
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(1),
            )
            .expect("nonzero timeouts"),
            PayloadLimits::new(1024 * 1024, 4096).expect("nonzero limits"),
            Duration::from_secs(2),
            Duration::from_secs(8),
            3,
        ),
        shutdown,
    )
}
