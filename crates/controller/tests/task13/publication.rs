use videnoa_controller::domain::{FailureCode, TaskStatus};
use videnoa_controller::lifecycle::{AdvanceCommand, LifecycleService, PublicationIntent};
use videnoa_controller::scheduler::PublicationOutcome;

use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{assert_status, output_path, publish, verified_task};
use crate::transfer_support::{verified_path, TestResult};

#[tokio::test]
async fn publication_never_replaces_an_existing_destination() -> TestResult {
    // Given: a verified task whose destination was occupied before publication admission.
    let server = MockVidenoa::start().await?;
    let output = b"verified publication".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let destination = output_path(&fixture, &prepared).await?;
    tokio::fs::write(&destination, b"unrelated existing bytes").await?;

    // When: publication rechecks the output capability.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: the destination and verified source are preserved and the failure is terminal.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert_eq!(
        tokio::fs::read(&destination).await?,
        b"unrelated existing bytes"
    );
    assert!(verified_path(&fixture.temp_root, prepared.task_id).exists());
    let task = fixture.task(prepared.task_id).await?;
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(
        task.failure.as_ref().map(|failure| failure.failure_code),
        Some(FailureCode::OutputExists)
    );
    Ok(())
}

#[tokio::test]
async fn matching_final_after_crash_converges_without_verified_source() -> TestResult {
    // Given: durable publishing intent and a matching final file left after finalization.
    let server = MockVidenoa::start().await?;
    let output = b"crash recovered publication".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    LifecycleService::new(fixture.store.clone())
        .advance(
            &task,
            &attempt,
            AdvanceCommand::FinishVerification(PublicationIntent::new(
                ".videnoa-crash-recovery.staging",
            )),
            fixture.now,
        )
        .await?;
    let destination = output_path(&fixture, &prepared).await?;
    tokio::fs::write(&destination, &output).await?;
    tokio::fs::remove_dir_all(fixture.temp_root.join(prepared.task_id.to_string())).await?;

    // When: startup-style publication reconciliation runs.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: exact final evidence proves publication and cleanup completes.
    assert_eq!(outcome, PublicationOutcome::Completed);
    assert_eq!(tokio::fs::read(&destination).await?, output);
    assert_status(&fixture, &prepared, TaskStatus::Completed).await?;
    Ok(())
}

#[tokio::test]
async fn corrupt_owned_staging_is_ambiguous_and_preserved() -> TestResult {
    // Given: durable publishing intent whose hidden destination staging bytes do not match.
    let server = MockVidenoa::start().await?;
    let output = b"expected publication".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    let staging_name = ".videnoa-corrupt.staging";
    LifecycleService::new(fixture.store.clone())
        .advance(
            &task,
            &attempt,
            AdvanceCommand::FinishVerification(PublicationIntent::new(staging_name)),
            fixture.now,
        )
        .await?;
    let destination = output_path(&fixture, &prepared).await?;
    let staging = destination
        .parent()
        .ok_or_else(|| std::io::Error::other("destination parent missing"))?
        .join(staging_name);
    tokio::fs::write(&staging, b"corrupt staging bytes").await?;

    // When: publication reconciliation inspects the owned staging file.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: it preserves all evidence and closes with publication ambiguity.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert_eq!(tokio::fs::read(&staging).await?, b"corrupt staging bytes");
    assert!(!destination.exists());
    assert!(verified_path(&fixture.temp_root, prepared.task_id).exists());
    let task = fixture.task(prepared.task_id).await?;
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(
        task.failure.as_ref().map(|failure| failure.failure_code),
        Some(FailureCode::PublicationAmbiguous)
    );
    Ok(())
}

#[tokio::test]
async fn valid_hidden_staging_resumes_without_verified_source() -> TestResult {
    // Given: a crash left durable publishing intent and a complete hidden staging file.
    let server = MockVidenoa::start().await?;
    let output = b"resumable staging publication".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    let staging_name = ".videnoa-resume.staging";
    LifecycleService::new(fixture.store.clone())
        .advance(
            &task,
            &attempt,
            AdvanceCommand::FinishVerification(PublicationIntent::new(staging_name)),
            fixture.now,
        )
        .await?;
    let destination = output_path(&fixture, &prepared).await?;
    let staging = destination
        .parent()
        .ok_or_else(|| std::io::Error::other("destination parent missing"))?
        .join(staging_name);
    tokio::fs::write(&staging, &output).await?;
    tokio::fs::remove_dir_all(fixture.temp_root.join(prepared.task_id.to_string())).await?;

    // When: publication reconciliation resumes from the hidden staging file.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: no source recopy is needed and the exact final output completes cleanup.
    assert_eq!(outcome, PublicationOutcome::Completed);
    assert_eq!(tokio::fs::read(&destination).await?, output);
    assert!(!staging.exists());
    assert_status(&fixture, &prepared, TaskStatus::Completed).await?;
    Ok(())
}

#[tokio::test]
async fn racing_destination_preserves_final_and_owned_staging() -> TestResult {
    // Given: valid owned staging exists when an unrelated destination wins the finalization race.
    let server = MockVidenoa::start().await?;
    let output = b"race-safe publication".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    let staging_name = ".videnoa-race.staging";
    LifecycleService::new(fixture.store.clone())
        .advance(
            &task,
            &attempt,
            AdvanceCommand::FinishVerification(PublicationIntent::new(staging_name)),
            fixture.now,
        )
        .await?;
    let destination = output_path(&fixture, &prepared).await?;
    let staging = destination
        .parent()
        .ok_or_else(|| std::io::Error::other("destination parent missing"))?
        .join(staging_name);
    tokio::fs::write(&staging, &output).await?;
    tokio::fs::write(&destination, b"racing unrelated output").await?;

    // When: publication reconciliation encounters the raced destination.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: neither file is overwritten or deleted and ownership is marked ambiguous.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert_eq!(
        tokio::fs::read(&destination).await?,
        b"racing unrelated output"
    );
    assert_eq!(tokio::fs::read(&staging).await?, output);
    let task = fixture.task(prepared.task_id).await?;
    assert_eq!(
        task.failure.as_ref().map(|failure| failure.failure_code),
        Some(FailureCode::PublicationAmbiguous)
    );
    Ok(())
}
