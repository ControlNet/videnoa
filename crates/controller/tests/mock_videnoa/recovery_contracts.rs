use std::error::Error;
use std::time::Duration;

use videnoa_controller::domain::{FailureCode, TaskStatus};
use videnoa_controller::recovery::{
    Reconciler, RecoveryCommandKind, RecoveryConfig, ShutdownCoordinator,
};
use videnoa_controller::remote::{PayloadLimits, RemoteTimeouts};

use super::mock_videnoa::faults::{Fault, OfflineMode, ResponseFault};
use super::mock_videnoa::journal::Route;
use super::mock_videnoa::server::MockVidenoa;
use super::recovery_support::Fixture;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test]
async fn malformed_tasks_fail_independently_without_aborting_startup() -> TestResult {
    // Given: three malformed durable tasks and one unrelated queued task.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 8).await?;
    let missing_attempt = fixture.task_at(TaskStatus::Reserved).await?;
    let missing_worker = fixture.task_at(TaskStatus::Reserved).await?;
    let missing_remote = fixture.task_at(TaskStatus::Processing).await?;
    let queued = fixture.task_at(TaskStatus::Queued).await?;
    sqlx::query("DELETE FROM task_attempts WHERE task_id = ?")
        .bind(missing_attempt.task_id.to_string())
        .execute(fixture.store.database().pool())
        .await?;
    sqlx::query("UPDATE tasks SET worker_id = NULL WHERE id = ?")
        .bind(missing_worker.task_id.to_string())
        .execute(fixture.store.database().pool())
        .await?;
    sqlx::query(
        "UPDATE task_attempts SET remote_job_id = NULL, remote_input_path = NULL, remote_output_path = NULL WHERE task_id = ?",
    )
    .bind(missing_remote.task_id.to_string())
    .execute(fixture.store.database().pool())
    .await?;

    // When: startup scans the complete durable batch.
    let report = reconciler(&fixture, ShutdownCoordinator::new())
        .reconcile_startup(fixture.now)
        .await?;

    // Then: malformed rows are actionable ambiguity and unrelated work still dispatches.
    assert_eq!(
        report.command_kind(queued.task_id),
        Some(RecoveryCommandKind::AwaitReservation)
    );
    for task_id in [
        missing_attempt.task_id,
        missing_worker.task_id,
        missing_remote.task_id,
    ] {
        let task = fixture.load_task(task_id).await?;
        assert_eq!(task.status, TaskStatus::Failed);
        assert_eq!(
            task.failure.map(|failure| failure.failure_code),
            Some(FailureCode::RemoteStateAmbiguous)
        );
    }
    Ok(())
}

#[tokio::test]
async fn contradictory_remote_identity_is_nonretryable_ambiguity() -> TestResult {
    // Given: durable processing evidence that disagrees with the remote job payload.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 2).await?;
    let state = fixture.task_at(TaskStatus::Processing).await?;
    server
        .set_fault(Fault::Response(ResponseFault {
            route: Route::JobPoll,
            status: 200,
            body: serde_json::to_vec(&serde_json::json!({
                "id": "00000000-0000-4000-8000-000000000001",
                "status": "running",
                "created_at": "2026-09-02T00:00:00Z",
                "started_at": "2026-09-02T00:00:01Z",
                "completed_at": null,
                "progress": null,
                "error": null,
                "workflow_name": "other-workflow.json",
                "workflow_source": "workflow",
                "params": {"input": "other/input.mkv", "output": "other/output.mp4"},
                "rerun_of_job_id": null,
                "duration_ms": null
            }))?,
        }))
        .await;

    // When: recovery polls the known remote job.
    reconciler(&fixture, ShutdownCoordinator::new())
        .reconcile_startup(fixture.now)
        .await?;

    // Then: contradictory evidence fails closed without replaying submission.
    let task = fixture.load_task(state.task_id).await?;
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(
        task.failure.map(|failure| failure.failure_code),
        Some(FailureCode::RemoteStateAmbiguous)
    );
    assert_eq!(server.counters().await.get(Route::Run), 1);
    Ok(())
}

