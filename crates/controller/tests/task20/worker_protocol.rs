//! The Controller and the Worker agree on a protocol version and on what the
//! Worker's replies mean: typed error codes, the cancel fields of a job, and
//! the lookup of a submission by its idempotency key.
use std::time::Duration;

use chrono::Utc;
use videnoa_controller::domain::{FailureCode, FailureStage, Task, TaskStatus};
use videnoa_controller::remote::PROTOCOL_VERSION;

use crate::mock_videnoa::domain::CancelReason;
use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;
use crate::remote_scope::{
    deferred, durable_task, durable_worker, fault, processing_task, request_cancellation,
    submitting_task_with_lost_receipt, wait_for_durable, wait_for_worker_record,
};
use crate::support::{ControllerFixture, TestResult};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_worker_speaking_a_newer_protocol_is_offline_and_says_why() -> TestResult {
    // Given: a worker whose health reports a protocol this Controller does not speak.
    let worker = MockVidenoa::start().await?;
    let newer = PROTOCOL_VERSION + 1;
    worker.report_protocol(Some(newer)).await;
    let fixture = ControllerFixture::start().await?;
    let registered = fixture
        .register_worker_without_wait(&worker, "newer-protocol", true)
        .await?;
    let task = fixture
        .create_task("newer-protocol", b"input-video")
        .await?;

    // When: the health service probes it.
    let failed = wait_for_worker_record(&fixture, registered.id, |record| {
        record.last_error.is_some()
    })
    .await?;

    // Then: it is offline with a message naming both versions, and nothing is scheduled there.
    assert!(!failed.online);
    let message = failed.last_error.unwrap_or_default();
    assert!(
        message.contains(&format!("protocol {newer}"))
            && message.contains(&format!("1 to {PROTOCOL_VERSION}")),
        "{message}"
    );
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    assert_eq!(fixture.task(&task).await?.task.status, TaskStatus::Queued);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_worker_that_reports_no_protocol_is_used_as_protocol_1() -> TestResult {
    // Given: a v0.1.10 worker, whose health has no protocol field.
    let worker = MockVidenoa::start().await?;
    worker.report_protocol(None).await;
    let fixture = ControllerFixture::start().await?;
    let registered = fixture.register_worker(&worker, "legacy-protocol").await?;

    // When/Then: it comes online and takes work.
    wait_for_worker_record(&fixture, registered.id, |record| record.online).await?;
    let task = fixture
        .create_task("legacy-protocol", b"input-video")
        .await?;
    crate::remote_scope::wait_for_durable(&fixture, &task, |record| {
        record.status != TaskStatus::Queued
    })
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_processing_task_waits_while_its_worker_speaks_a_newer_protocol() -> TestResult {
    // Given: a processing task reconciled only by this test.
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start().await?;
    let task = processing_task(&worker, &fixture, "protocol-processing").await?;
    fixture.crash().await?;
    worker.report_protocol(Some(PROTOCOL_VERSION + 1)).await;

    // When: reconciliation checks the worker.
    let report = fixture
        .reconciler()?
        .reconcile_task_id(task.id, Utc::now())
        .await?;

    // Then: the task waits with its remote job and the worker is backed off.
    assert!(deferred(&report, &task));
    assert_eq!(
        durable_task(&fixture, &task).await?.status,
        TaskStatus::Processing
    );
    assert!(!durable_worker(&fixture, &task).await?.online);
    assert_eq!(worker.active_job_count().await, 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_job_cancelled_on_the_worker_says_who_cancelled_it() -> TestResult {
    for (reason, message) in [
        (CancelReason::User, "remote job was cancelled on the worker"),
        (
            CancelReason::Shutdown,
            "remote job was cancelled when the worker shut down",
        ),
        (
            CancelReason::Restart,
            "remote job was cancelled during worker restart",
        ),
    ] {
        // Given: a processing task whose job is cancelled on the worker.
        let worker = MockVidenoa::start_persistent().await?;
        let mut fixture = ControllerFixture::start().await?;
        let task = processing_task(&worker, &fixture, "worker-cancelled").await?;
        fixture.crash().await?;
        let job_id = remote_job_id(&fixture, &task).await?;
        worker.request_cancel(&job_id, reason, true).await?;

        // When: reconciliation polls it.
        fixture
            .reconciler()?
            .reconcile_task_id(task.id, Utc::now())
            .await?;

        // Then: the failure names the cause the worker reported.
        let failure = fixture
            .store
            .current_attempt(task.id)
            .await?
            .and_then(|attempt| attempt.attempt.failure)
            .ok_or("attempt has no failure")?;
        assert_eq!(failure.message, message, "{reason:?}");
        assert_eq!(
            durable_task(&fixture, &task).await?.status,
            TaskStatus::Failed
        );
    }
    Ok(())
}

async fn remote_job_id(fixture: &ControllerFixture, task: &Task) -> TestResult<String> {
    Ok(fixture
        .store
        .current_attempt(task.id)
        .await?
        .and_then(|attempt| attempt.attempt.remote_job_id)
        .ok_or("remote job id missing")?
        .to_string())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_a_lost_submission_looks_it_up_instead_of_resubmitting() -> TestResult {
    // Given: a cancelled Submitting task whose accepted run lost its receipt.
    let (fixture, worker, task, retry_at) =
        submitting_task_with_lost_receipt("cancel-lookup").await?;
    request_cancellation(&fixture, &task).await?;

    // When: reconciliation learns whether the worker accepted it.
    fixture
        .reconciler()?
        .reconcile_task_id(task.id, retry_at)
        .await?;

    // Then: it asked by key, sent no second run, and cancelled the job it found.
    let counters = worker.counters().await;
    assert_eq!(counters.get(Route::JobLookup), 1);
    assert_eq!(counters.get(Route::Run), 1);
    assert_eq!(
        durable_task(&fixture, &task).await?.status,
        TaskStatus::Cancelled
    );
    assert_eq!(worker.active_job_count().await, 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_a_submission_the_worker_never_accepted_creates_no_job() -> TestResult {
    // Given: a cancelled Submitting task whose run the worker never received.
    let (fixture, worker, task, retry_at) =
        submitting_task_with_lost_receipt("cancel-lookup-none").await?;
    request_cancellation(&fixture, &task).await?;
    // The lost run's job is gone, and with it the key (as after a worker
    // deleted the job's history).
    assert_eq!(worker.job_count().await, 1);
    worker.forget_jobs().await;

    // When: reconciliation looks the submission up.
    fixture
        .reconciler()?
        .reconcile_task_id(task.id, retry_at)
        .await?;

    // Then: the task is cancelled without a job being created to cancel.
    assert_eq!(
        durable_task(&fixture, &task).await?.status,
        TaskStatus::Cancelled
    );
    assert_eq!(worker.counters().await.get(Route::Run), 1);
    assert_eq!(worker.job_count().await, 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_a_lost_submission_on_a_protocol_1_worker_resubmits() -> TestResult {
    // Given: a worker without the lookup route.
    let (fixture, worker, task, retry_at) =
        submitting_task_with_lost_receipt("cancel-lookup-v1").await?;
    worker.report_protocol(None).await;
    request_cancellation(&fixture, &task).await?;

    // When: reconciliation learns whether the worker accepted it.
    fixture
        .reconciler()?
        .reconcile_task_id(task.id, retry_at)
        .await?;

    // Then: it falls back to resubmitting with the same key.
    let counters = worker.counters().await;
    assert_eq!(counters.get(Route::JobLookup), 0);
    assert_eq!(counters.get(Route::Run), 2);
    assert_eq!(
        durable_task(&fixture, &task).await?.status,
        TaskStatus::Cancelled
    );
    assert_eq!(worker.active_job_count().await, 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_worker_500_that_retrying_cannot_fix_fails_the_submission() -> TestResult {
    // Given: a worker that cannot read the saved workflow, and says so with a code.
    let worker = MockVidenoa::start_persistent().await?;
    let fixture = ControllerFixture::start().await?;
    let reason = "failed to read workflow: Permission denied (os error 13)";
    fault(
        &worker,
        Route::Run,
        500,
        format!(r#"{{"error":"{reason}","code":"workflow_unreadable"}}"#).into_bytes(),
    )
    .await;
    fixture
        .register_worker(&worker, "unreadable-workflow")
        .await?;

    // When: the task is submitted.
    let task = fixture
        .create_task("unreadable-workflow", b"input-video")
        .await?;
    wait_for_durable(&fixture, &task, |record| {
        record.status == TaskStatus::Failed
    })
    .await?;

    // Then: it fails once, terminally, with the worker's reason, and no job exists.
    let detail = fixture.task(&task).await?;
    let failure = detail.task.failure.ok_or("failed task has no failure")?;
    assert_eq!(failure.failure_stage, FailureStage::Submission);
    assert_eq!(failure.failure_code, FailureCode::RemoteSubmissionFailed);
    assert!(!failure.retryable);
    assert!(failure.message.ends_with(reason), "{}", failure.message);
    assert_eq!(worker.counters().await.get(Route::Run), 1);
    assert_eq!(worker.job_count().await, 0);
    Ok(())
}
