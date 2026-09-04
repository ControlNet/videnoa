use std::collections::BTreeMap;
use std::error::Error;
use std::time::Duration;

use videnoa_controller::domain::{FailureCode, TaskStatus};
use videnoa_controller::lifecycle::{Lifecycle, RecoveryAction};
use videnoa_controller::recovery::{
    Reconciler, RecoveryCommandKind, RecoveryConfig, ShutdownCoordinator,
};
use videnoa_controller::remote::{PayloadLimits, RemoteTimeouts};

use super::mock_videnoa::faults::{OfflineMode, RestartMode};
use super::mock_videnoa::journal::Route;
use super::mock_videnoa::server::MockVidenoa;
use super::recovery_support::Fixture;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[test]
fn restart_boundary_dispatch_is_exhaustive_for_every_nonterminal_state() {
    // Given: every nonterminal lifecycle state and Task 9's exact recovery action.
    let cases = [
        (TaskStatus::Queued, RecoveryCommandKind::AwaitReservation),
        (TaskStatus::Reserved, RecoveryCommandKind::Upload),
        (TaskStatus::Uploading, RecoveryCommandKind::Upload),
        (TaskStatus::Staged, RecoveryCommandKind::Submit),
        (TaskStatus::Submitting, RecoveryCommandKind::Submit),
        (TaskStatus::Processing, RecoveryCommandKind::Poll),
        (TaskStatus::RemoteCompleted, RecoveryCommandKind::Download),
        (TaskStatus::Downloading, RecoveryCommandKind::Download),
        (TaskStatus::Verifying, RecoveryCommandKind::Verify),
        (TaskStatus::Publishing, RecoveryCommandKind::Publish),
        (TaskStatus::RemoteCleanup, RecoveryCommandKind::Cleanup),
    ];

    // When: the recovery command classifier consumes each exact Task 9 action.
    for (status, expected) in cases {
        let action = Lifecycle::recovery(status);
        let command = RecoveryCommandKind::for_action(action);

        // Then: no state falls through a wildcard and each command stays stage-typed.
        assert_eq!(command, expected);
    }
    assert_eq!(
        Lifecycle::recovery(TaskStatus::Completed),
        RecoveryAction::Completed
    );
    assert_eq!(
        Lifecycle::recovery(TaskStatus::Failed),
        RecoveryAction::Failed
    );
    assert_eq!(
        Lifecycle::recovery(TaskStatus::Cancelled),
        RecoveryAction::Cancelled
    );
}

#[tokio::test]
async fn startup_scans_durable_tasks_and_dispatches_recovery_commands() -> TestResult {
    // Given: one durable task at every nonterminal restart boundary.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 16).await?;
    let mut expected = BTreeMap::new();
    for (status, kind) in [
        (TaskStatus::Queued, RecoveryCommandKind::AwaitReservation),
        (TaskStatus::Reserved, RecoveryCommandKind::Upload),
        (TaskStatus::Uploading, RecoveryCommandKind::Upload),
        (TaskStatus::Staged, RecoveryCommandKind::Poll),
        (TaskStatus::Submitting, RecoveryCommandKind::Poll),
        (TaskStatus::Processing, RecoveryCommandKind::Poll),
        (TaskStatus::RemoteCompleted, RecoveryCommandKind::Download),
        (TaskStatus::Downloading, RecoveryCommandKind::Download),
        (TaskStatus::Verifying, RecoveryCommandKind::Verify),
        (TaskStatus::Publishing, RecoveryCommandKind::Publish),
        (TaskStatus::RemoteCleanup, RecoveryCommandKind::Cleanup),
    ] {
        let state = fixture.task_at(status).await?;
        expected.insert(state.task_id, kind);
    }
    let reconciler = reconciler(&fixture);

    // When: startup reconciliation scans SQLite rather than an in-memory queue.
    let report = reconciler.reconcile_startup(fixture.now).await?;

    // Then: every durable row is visited and only stage-typed work is exposed.
    assert_eq!(report.traces().len(), expected.len());
    for (task_id, kind) in expected {
        assert_eq!(report.command_kind(task_id), Some(kind));
    }
    Ok(())
}

