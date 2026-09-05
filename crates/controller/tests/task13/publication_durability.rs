use videnoa_controller::domain::TaskStatus;
use videnoa_controller::lifecycle::{AdvanceCommand, LifecycleService, PublicationIntent};
use videnoa_controller::scheduler::{PublicationOutcome, TransferCheckpointPoint};

use crate::checkpoints::CheckpointGate;
use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{output_path, publish, verified_task};
use crate::transfer_support::{verified_path, zero_jitter, TestResult};

#[tokio::test]
async fn recovered_final_retries_parent_sync_before_finishing_publication() -> TestResult {
    // Given: rename completed before a crash and recovery pauses before re-synchronizing parents.
    let server = MockVidenoa::start().await?;
    let output = b"recovered final durability".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    LifecycleService::new(fixture.store.clone())
        .advance(
            &task,
            &attempt,
            AdvanceCommand::FinishVerification(PublicationIntent::direct()),
            fixture.now,
        )
        .await?;
    let destination = output_path(&fixture, &prepared).await?;
    tokio::fs::rename(
        verified_path(&fixture.temp_root, prepared.task_id),
        &destination,
    )
    .await?;
    let gate = CheckpointGate::new(TransferCheckpointPoint::PublicationFinalized);
    let executor = fixture.executor()?.with_checkpoint_observer(gate.clone());
    let task_id = prepared.task_id;
    let now = fixture.now;
    let publication = tokio::spawn(async move {
        let outcome = executor.publish(task_id, now, zero_jitter()?).await?;
        TestResult::Ok(outcome)
    });
    gate.wait().await?;
    let displaced_root = fixture.output_root.with_extension("sync-failure");
    tokio::fs::rename(&fixture.output_root, &displaced_root).await?;
    tokio::fs::create_dir(&fixture.output_root).await?;

    // When: the first parent sync fails, then the original root is restored and publication retries.
    gate.release();
    let first = publication.await?;
    tokio::fs::remove_dir(&fixture.output_root).await?;
    tokio::fs::rename(&displaced_root, &fixture.output_root).await?;
    assert!(first.is_err());
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::Publishing
    );
    let run_requests = server.counters().await.get(Route::Run);
    let retry = publish(&fixture, &prepared).await?;

    // Then: retry re-synchronizes the matching final, completes cleanup, and never replays compute.
    assert_eq!(retry, PublicationOutcome::Completed);
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::Completed
    );
    assert_eq!(server.counters().await.get(Route::Run), run_requests);
    assert_eq!(tokio::fs::read(&destination).await?, output);
    Ok(())
}
