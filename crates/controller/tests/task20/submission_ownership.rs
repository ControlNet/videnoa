use std::sync::Arc;

use reqwest::StatusCode;
use serde_json::json;
use videnoa_controller::domain::TaskStatus;
use videnoa_controller::lifecycle::LifecycleService;
use videnoa_controller::scheduler::{TransferCheckpointObserver, TransferCheckpointPoint};

use crate::mock_videnoa::api::MockClient;
use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::faults::Fault;
use crate::mock_videnoa::journal::{HeaderValueSnapshot, Route};
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{
    assert_completed_pipeline, assert_restarted_pipeline, coherent_task_attempt, complete_mock_job,
    lifecycle_operation_error, CheckpointGate, ControllerFixture, TestResult,
};

#[tokio::test]
async fn same_key_replay_maps_to_one_remote_job() -> TestResult {
    // Given: one persistent worker and a stable submission key.
    let worker = MockVidenoa::start_persistent().await?;
    let client = MockClient::new(worker.base_url())?;
    let params = json!({"input": "task/input.mkv", "output": "task/output.mp4"});

    // When: the same request is replayed with the same key.
    let created = client
        .run("eligible-workflow.json", "durable-key", params.clone())
        .await?;
    let replayed = client
        .run("eligible-workflow.json", "durable-key", params)
        .await?;

    // Then: two requests resolve to one durable remote job.
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(replayed.status, StatusCode::OK);
    assert_eq!(created.body.id, replayed.body.id);
    assert_eq!(worker.counters().await.get(Route::Run), 2);
    assert_eq!(worker.job_count().await, 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn normal_attempt_submits_exactly_once() -> TestResult {
    // Given: one API-registered worker with its first response held after acceptance.
    let worker = MockVidenoa::start_persistent().await?;
    let fixture = ControllerFixture::start().await?;
    fixture
        .register_worker(&worker, "single-submit-baseline")
        .await?;
    let run = worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;

    // When: one task reaches durable remote acceptance without timing out.
    let task = fixture
        .create_task("single-submit-baseline", b"input-video")
        .await?;
    worker.await_checkpoint(&run).await?;

    // Then: one durable attempt owns one request and one remote job.
    let durable_task = fixture
        .store
        .task(task.id)
        .await?
        .ok_or_else(|| std::io::Error::other("durable task is missing"))?;
    let durable_attempt = fixture
        .store
        .current_attempt(task.id)
        .await?
        .ok_or_else(|| std::io::Error::other("durable attempt is missing"))?;
    assert_eq!(durable_task.status, TaskStatus::Submitting);
    assert_eq!(durable_attempt.attempt.status, TaskStatus::Submitting);
    assert_eq!(worker.counters().await.get(Route::Run), 1);
    assert_eq!(worker.job_count().await, 1);
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    worker.release(run).await?;
    assert_completed_pipeline(&fixture, &worker, &task, b"enhanced-video").await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn timed_out_submission_retries_in_same_generation_after_backoff() -> TestResult {
    // Given: one durable attempt stopped immediately before its first remote submission.
    let gate = CheckpointGate::new(TransferCheckpointPoint::BeforeRemoteSubmit);
    let observer: Arc<dyn TransferCheckpointObserver> = gate.clone();
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start_with_checkpoint_observer(Some(observer)).await?;
    fixture.register_worker(&worker, "submission-owner").await?;
    let task = fixture
        .create_task("submission-owner", b"input-video")
        .await?;
    gate.wait().await?;
    assert_eq!(fixture.task(&task).await?.task.status, TaskStatus::Staged);
    fixture.crash().await?;
    gate.release();
    worker.set_fault(Fault::AcceptThenDropRunResponse).await;
    let reconciler = fixture.reconciler()?;

    // When: the same Controller generation re-enters submission after uncertain acceptance.
    assert!(reconciler
        .reconcile_task_id(task.id, chrono::Utc::now())
        .await
        .is_err());
    let report = reconciler
        .reconcile_task_id(task.id, chrono::Utc::now())
        .await?;

    // Then: retries expose a durable deadline and do not run before it.
    let durable_task = fixture
        .store
        .task(task.id)
        .await?
        .ok_or_else(|| std::io::Error::other("durable task is missing"))?;
    let durable_attempt = fixture
        .store
        .current_attempt(task.id)
        .await?
        .ok_or_else(|| std::io::Error::other("durable attempt is missing"))?;
    assert_eq!(durable_task.status, TaskStatus::Submitting);
    assert_eq!(durable_attempt.attempt.status, TaskStatus::Submitting);
    assert!(report
        .deferred()
        .iter()
        .any(|deferred| deferred.task_id == task.id));
    assert_eq!(worker.counters().await.get(Route::Run), 1);
    assert_eq!(worker.job_count().await, 1);

    assert_eq!(durable_attempt.attempt.retry.retry_count, 1);
    assert_eq!(durable_task.retry, durable_attempt.attempt.retry);
    let retry_at = durable_attempt
        .attempt
        .retry
        .next_retry_at
        .expect("retry deadline");

    // When: the same generation reaches the durable retry deadline.
    reconciler.reconcile_task_id(task.id, retry_at).await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    // Then: replay has already recovered the original attempt before runtime restart.
    assert_eq!(
        fixture.store.task(task.id).await?.expect("task").status,
        TaskStatus::Processing
    );
    let recovered = fixture
        .store
        .current_attempt(task.id)
        .await?
        .expect("attempt");
    assert_eq!(recovered.attempt.retry.retry_count, 0);
    assert!(recovered.attempt.retry.next_retry_at.is_none());
    fixture.restart().await?;
    assert_restarted_pipeline(&fixture, &worker, &task, b"enhanced-video").await?;
    let keys = worker
        .journal()
        .await
        .into_iter()
        .filter(|entry| entry.route == Route::Run)
        .filter_map(|entry| {
            entry
                .headers
                .into_iter()
                .find_map(|header| (header.name == "idempotency-key").then_some(header.value))
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        keys.as_slice(),
        [HeaderValueSnapshot::Bytes(first), HeaderValueSnapshot::Bytes(second)] if first == second
    ));
    assert_eq!(worker.counters().await.get(Route::Run), 2);
    assert_eq!(worker.job_count().await, 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn same_generation_cancellation_retries_uncertain_submission_after_backoff() -> TestResult {
    // Given: one submission accepted remotely after its Controller generation lost the response.
    let gate = CheckpointGate::new(TransferCheckpointPoint::BeforeRemoteSubmit);
    let observer: Arc<dyn TransferCheckpointObserver> = gate.clone();
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start_with_checkpoint_observer(Some(observer)).await?;
    fixture
        .register_worker(&worker, "owned-cancellation")
        .await?;
    let task = fixture
        .create_task("owned-cancellation", b"input-video")
        .await?;
    gate.wait().await?;
    fixture.crash().await?;
    gate.release();
    worker.set_fault(Fault::AcceptThenDropRunResponse).await;
    let reconciler = fixture.reconciler()?;
    assert!(reconciler
        .reconcile_task_id(task.id, chrono::Utc::now())
        .await
        .is_err());
    let (durable_task, durable_attempt) = coherent_task_attempt(
        &fixture,
        &task,
        TaskStatus::Submitting,
        "request owned submission cancellation",
    )
    .await?;
    LifecycleService::new(fixture.store.clone())
        .request_cancellation(&durable_task, Some(&durable_attempt), chrono::Utc::now())
        .await
        .map_err(|error| {
            lifecycle_operation_error(
                "request owned submission cancellation",
                &durable_task,
                &durable_attempt,
                error,
            )
        })?;

    // When: cancellation reconciliation re-enters through that same generation.
    let report = reconciler
        .reconcile_task_id(task.id, chrono::Utc::now())
        .await?;

    // Then: ownership defers cancellation without replaying submission or cancelling unknown work.
    assert!(report
        .deferred()
        .iter()
        .any(|deferred| deferred.task_id == task.id));
    assert_eq!(
        fixture
            .store
            .task(task.id)
            .await?
            .ok_or_else(|| std::io::Error::other("durable task is missing"))?
            .status,
        TaskStatus::Submitting
    );
    let counters = worker.counters().await;
    assert_eq!(counters.get(Route::Run), 1);
    assert_eq!(counters.get(Route::JobCancel), 0);
    assert_eq!(worker.job_count().await, 1);
    let retry_at = fixture
        .store
        .current_attempt(task.id)
        .await?
        .expect("attempt")
        .attempt
        .retry
        .next_retry_at
        .expect("retry deadline");
    reconciler.reconcile_task_id(task.id, retry_at).await?;
    assert_eq!(
        fixture.store.task(task.id).await?.expect("task").status,
        TaskStatus::Cancelled
    );
    assert_eq!(worker.counters().await.get(Route::Run), 2);
    assert_eq!(worker.counters().await.get(Route::JobCancel), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn orchestration_recovers_lost_submission_response_without_restart() -> TestResult {
    // The isolated mock accepts exactly one job and drops its first response.
    let worker = MockVidenoa::start_persistent().await?;
    worker.set_fault(Fault::AcceptThenDropRunResponse).await;
    let fixture = ControllerFixture::start().await?;
    fixture
        .register_worker(&worker, "automatic-submission-retry")
        .await?;
    let task = fixture
        .create_task("automatic-submission-retry", b"input-video")
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            if fixture.store.task(task.id).await?.expect("task").status == TaskStatus::Processing {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await??;
    assert_eq!(worker.counters().await.get(Route::Run), 2);
    assert_eq!(worker.job_count().await, 1);
    assert_eq!(
        fixture
            .store
            .task(task.id)
            .await?
            .expect("task")
            .attempt_count,
        1
    );
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    assert_restarted_pipeline(&fixture, &worker, &task, b"enhanced-video").await
}
