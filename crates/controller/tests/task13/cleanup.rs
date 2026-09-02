use chrono::Duration;
use videnoa_controller::domain::{FailureCode, FailureStage, TaskStatus};
use videnoa_controller::scheduler::{PublicationOutcome, TransferCheckpointPoint};

use crate::checkpoints::CheckpointGate;
use crate::mock_videnoa::faults::{DeleteOutcome, Fault, OfflineMode};
use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{output_path, publish, verified_task};
use crate::transfer_support::{zero_jitter, TestResult};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

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

#[cfg(unix)]
#[tokio::test]
async fn local_cleanup_failure_retries_before_remote_delete() -> TestResult {
    // Given: publication pauses immediately before local temp removal.
    let server = MockVidenoa::start().await?;
    let output = b"local cleanup failure".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let gate = CheckpointGate::new(TransferCheckpointPoint::BeforeLocalCleanup);
    let executor = fixture.executor()?.with_checkpoint_observer(gate.clone());
    let task_id = prepared.task_id;
    let now = fixture.now;
    let publication = tokio::spawn(async move {
        let outcome = executor.publish(task_id, now, zero_jitter()?).await?;
        TestResult::Ok(outcome)
    });
    gate.wait().await?;
    std::fs::set_permissions(&fixture.temp_root, std::fs::Permissions::from_mode(0o500))?;

    // When: cleanup resumes without permission to unlink the task directory.
    gate.release();
    let outcome = publication.await?;
    std::fs::set_permissions(&fixture.temp_root, std::fs::Permissions::from_mode(0o700))?;
    let outcome = outcome?;

    // Then: cleanup is durably retried before any remote DELETE or compute replay.
    assert!(matches!(
        outcome,
        PublicationOutcome::RetryScheduled { retry_count: 1, .. }
    ));
    assert!(fixture
        .temp_root
        .join(prepared.task_id.to_string())
        .exists());
    assert_eq!(server.counters().await.get(Route::DeleteFile), 0);
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::RemoteCleanup
    );
    assert!(output_path(&fixture, &prepared).await?.exists());
    Ok(())
}

#[tokio::test]
async fn remote_delete_network_failure_exhausts_without_replaying_work() -> TestResult {
    // Given: publication is ready but the worker endpoint becomes unreachable.
    let mut server = MockVidenoa::start().await?;
    let output = b"network cleanup exhaustion".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let before = server.counters().await;
    server.set_offline(OfflineMode::ConnectionRefused).await?;

    // When: remote cleanup consumes every persisted retry opportunity.
    let first = publish(&fixture, &prepared).await?;
    let second = fixture
        .executor()?
        .cleanup(
            prepared.task_id,
            fixture.now + Duration::seconds(2),
            zero_jitter()?,
        )
        .await?;
    let third = fixture
        .executor()?
        .cleanup(
            prepared.task_id,
            fixture.now + Duration::seconds(10),
            zero_jitter()?,
        )
        .await?;
    let fourth = fixture
        .executor()?
        .cleanup(
            prepared.task_id,
            fixture.now + Duration::seconds(20),
            zero_jitter()?,
        )
        .await?;

    // Then: retries exhaust terminally without rerunning upload, run, or download.
    assert!(matches!(first, PublicationOutcome::RetryScheduled { .. }));
    assert!(matches!(second, PublicationOutcome::RetryScheduled { .. }));
    assert!(matches!(third, PublicationOutcome::RetryScheduled { .. }));
    assert_eq!(fourth, PublicationOutcome::Failed);
    let task = fixture.task(prepared.task_id).await?;
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(
        task.failure.as_ref().map(|failure| failure.failure_code),
        Some(FailureCode::CleanupFailed)
    );
    let after = server.counters().await;
    for route in [Route::Upload, Route::Run, Route::Download] {
        assert_eq!(after.get(route), before.get(route));
    }
    assert!(output_path(&fixture, &prepared).await?.exists());
    Ok(())
}

#[tokio::test]
async fn crash_after_remote_delete_converges_through_not_found() -> TestResult {
    // Given: remote DELETE succeeds before the completion lifecycle CAS.
    let server = MockVidenoa::start().await?;
    let output = b"delete before completion crash".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let gate = CheckpointGate::new(TransferCheckpointPoint::RemoteDeleteSucceeded);
    let executor = fixture.executor()?.with_checkpoint_observer(gate.clone());
    let task_id = prepared.task_id;
    let now = fixture.now;
    let publication = tokio::spawn(async move {
        let outcome = executor.publish(task_id, now, zero_jitter()?).await?;
        TestResult::Ok(outcome)
    });
    gate.wait().await?;
    assert_eq!(server.counters().await.get(Route::DeleteFile), 1);
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::RemoteCleanup
    );
    publication.abort();
    let _ = publication.await;

    // When: a fresh cleanup execution retries the already-applied DELETE.
    let outcome = fixture
        .executor()?
        .cleanup(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;

    // Then: remote 404 is idempotent success and completion converges.
    assert_eq!(outcome, PublicationOutcome::Completed);
    assert_eq!(server.counters().await.get(Route::DeleteFile), 2);
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::Completed
    );
    Ok(())
}