#[tokio::test]
async fn worker_outage_persists_backoff_and_retains_assignment_capacity() -> TestResult {
    // Given: one processing task assigned to a worker that becomes unavailable.
    let mut server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 2).await?;
    let state = fixture.task_at(TaskStatus::Processing).await?;
    server.set_offline(OfflineMode::ServiceUnavailable).await?;
    let reconciler = reconciler(&fixture);

    // When: reconciliation reaches the offline worker.
    let report = reconciler.reconcile_startup(fixture.now).await?;

    // Then: health backoff is durable while assignment and used capacity remain reserved.
    assert_eq!(report.deferred().len(), 1);
    let worker = fixture
        .store
        .worker(fixture.worker_id)
        .await?
        .ok_or_else(|| std::io::Error::other("worker missing"))?;
    assert!(!worker.online);
    assert_eq!(worker.health_retry_count, 1);
    assert!(worker.next_health_check_at > Some(fixture.now));
    let task = fixture.load_task(state.task_id).await?;
    assert_eq!(task.status, TaskStatus::Processing);
    assert_eq!(task.worker_id, Some(fixture.worker_id));
    assert_eq!(fixture.store.worker_used_slots(fixture.worker_id).await?, 1);
    Ok(())
}

#[tokio::test]
async fn missing_remote_job_after_worker_state_loss_is_nonretryable_ambiguity() -> TestResult {
    // Given: a processing attempt whose remote worker loses its durable job database.
    let mut server = MockVidenoa::start_persistent().await?;
    let fixture = Fixture::new(&server, 2).await?;
    let state = fixture.task_at(TaskStatus::Processing).await?;
    server.restart(RestartMode::LoseState).await?;
    let run_count = server.counters().await.get(Route::Run);

    // When: startup reconciliation polls only the known durable remote job identifier.
    reconciler(&fixture).reconcile_startup(fixture.now).await?;

    // Then: the task closes as nonretryable ambiguity and no submission is replayed.
    let task = fixture.load_task(state.task_id).await?;
    let failure = task
        .failure
        .ok_or_else(|| std::io::Error::other("failure missing"))?;
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(failure.failure_code, FailureCode::RemoteStateAmbiguous);
    assert!(!failure.retryable);
    assert_eq!(server.counters().await.get(Route::Run), run_count);
    Ok(())
}

#[tokio::test]
async fn restart_cancelled_remote_job_requires_explicit_processing_retry() -> TestResult {
    // Given: a processing job cancelled by a state-retaining worker restart.
    let mut server = MockVidenoa::start_persistent().await?;
    let fixture = Fixture::new(&server, 2).await?;
    let state = fixture.task_at(TaskStatus::Processing).await?;
    server.restart(RestartMode::RetainState).await?;

    // When: reconciliation observes the known job as cancelled.
    reconciler(&fixture).reconcile_startup(fixture.now).await?;

    // Then: the attempt history closes with a retryable processing failure.
    let task = fixture.load_task(state.task_id).await?;
    let failure = task
        .failure
        .ok_or_else(|| std::io::Error::other("failure missing"))?;
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(failure.failure_code, FailureCode::ProcessingFailed);
    assert!(failure.retryable);
    assert_eq!(task.attempt_count, 1);
    Ok(())
}

fn reconciler(fixture: &Fixture) -> Reconciler {
    let timeouts = RemoteTimeouts::new(
        Duration::from_secs(1),
        Duration::from_secs(2),
        Duration::from_secs(1),
    )
    .expect("nonzero recovery timeouts");
    let limits = PayloadLimits::new(1024 * 1024, 4096).expect("nonzero recovery limits");
    Reconciler::new(
        fixture.store.clone(),
        RecoveryConfig::new(
            fixture.paths.clone(),
            timeouts,
            limits,
            Duration::from_secs(2),
            Duration::from_secs(8),
            3,
        ),
        ShutdownCoordinator::new(),
    )
}
