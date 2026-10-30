//! The Controller and the Worker agree on a protocol version and on what the
//! Worker's replies mean: typed error codes, the cancel fields of a job, and
//! the lookup of a submission by its idempotency key.
use std::time::Duration;

use chrono::Utc;
use videnoa_controller::domain::{FailureCode, FailureStage, TaskStatus};
use videnoa_controller::remote::PROTOCOL_VERSION;

use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;
use crate::remote_scope::{
    deferred, durable_task, durable_worker, fault, processing_task, wait_for_durable,
    wait_for_worker_record,
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
