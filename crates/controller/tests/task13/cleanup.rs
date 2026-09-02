use chrono::Duration;
use videnoa_controller::domain::{FailureCode, FailureStage, TaskStatus};
use videnoa_controller::scheduler::PublicationOutcome;

use crate::mock_videnoa::faults::{DeleteOutcome, Fault};
use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{output_path, publish, verified_task};
use crate::transfer_support::{zero_jitter, TestResult};

#[tokio::test]
async fn remote_delete_not_found_is_cleanup_success() -> TestResult {
    // Given: a verified task whose remote workspace DELETE reports already absent.
    let server = MockVidenoa::start().await?;
    let output = b"idempotent cleanup".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    server
        .set_fault(Fault::DeleteScript(vec![DeleteOutcome::NotFound]))
        .await;

    // When: publication and cleanup converge.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: the task completes after one DELETE and local temp removal.
    assert_eq!(outcome, PublicationOutcome::Completed);
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::Completed
    );
    assert!(!fixture
        .temp_root
        .join(prepared.task_id.to_string())
        .exists());
    assert_eq!(server.counters().await.get(Route::DeleteFile), 1);
    assert!(output_path(&fixture, &prepared).await?.exists());
    Ok(())
}

#[tokio::test]
async fn remote_delete_server_error_retries_after_local_cleanup() -> TestResult {
    // Given: publication succeeds but the first remote workspace DELETE returns 500.
    let server = MockVidenoa::start().await?;
    let output = b"retry cleanup".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    server
        .set_fault(Fault::DeleteScript(vec![
            DeleteOutcome::ServerError,
            DeleteOutcome::Success,
        ]))
        .await;

    // When: cleanup runs once and then retries after the durable deadline.
    let first = publish(&fixture, &prepared).await?;
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    let second = fixture
        .executor()?
        .cleanup(
            prepared.task_id,
            fixture.now + Duration::seconds(2),
            zero_jitter()?,
        )
        .await?;

    // Then: local bytes stay gone, retry metadata was paired, and completion follows DELETE.
    assert!(matches!(
        first,
        PublicationOutcome::RetryScheduled { retry_count: 1, .. }
    ));
    assert_eq!(task.status, TaskStatus::RemoteCleanup);
    assert_eq!(task.retry, attempt.attempt.retry);
    assert!(!fixture
        .temp_root
        .join(prepared.task_id.to_string())
        .exists());
    assert_eq!(second, PublicationOutcome::Completed);
    assert_eq!(server.counters().await.get(Route::DeleteFile), 2);
    Ok(())
}

#[tokio::test]
async fn remote_delete_client_error_is_terminal_configuration_failure() -> TestResult {
    // Given: publication succeeds but remote workspace DELETE returns a non-retryable 400.
    let server = MockVidenoa::start().await?;
    let output = b"terminal cleanup".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    server
        .set_fault(Fault::DeleteScript(vec![DeleteOutcome::ClientError]))
        .await;

    // When: cleanup classifies the response.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: task history closes terminally without deleting the published output.
    assert_eq!(outcome, PublicationOutcome::Failed);
    let task = fixture.task(prepared.task_id).await?;
    let failure = task
        .failure
        .ok_or_else(|| std::io::Error::other("cleanup failure missing"))?;
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(failure.failure_stage, FailureStage::RemoteCleanup);
    assert_eq!(failure.failure_code, FailureCode::CleanupFailed);
    assert!(!failure.retryable);
    assert!(output_path(&fixture, &prepared).await?.exists());
    Ok(())
}
