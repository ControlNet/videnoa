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
    let page_size = std::num::NonZeroU16::new(2)
        .ok_or_else(|| std::io::Error::other("recovery page size must be nonzero"))?;
    let reconciler = reconciler(&fixture).with_recovery_page_size(page_size);

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

#[tokio::test]
async fn processing_polls_persist_progress_and_complete_without_losing_attempt_identity(
) -> TestResult {
    // Given: a real HTTP mock worker and durable processing task.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1).await?;
    let state = fixture.task_at(TaskStatus::Processing).await?;
    let original = fixture.store.current_attempt(state.task_id).await?.unwrap();
    let remote_id = original.attempt.remote_job_id.unwrap();
    let sample = crate::mock_videnoa::domain::JobProgress {
        current_frame: 125,
        total_frames: Some(1000),
        fps: 25.0,
        eta_seconds: Some(35.2),
    };
    server
        .set_job(
            &remote_id.to_string(),
            crate::mock_videnoa::domain::JobStatus::Running,
            Some(sample),
        )
        .await?;
    let reconciler = reconciler(&fixture);

    // Wire the real Store observer used by SSE, without any password verification fixture.
    let config_workspace = tempfile::tempdir()?;
    let config =
        videnoa_controller::config::ControllerConfig::from_toml_in("", config_workspace.path())?;
    let events = videnoa_controller::operations::EventHub::new();
    let mut wakeups = events.subscribe_wakeups();
    let _operations = videnoa_controller::operations::OperationsState::new(
        videnoa_controller::operations::OperationsDependencies {
            auth: videnoa_controller::auth::AuthService::new(
                config.auth.clone(),
                fixture.store.clone(),
            )?,
            store: fixture.store.clone(),
            scheduler: videnoa_controller::scheduler::Scheduler::load(fixture.store.clone())?,
            paths: fixture.paths.clone(),
            config,
            events,
            payload_limits: PayloadLimits::new(1024 * 1024, 4096)?,
        },
    );
    // When: a running poll observes actual frame/FPS/ETA values.
    reconciler
        .reconcile_task_id(state.task_id, fixture.now)
        .await?;
    let task = fixture.load_task(state.task_id).await?;
    let attempt = fixture.store.current_attempt(state.task_id).await?.unwrap();
    tokio::time::timeout(Duration::from_secs(1), wakeups.recv()).await??;
    assert!((task.progress.percent - 12.5).abs() < f32::EPSILON);
    assert_eq!(task.progress.processed_frames, Some(125));
    assert_eq!(task.progress.total_frames, Some(1000));
    assert_eq!(task.progress.frames_per_second, Some(25.0));
    assert_eq!(task.progress.eta_seconds, Some(36));
    assert_eq!(task.progress, attempt.attempt.progress);
    assert_eq!(attempt.attempt.id, original.attempt.id);
    assert_eq!(attempt.attempt.remote_job_id, Some(remote_id));

    // Then: unchanged samples do not churn versions, and completion uses fresh CAS versions.
    reconciler
        .reconcile_task_id(state.task_id, fixture.now)
        .await?;
    assert_eq!(
        fixture.load_task(state.task_id).await?.version,
        task.version
    );
    assert!(matches!(
        wakeups.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    server
        .set_job(
            &remote_id.to_string(),
            crate::mock_videnoa::domain::JobStatus::Completed,
            None,
        )
        .await?;
    reconciler
        .reconcile_task_id(state.task_id, fixture.now)
        .await?;
    let completed = fixture.load_task(state.task_id).await?;
    assert_eq!(completed.status, TaskStatus::RemoteCompleted);
    assert!((completed.progress.percent - 100.0).abs() < f32::EPSILON);
    assert_eq!(completed.progress.eta_seconds, Some(0));
    assert_eq!(
        completed.progress,
        fixture
            .store
            .current_attempt(state.task_id)
            .await?
            .unwrap()
            .attempt
            .progress
    );
    assert_eq!(completed.attempt_count, 1);
    Ok(())
}

#[tokio::test]
async fn processing_progress_cannot_race_past_cancellation() -> TestResult {
    use crate::mock_videnoa::checkpoints::Checkpoint;
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1).await?;
    let state = fixture.task_at(TaskStatus::Processing).await?;
    let attempt = fixture.store.current_attempt(state.task_id).await?.unwrap();
    let remote = attempt.attempt.remote_job_id.unwrap();
    server
        .set_job(
            &remote.to_string(),
            crate::mock_videnoa::domain::JobStatus::Running,
            Some(crate::mock_videnoa::domain::JobProgress {
                current_frame: 20,
                total_frames: Some(100),
                fps: 10.0,
                eta_seconds: Some(8.0),
            }),
        )
        .await?;
    let ticket = server.pause(Checkpoint::BeforePollResponse).await;
    let recovery = reconciler(&fixture);
    let now = fixture.now;
    let id = state.task_id;
    let poll = tokio::spawn(async move { recovery.reconcile_task_id(id, now).await });
    server.await_checkpoint(&ticket).await?;
    let task = fixture.load_task(id).await?;
    fixture
        .service
        .request_cancellation(&task, Some(&attempt), now)
        .await?;
    server.release(ticket).await?;
    assert!(matches!(
        poll.await?,
        Err(videnoa_controller::recovery::RecoveryError::Conflict)
    ));
    let current = fixture.load_task(id).await?;
    assert!(current.cancel_requested_at.is_some());
    assert_eq!(current.progress, task.progress);
    assert_eq!(
        fixture
            .store
            .current_attempt(id)
            .await?
            .unwrap()
            .attempt
            .progress,
        attempt.attempt.progress
    );
    Ok(())
}

#[tokio::test]
async fn failed_attempt_progress_write_rolls_back_the_task_update() -> TestResult {
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1).await?;
    let state = fixture.task_at(TaskStatus::Processing).await?;
    let before = fixture.load_task(state.task_id).await?;
    let attempt = fixture.store.current_attempt(state.task_id).await?.unwrap();
    server
        .set_job(
            &attempt.attempt.remote_job_id.unwrap().to_string(),
            crate::mock_videnoa::domain::JobStatus::Running,
            Some(crate::mock_videnoa::domain::JobProgress {
                current_frame: 1,
                total_frames: Some(10),
                fps: 1.0,
                eta_seconds: Some(9.0),
            }),
        )
        .await?;
    sqlx::query("CREATE TRIGGER reject_progress BEFORE UPDATE OF progress_json ON task_attempts BEGIN SELECT RAISE(ABORT, 'injected attempt failure'); END")
        .execute(fixture.store.database().pool()).await?;
    assert!(reconciler(&fixture)
        .reconcile_task_id(state.task_id, fixture.now)
        .await
        .is_err());
    assert_eq!(fixture.load_task(state.task_id).await?, before);
    assert_eq!(
        fixture.store.current_attempt(state.task_id).await?.unwrap(),
        attempt
    );
    Ok(())
}
