use videnoa_controller::domain::TaskStatus;
use videnoa_controller::lifecycle::LifecycleService;
use videnoa_controller::scheduler::{PublicationOutcome, TransferCheckpointPoint};

use crate::checkpoints::CheckpointGate;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{output_path, publish, verified_task};
use crate::transfer_support::{verified_path, zero_jitter, TestResult};

#[tokio::test]
async fn verifying_cancellation_prevents_publication_effects() -> TestResult {
    // Given: cancellation intent is durably accepted while a task is still Verifying.
    let server = MockVidenoa::start().await?;
    let output = b"cancel before publication".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    LifecycleService::new(fixture.store.clone())
        .request_cancellation(&task, Some(&attempt), fixture.now)
        .await?;

    // When: the production publication executor observes the cancelled snapshot.
    let outcome = publish(&fixture, &prepared).await;

    // Then: ordinary publication is blocked before destination effects.
    assert!(outcome.is_err());
    assert!(!output_path(&fixture, &prepared).await?.exists());
    assert!(verified_path(&fixture.temp_root, prepared.task_id).exists());
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::Verifying
    );
    Ok(())
}

#[tokio::test]
async fn late_cancellation_cannot_interrupt_publishing() -> TestResult {
    // Given: production publication has crossed into Publishing and verified staging.
    let server = MockVidenoa::start().await?;
    let output = b"late publishing cancellation".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let gate = CheckpointGate::new(TransferCheckpointPoint::StagingVerified);
    let executor = fixture.executor()?.with_checkpoint_observer(gate.clone());
    let task_id = prepared.task_id;
    let now = fixture.now;
    let publication = tokio::spawn(async move {
        let outcome = executor.publish(task_id, now, zero_jitter()?).await?;
        TestResult::Ok(outcome)
    });
    gate.wait().await?;
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;

    // When: cancellation is requested after publication became irreversible.
    let cancellation = LifecycleService::new(fixture.store.clone())
        .request_cancellation(&task, Some(&attempt), fixture.now)
        .await;
    gate.release();
    let outcome = publication.await??;

    // Then: cancellation conflicts and publication-cleanup still converges.
    assert!(cancellation.is_err());
    assert_eq!(outcome, PublicationOutcome::Completed);
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::Completed
    );
    Ok(())
}

#[tokio::test]
async fn late_cancellation_cannot_interrupt_remote_cleanup() -> TestResult {
    // Given: publication has committed and production cleanup is paused before remote DELETE.
    let server = MockVidenoa::start().await?;
    let output = b"late cleanup cancellation".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let gate = CheckpointGate::new(TransferCheckpointPoint::BeforeRemoteDelete);
    let executor = fixture.executor()?.with_checkpoint_observer(gate.clone());
    let task_id = prepared.task_id;
    let now = fixture.now;
    let publication = tokio::spawn(async move {
        let outcome = executor.publish(task_id, now, zero_jitter()?).await?;
        TestResult::Ok(outcome)
    });
    gate.wait().await?;
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    assert_eq!(task.status, TaskStatus::RemoteCleanup);

    // When: cancellation is requested after cleanup became mandatory.
    let cancellation = LifecycleService::new(fixture.store.clone())
        .request_cancellation(&task, Some(&attempt), fixture.now)
        .await;
    gate.release();
    let outcome = publication.await??;

    // Then: cancellation conflicts and mandatory cleanup completes.
    assert!(cancellation.is_err());
    assert_eq!(outcome, PublicationOutcome::Completed);
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::Completed
    );
    Ok(())
}
