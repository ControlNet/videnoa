//! Remote errors have a scope. A reply about the Worker itself (credentials,
//! rate limits, a health reply that is not OK, an incompatible transport)
//! defers its tasks; only evidence about the job may fail a task, and no
//! remote reply may stop the Controller.
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use videnoa_controller::domain::{Task, TaskStatus, WorkerId};
use videnoa_controller::lifecycle::LifecycleService;
use videnoa_controller::persistence::{TaskRecord, WorkerRecord};
use videnoa_controller::recovery::{RecoveryCommandKind, RecoveryReport};
use videnoa_controller::scheduler::{TransferCheckpointObserver, TransferCheckpointPoint};

use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::faults::{DeleteOutcome, Fault, ResponseFault};
use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{
    assert_completed_pipeline, complete_mock_job, CheckpointGate, ControllerFixture, TestResult,
};

const AUTHENTICATION_FAILED: &str = "worker authentication failed; check the saved worker password";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn health_replies_that_are_not_ok_defer_reconciliation() -> TestResult {
    // Given: a processing task reconciled only by this test.
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start().await?;
    let task = processing_task(&worker, &fixture, "health-reply").await?;
    fixture.crash().await?;
    let reconciler = fixture.reconciler()?;

    for (status, body) in [
        (200, br#"{"status":"degraded"}"#.to_vec()),
        (200, b"<html>proxy login</html>".to_vec()),
        (404, Vec::new()),
        (302, Vec::new()),
        (429, Vec::new()),
    ] {
        fault(&worker, Route::Health, status, body).await;

        // When: reconciliation probes the worker.
        let report = reconciler
            .reconcile_task_id(task.id, Utc::now())
            .await
            .map_err(|error| format!("health HTTP {status}: {error}"))?;

        // Then: the task waits for the worker instead of failing or stopping the Controller.
        assert!(deferred(&report, &task), "health HTTP {status}");
        assert_eq!(
            durable_task(&fixture, &task).await?.status,
            TaskStatus::Processing
        );
    }
    let record = durable_worker(&fixture, &task).await?;
    assert!(!record.online);
    assert!(record.health_retry_count > 0);
    assert_eq!(worker.active_job_count().await, 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn health_rejecting_credentials_blocks_the_worker() -> TestResult {
    // Given: a processing task reconciled only by this test.
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start().await?;
    let task = processing_task(&worker, &fixture, "health-auth").await?;
    fixture.crash().await?;
    fault(&worker, Route::Health, 401, Vec::new()).await;

    // When: reconciliation probes the worker.
    let report = fixture
        .reconciler()?
        .reconcile_task_id(task.id, Utc::now())
        .await?;

    // Then: the worker is blocked on its credentials until the health
    // service sees them work, and the task waits.
    assert!(deferred(&report, &task));
    assert_auth_blocked(&durable_worker(&fixture, &task).await?);
    assert_eq!(
        durable_task(&fixture, &task).await?.status,
        TaskStatus::Processing
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejected_remote_cancel_defers_instead_of_stopping_the_controller() -> TestResult {
    // Given: a processing task with a requested cancellation.
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start().await?;
    let task = processing_task(&worker, &fixture, "cancel-reject").await?;
    fixture.crash().await?;
    request_cancellation(&fixture, &task).await?;
    let reconciler = fixture.reconciler()?;

    for status in [400, 409, 403] {
        fault(&worker, Route::JobCancel, status, Vec::new()).await;

        // When: the worker rejects the remote cancel.
        let report = reconciler
            .reconcile_task_id(task.id, Utc::now())
            .await
            .map_err(|error| format!("cancel HTTP {status}: {error}"))?;

        // Then: cancellation stays pending and the remote job is untouched.
        assert!(deferred(&report, &task), "cancel HTTP {status}");
        let durable = durable_task(&fixture, &task).await?;
        assert_eq!(durable.status, TaskStatus::Processing);
        assert!(durable.cancel_requested_at.is_some());
        assert_eq!(worker.active_job_count().await, 1);
    }
    assert_auth_blocked(&durable_worker(&fixture, &task).await?);

    // When: the worker accepts the cancel again.
    reconciler.reconcile_task_id(task.id, Utc::now()).await?;

    // Then: cancellation converges.
    assert_eq!(
        durable_task(&fixture, &task).await?.status,
        TaskStatus::Cancelled
    );
    assert_eq!(worker.active_job_count().await, 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejected_cancel_after_confirming_submission_defers() -> TestResult {
    // Given: a cancelled task whose submission the worker accepted but whose
    // receipt was lost.
    let (fixture, worker, task, retry_at) =
        submitting_task_with_lost_receipt("cancel-submit").await?;
    request_cancellation(&fixture, &task).await?;
    fault(&worker, Route::JobCancel, 403, Vec::new()).await;
    let reconciler = fixture.reconciler()?;

    // When: reconciliation confirms the job and the worker rejects its cancel.
    let report = reconciler.reconcile_task_id(task.id, retry_at).await?;

    // Then: the task waits for the worker, still owning the confirmed job.
    assert!(deferred(&report, &task));
    let durable = durable_task(&fixture, &task).await?;
    assert_ne!(durable.status, TaskStatus::Cancelled);
    assert!(durable.cancel_requested_at.is_some());
    assert_eq!(worker.active_job_count().await, 1);
    assert_auth_blocked(&durable_worker(&fixture, &task).await?);

    // When: the worker accepts the cancel again.
    reconciler.reconcile_task_id(task.id, Utc::now()).await?;

    // Then: the confirmed job is cancelled and the task converges.
    assert_eq!(
        durable_task(&fixture, &task).await?.status,
        TaskStatus::Cancelled
    );
    assert_eq!(worker.active_job_count().await, 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn health_payload_drift_during_processing_does_not_stop_orchestration() -> TestResult {
    // Given: a processing task on a worker that briefly reports a health
    // value this Controller does not know.
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start().await?;
    let task = processing_task(&worker, &fixture, "health-drift").await?;
    for _ in 0..3 {
        fault(
            &worker,
            Route::Health,
            200,
            br#"{"status":"degraded"}"#.to_vec(),
        )
        .await;
    }

    // When/Then: orchestration keeps running and the task completes once
    // the worker reports ok again.
    assert_orchestration_keeps_running(&mut fixture).await;
    complete_mock_job(&worker, &task, b"drift-output").await?;
    assert_completed_pipeline(&fixture, &worker, &task, b"drift-output").await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn health_not_found_during_processing_does_not_stop_orchestration() -> TestResult {
    // Given: a processing task behind a proxy that briefly answers 404 for health.
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start().await?;
    let task = processing_task(&worker, &fixture, "health-404").await?;
    for _ in 0..3 {
        fault(&worker, Route::Health, 404, Vec::new()).await;
    }

    // When/Then: orchestration keeps running and the task completes.
    assert_orchestration_keeps_running(&mut fixture).await;
    complete_mock_job(&worker, &task, b"proxy-output").await?;
    assert_completed_pipeline(&fixture, &worker, &task, b"proxy-output").await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn poll_replies_about_the_worker_defer_instead_of_failing_the_task() -> TestResult {
    // Given: a processing task reconciled only by this test.
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start().await?;
    let task = processing_task(&worker, &fixture, "poll-scope").await?;
    fixture.crash().await?;
    let reconciler = fixture.reconciler()?;

    for status in [429, 302, 409, 400, 401] {
        fault(&worker, Route::JobPoll, status, Vec::new()).await;

        // When: the poll is answered for the worker, not for the job.
        let report = reconciler
            .reconcile_task_id(task.id, Utc::now())
            .await
            .map_err(|error| format!("poll HTTP {status}: {error}"))?;

        // Then: the task keeps its running remote job and waits.
        assert!(deferred(&report, &task), "poll HTTP {status}");
        assert_eq!(
            durable_task(&fixture, &task).await?.status,
            TaskStatus::Processing,
            "poll HTTP {status}"
        );
        assert_eq!(worker.active_job_count().await, 1);
    }
    assert_auth_blocked(&durable_worker(&fixture, &task).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unreadable_job_status_keeps_polling() -> TestResult {
    // Given: a processing task whose worker reports a job status this
    // Controller does not know.
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start().await?;
    let task = processing_task(&worker, &fixture, "poll-drift").await?;
    fixture.crash().await?;
    let mut job = remote_job(&worker, &fixture, &task).await?;
    job["status"] = serde_json::json!("paused");
    fault(&worker, Route::JobPoll, 200, serde_json::to_vec(&job)?).await;

    // When: reconciliation polls the job.
    let report = fixture
        .reconciler()?
        .reconcile_task_id(task.id, Utc::now())
        .await?;

    // Then: the job is treated as still running.
    assert_eq!(
        report.command_kind(task.id),
        Some(RecoveryCommandKind::Poll)
    );
    assert_eq!(
        durable_task(&fixture, &task).await?.status,
        TaskStatus::Processing
    );
    assert_eq!(worker.active_job_count().await, 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rotated_worker_password_during_processing_waits_for_the_new_password() -> TestResult {
    // Given: a processing task on a password-protected worker.
    let worker = MockVidenoa::start_persistent().await?;
    let first = uuid::Uuid::new_v4().to_string();
    worker.require_password(&first).await;
    let fixture = ControllerFixture::start().await?;
    let registered = fixture.register_protected_worker(&worker, &first).await?;
    let run = worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let task = fixture.create_task("rotate", b"input-video").await?;
    worker.await_checkpoint(&run).await?;
    worker.release(run).await?;
    wait_for_durable(&fixture, &task, |record| {
        record.status == TaskStatus::Processing
    })
    .await?;

    // When: the worker operator rotates the password.
    let rotated = uuid::Uuid::new_v4().to_string();
    worker.require_password(&rotated).await;

    // Then: the worker is blocked on its credentials while the task keeps
    // its running remote job, without retrying the old password.
    wait_for_worker_record(&fixture, registered.id, |record| {
        record.last_error.as_deref() == Some(AUTHENTICATION_FAILED)
    })
    .await?;
    tokio::time::sleep(Duration::from_millis(2_200)).await;
    let blocked = durable_task(&fixture, &task).await?;
    assert_eq!(blocked.status, TaskStatus::Processing);
    assert_eq!(worker.active_job_count().await, 1);
    let failures = worker.authentication_failures();
    assert!(failures <= 3, "{failures} authentication failures");
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    assert_eq!(worker.authentication_failures(), failures);

    // When: the Controller is given the new password.
    fixture
        .replace_worker_password(registered.id, &rotated)
        .await?;

    // Then: the same remote job completes the task.
    complete_mock_job(&worker, &task, b"rotated-output").await?;
    assert_completed_pipeline(&fixture, &worker, &task, b"rotated-output").await?;
    assert_eq!(worker.counters().await.get(Route::Run), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rate_limited_poll_during_processing_does_not_fail_the_task() -> TestResult {
    // Given: a processing task whose next poll is rate limited.
    let worker = MockVidenoa::start_persistent().await?;
    let fixture = ControllerFixture::start().await?;
    let task = processing_task(&worker, &fixture, "poll-429").await?;
    fault(&worker, Route::JobPoll, 429, Vec::new()).await;

    // When/Then: the task completes from the same remote job.
    complete_mock_job(&worker, &task, b"rate-limited-output").await?;
    assert_completed_pipeline(&fixture, &worker, &task, b"rate-limited-output").await?;
    assert_eq!(worker.counters().await.get(Route::Run), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejected_credentials_on_submission_defer_instead_of_failing() -> TestResult {
    // Given: a staged task reconciled only by this test.
    let (fixture, worker, task) = staged_task("submit-auth").await?;
    fault(&worker, Route::Run, 401, Vec::new()).await;
    let reconciler = fixture.reconciler()?;

    // When: the worker rejects the credentials on submission.
    let report = reconciler.reconcile_task_id(task.id, Utc::now()).await?;

    // Then: submission is retried later on the same attempt.
    assert!(deferred(&report, &task));
    let attempt = fixture
        .store
        .current_attempt(task.id)
        .await?
        .ok_or("attempt missing")?;
    assert_eq!(attempt.attempt.status, TaskStatus::Submitting);
    let retry_at = attempt
        .attempt
        .retry
        .next_retry_at
        .ok_or("retry deadline missing")?;
    assert_auth_blocked(&durable_worker(&fixture, &task).await?);

    // When: the retry is due and the worker accepts the credentials.
    reconciler.reconcile_task_id(task.id, retry_at).await?;

    // Then: the same attempt is submitted once.
    assert_eq!(
        durable_task(&fixture, &task).await?.status,
        TaskStatus::Processing
    );
    assert_eq!(worker.job_count().await, 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejected_credentials_while_cancelling_a_submission_keep_its_job() -> TestResult {
    // Given: a cancelled task whose submission the worker accepted but whose
    // receipt was lost.
    let (fixture, worker, task, retry_at) =
        submitting_task_with_lost_receipt("cancel-submit-auth").await?;
    request_cancellation(&fixture, &task).await?;
    fault(&worker, Route::JobLookup, 401, Vec::new()).await;
    let reconciler = fixture.reconciler()?;

    // When: confirming the submission is rejected for credentials.
    let report = reconciler.reconcile_task_id(task.id, retry_at).await?;

    // Then: the task does not decide the job was never accepted.
    assert!(deferred(&report, &task));
    assert_eq!(
        durable_task(&fixture, &task).await?.status,
        TaskStatus::Submitting
    );
    assert_eq!(worker.active_job_count().await, 1);
    assert_auth_blocked(&durable_worker(&fixture, &task).await?);

    // When: the retry is due and the worker accepts the credentials.
    let retry_at = fixture
        .store
        .current_attempt(task.id)
        .await?
        .and_then(|attempt| attempt.attempt.retry.next_retry_at)
        .ok_or("retry deadline missing")?;
    reconciler.reconcile_task_id(task.id, retry_at).await?;

    // Then: the confirmed job is cancelled with the task.
    assert_eq!(
        durable_task(&fixture, &task).await?.status,
        TaskStatus::Cancelled
    );
    assert_eq!(worker.active_job_count().await, 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remote_cleanup_retries_rejected_credentials_and_rate_limits() -> TestResult {
    // Given: a worker that rejects the first workspace deletes for credentials
    // and rate limiting.
    let worker = MockVidenoa::start_persistent().await?;
    worker
        .set_fault(Fault::DeleteScript(vec![
            DeleteOutcome::Unauthorized,
            DeleteOutcome::RateLimited,
            DeleteOutcome::Success,
        ]))
        .await;
    let fixture = ControllerFixture::start().await?;
    let task = processing_task(&worker, &fixture, "cleanup-scope").await?;

    // When: the task reaches remote cleanup.
    complete_mock_job(&worker, &task, b"cleanup-output").await?;

    // Then: cleanup retries until the worker accepts it.
    assert_completed_pipeline(&fixture, &worker, &task, b"cleanup-output").await?;
    assert_eq!(worker.counters().await.get(Route::DeleteFile), 3);
    Ok(())
}

/// Asserts that orchestration survives a window in which the faulty replies
/// are consumed.
pub(crate) async fn assert_orchestration_keeps_running(fixture: &mut ControllerFixture) {
    let stopped = tokio::time::timeout(
        Duration::from_secs(5),
        fixture.wait_for_orchestration_error(),
    )
    .await;
    assert!(stopped.is_err(), "orchestration stopped: {stopped:?}");
}

pub(crate) async fn processing_task(
    worker: &MockVidenoa,
    fixture: &ControllerFixture,
    name: &str,
) -> TestResult<Task> {
    fixture.register_worker(worker, name).await?;
    let run = worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let task = fixture.create_task(name, b"input-video").await?;
    worker.await_checkpoint(&run).await?;
    worker.release(run).await?;
    wait_for_durable(fixture, &task, |record| {
        record.status == TaskStatus::Processing
    })
    .await?;
    Ok(task)
}

/// A `Staged` task about to be submitted, reconciled only by the test.
pub(crate) async fn staged_task(name: &str) -> TestResult<(ControllerFixture, MockVidenoa, Task)> {
    let gate = CheckpointGate::new(TransferCheckpointPoint::BeforeRemoteSubmit);
    let observer: Arc<dyn TransferCheckpointObserver> = gate.clone();
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start_with_checkpoint_observer(Some(observer)).await?;
    fixture.register_worker(&worker, name).await?;
    let task = fixture.create_task(name, b"input-video").await?;
    gate.wait().await?;
    fixture.crash().await?;
    gate.release();
    Ok((fixture, worker, task))
}

/// The remote job of a processing task, as the worker reports it.
pub(crate) async fn remote_job(
    worker: &MockVidenoa,
    fixture: &ControllerFixture,
    task: &Task,
) -> TestResult<serde_json::Value> {
    let id = fixture
        .store
        .current_attempt(task.id)
        .await?
        .and_then(|attempt| attempt.attempt.remote_job_id)
        .ok_or("remote job id missing")?;
    let url = format!("{}/api/jobs/{id}", worker.base_url().trim_end_matches('/'));
    Ok(reqwest::get(url).await?.error_for_status()?.json().await?)
}

pub(crate) async fn wait_for_worker_record(
    fixture: &ControllerFixture,
    id: WorkerId,
    predicate: impl Fn(&WorkerRecord) -> bool,
) -> TestResult<WorkerRecord> {
    tokio::time::timeout(
        crate::mock_videnoa::deadline::eventually(Duration::from_secs(5)),
        async {
            loop {
                let record = fixture.store.worker(id).await?.ok_or("worker missing")?;
                if predicate(&record) {
                    return Ok(record);
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        },
    )
    .await
    .map_err(|_| std::io::Error::other("worker did not reach the expected durable state"))?
}

/// A `Submitting` task, reconciled only by the test, whose first submission
/// the worker accepted but whose response was lost. Returns when its
/// confirmation retry is due.
pub(crate) async fn submitting_task_with_lost_receipt(
    name: &str,
) -> TestResult<(ControllerFixture, MockVidenoa, Task, DateTime<Utc>)> {
    let (fixture, worker, task) = staged_task(name).await?;
    worker.set_fault(Fault::AcceptThenDropRunResponse).await;
    assert!(fixture
        .reconciler()?
        .reconcile_task_id(task.id, Utc::now())
        .await
        .is_err());
    assert_eq!(worker.active_job_count().await, 1);
    let attempt = fixture
        .store
        .current_attempt(task.id)
        .await?
        .ok_or("attempt missing")?;
    assert_eq!(attempt.attempt.status, TaskStatus::Submitting);
    let retry_at = attempt
        .attempt
        .retry
        .next_retry_at
        .ok_or("retry deadline missing")?;
    Ok((fixture, worker, task, retry_at))
}

pub(crate) async fn fault(worker: &MockVidenoa, route: Route, status: u16, body: Vec<u8>) {
    worker
        .set_fault(Fault::Response(ResponseFault {
            route,
            status,
            body,
        }))
        .await;
}

pub(crate) fn deferred(report: &RecoveryReport, task: &Task) -> bool {
    report
        .deferred()
        .iter()
        .any(|entry| entry.task_id == task.id)
}

pub(crate) fn assert_auth_blocked(record: &WorkerRecord) {
    assert!(!record.online);
    assert_eq!(record.last_error.as_deref(), Some(AUTHENTICATION_FAILED));
    // No deadline: tasks wait until the health service sees the worker again.
    assert_eq!(record.next_health_check_at, None);
    assert!(record.health_retry_count > 0);
}

pub(crate) async fn request_cancellation(fixture: &ControllerFixture, task: &Task) -> TestResult {
    let durable = durable_task(fixture, task).await?;
    let attempt = fixture.store.current_attempt(task.id).await?;
    LifecycleService::new(fixture.store.clone())
        .request_cancellation(&durable, attempt.as_ref(), Utc::now())
        .await?;
    Ok(())
}

pub(crate) async fn durable_task(
    fixture: &ControllerFixture,
    task: &Task,
) -> TestResult<TaskRecord> {
    Ok(fixture.store.task(task.id).await?.ok_or("task missing")?)
}

pub(crate) async fn durable_worker(
    fixture: &ControllerFixture,
    task: &Task,
) -> TestResult<WorkerRecord> {
    let id: WorkerId = durable_task(fixture, task)
        .await?
        .worker_id
        .ok_or("task has no worker")?;
    Ok(fixture.store.worker(id).await?.ok_or("worker missing")?)
}

pub(crate) async fn wait_for_durable(
    fixture: &ControllerFixture,
    task: &Task,
    predicate: impl Fn(&TaskRecord) -> bool,
) -> TestResult<TaskRecord> {
    tokio::time::timeout(
        crate::mock_videnoa::deadline::eventually(Duration::from_secs(15)),
        async {
            loop {
                let record = durable_task(fixture, task).await?;
                if predicate(&record) {
                    return Ok(record);
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        },
    )
    .await
    .map_err(|_| std::io::Error::other("task did not reach the expected durable state"))?
}