#[tokio::test]
async fn submitting_cancellation_uses_typed_reconciliation_and_never_polls() -> TestResult {
    // Given: a submitting task with durable cancellation intent.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 2).await?;
    let state = fixture.task_at(TaskStatus::Submitting).await?;
    request_cancellation(&fixture, state.task_id).await?;

    // When: keyed replay proves the submission was accepted.
    let report = reconciler(&fixture, ShutdownCoordinator::new())
        .reconcile_startup(fixture.now)
        .await?;

    // Then: remote cancellation completes and ordinary polling is never exposed.
    assert_ne!(
        report.command_kind(state.task_id),
        Some(RecoveryCommandKind::Poll)
    );
    assert_eq!(
        fixture.load_task(state.task_id).await?.status,
        TaskStatus::Cancelled
    );
    assert_eq!(server.counters().await.get(Route::JobCancel), 1);
    Ok(())
}

#[tokio::test]
async fn rejected_submitting_cancellation_cleans_locally_without_polling() -> TestResult {
    // Given: a cancelling submission whose worker rejects the keyed request before acceptance.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 2).await?;
    let state = fixture.task_at(TaskStatus::Submitting).await?;
    request_cancellation(&fixture, state.task_id).await?;
    sqlx::query("UPDATE tasks SET workflow = 'missing-workflow.json' WHERE id = ?")
        .bind(state.task_id.to_string())
        .execute(fixture.store.database().pool())
        .await?;

    // When: recovery proves the submission was not accepted.
    let report = reconciler(&fixture, ShutdownCoordinator::new())
        .reconcile_startup(fixture.now)
        .await?;

    // Then: cancellation finishes from staged cleanup without remote polling or cancellation.
    assert_ne!(
        report.command_kind(state.task_id),
        Some(RecoveryCommandKind::Poll)
    );
    assert_eq!(
        fixture.load_task(state.task_id).await?.status,
        TaskStatus::Cancelled
    );
    assert_eq!(server.counters().await.get(Route::JobCancel), 0);
    Ok(())
}

#[tokio::test]
async fn processing_cancellation_cancels_known_job_without_polling() -> TestResult {
    // Given: a processing task with durable cancellation intent.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 2).await?;
    let state = fixture.task_at(TaskStatus::Processing).await?;
    request_cancellation(&fixture, state.task_id).await?;

    // When: startup reconciliation reaches the known remote job.
    let report = reconciler(&fixture, ShutdownCoordinator::new())
        .reconcile_startup(fixture.now)
        .await?;

    // Then: cancellation is durable, remote compute is removed, and Poll is absent.
    assert_ne!(
        report.command_kind(state.task_id),
        Some(RecoveryCommandKind::Poll)
    );
    assert_eq!(
        fixture.load_task(state.task_id).await?.status,
        TaskStatus::Cancelled
    );
    assert_eq!(server.counters().await.get(Route::JobCancel), 1);
    Ok(())
}

#[tokio::test]
async fn worker_outage_retry_count_stops_at_configured_bound() -> TestResult {
    // Given: assigned processing work whose worker remains offline beyond retry bounds.
    let mut server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 2).await?;
    let state = fixture.task_at(TaskStatus::Processing).await?;
    server.set_offline(OfflineMode::ServiceUnavailable).await?;
    let reconciler = reconciler(&fixture, ShutdownCoordinator::new());

    // When: startup recovery repeats beyond the configured maximum.
    for _ in 0..5 {
        reconciler.reconcile_startup(fixture.now).await?;
    }

    // Then: retries are capped, scheduling stops, and assignment capacity remains reserved.
    let worker = fixture
        .store
        .worker(fixture.worker_id)
        .await?
        .ok_or_else(|| std::io::Error::other("worker missing"))?;
    assert_eq!(worker.health_retry_count, 3);
    assert_eq!(worker.next_health_check_at, None);
    let task = fixture.load_task(state.task_id).await?;
    assert_eq!(task.status, TaskStatus::Processing);
    assert_eq!(task.worker_id, Some(fixture.worker_id));
    assert_eq!(fixture.store.worker_used_slots(fixture.worker_id).await?, 1);
    Ok(())
}

async fn request_cancellation(
    fixture: &Fixture,
    task_id: videnoa_controller::domain::TaskId,
) -> TestResult {
    let task = fixture.load_task(task_id).await?;
    let attempt = fixture
        .store
        .current_attempt(task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("attempt missing"))?;
    fixture
        .service
        .request_cancellation(&task, Some(&attempt), fixture.now)
        .await?;
    Ok(())
}

fn reconciler(fixture: &Fixture, shutdown: ShutdownCoordinator) -> Reconciler {
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
