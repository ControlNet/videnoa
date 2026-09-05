use std::path::PathBuf;

use videnoa_controller::domain::{FailureCode, TaskStatus};
use videnoa_controller::lifecycle::{AdvanceCommand, LifecycleService, PublicationIntent};
use videnoa_controller::scheduler::PublicationOutcome;

use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{output_path, publish, verified_task};
use crate::transfer_support::{verified_path, Fixture, PreparedTask, TestResult};

#[tokio::test]
async fn persisted_legacy_staging_evidence_is_preserved_as_ambiguous() -> TestResult {
    // Given: an older Controller persisted a destination staging name and file.
    let server = MockVidenoa::start().await?;
    let output = b"legacy publication bytes".repeat(1024);
    let staging_name = ".videnoa-legacy.staging";
    let (fixture, prepared, destination) =
        publishing_task(&server, &output, PublicationIntent::new(staging_name)).await?;
    let staging = destination
        .parent()
        .ok_or_else(|| std::io::Error::other("destination parent missing"))?
        .join(staging_name);
    tokio::fs::write(&staging, &output).await?;

    // When: the direct-publication implementation encounters the old evidence shape.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: it preserves both old staging and verified temp evidence without guessing ownership.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert_eq!(tokio::fs::read(&staging).await?, output);
    assert!(verified_path(&fixture.temp_root, prepared.task_id).exists());
    assert!(!destination.exists());
    assert_ambiguous(&fixture, &prepared).await?;
    Ok(())
}

#[tokio::test]
async fn missing_legacy_staging_leaf_recovers_from_direct_temp_evidence() -> TestResult {
    // Given: an older row names a staging leaf that is absent while verified temp bytes remain.
    let server = MockVidenoa::start().await?;
    let output = b"legacy row direct recovery".repeat(1024);
    let (fixture, prepared, _) = publishing_task(
        &server,
        &output,
        PublicationIntent::new(".videnoa-missing-legacy.staging"),
    )
    .await?;

    // When: publication verifies the legacy sibling is missing and uses direct recovery.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: exact final publication and mandatory cleanup complete normally.
    assert_eq!(outcome, PublicationOutcome::Completed);
    assert_eq!(
        tokio::fs::read(output_path(&fixture, &prepared).await?).await?,
        output
    );
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::Completed
    );
    Ok(())
}

#[tokio::test]
async fn matching_final_and_verified_temp_are_conservatively_ambiguous() -> TestResult {
    // Given: direct durable intent exists while matching bytes occupy both owned temp and final.
    let server = MockVidenoa::start().await?;
    let output = b"contradictory direct publication".repeat(1024);
    let (fixture, prepared, destination) =
        publishing_task(&server, &output, PublicationIntent::direct()).await?;
    tokio::fs::write(&destination, &output).await?;

    // When: recovery observes both possible ownership witnesses.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: it preserves both artifacts and terminates as publication ambiguity.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert_eq!(tokio::fs::read(&destination).await?, output);
    assert!(verified_path(&fixture.temp_root, prepared.task_id).exists());
    assert_ambiguous(&fixture, &prepared).await?;
    Ok(())
}

#[tokio::test]
async fn matching_final_and_corrupt_verified_temp_preserve_both_as_ambiguous() -> TestResult {
    // Given: final bytes match durable evidence but the still-present temp artifact was corrupted.
    let server = MockVidenoa::start().await?;
    let output = b"matching final with corrupt temp".repeat(1024);
    let (fixture, prepared, destination) =
        publishing_task(&server, &output, PublicationIntent::direct()).await?;
    tokio::fs::write(&destination, &output).await?;
    let verified = verified_path(&fixture.temp_root, prepared.task_id);
    let corrupt = b"corrupt temp bytes";
    tokio::fs::write(&verified, corrupt).await?;

    // When: recovery inspects both durable locations without repairing either one.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: both artifacts are preserved and automated recovery is blocked as ambiguous.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert_eq!(tokio::fs::read(&destination).await?, output);
    assert_eq!(tokio::fs::read(&verified).await?, corrupt);
    assert_ambiguous(&fixture, &prepared).await?;
    Ok(())
}

#[tokio::test]
async fn matching_final_and_temp_without_sidecar_preserve_both_as_ambiguous() -> TestResult {
    // Given: matching final and temp bytes coexist but the temp verification sidecar is missing.
    let server = MockVidenoa::start().await?;
    let output = b"matching final with missing sidecar".repeat(1024);
    let (fixture, prepared, destination) =
        publishing_task(&server, &output, PublicationIntent::direct()).await?;
    tokio::fs::write(&destination, &output).await?;
    let verified = verified_path(&fixture.temp_root, prepared.task_id);
    let sidecar = verified.with_file_name("output.mp4.verified.evidence");
    tokio::fs::remove_file(&sidecar).await?;

    // When: recovery observes matching bytes in both durable locations.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: final and temp bytes remain untouched and the state is publication ambiguity.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert_eq!(tokio::fs::read(&destination).await?, output);
    assert_eq!(tokio::fs::read(&verified).await?, output);
    assert!(!sidecar.exists());
    assert_ambiguous(&fixture, &prepared).await?;
    Ok(())
}

#[tokio::test]
async fn missing_sidecar_does_not_block_db_verified_temp_publication() -> TestResult {
    // Given: durable DB hash/size and verified temp bytes remain, but the download sidecar is gone.
    let server = MockVidenoa::start().await?;
    let output = b"db evidence replaces publication sidecar".repeat(1024);
    let (fixture, prepared, _) =
        publishing_task(&server, &output, PublicationIntent::direct()).await?;
    let verified = verified_path(&fixture.temp_root, prepared.task_id);
    tokio::fs::remove_file(verified.with_file_name("output.mp4.verified.evidence")).await?;

    // When: Publishing recovery validates the temp leaf directly against durable DB evidence.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: exact output publication and cleanup complete without reconstructing a sidecar.
    assert_eq!(outcome, PublicationOutcome::Completed);
    assert_eq!(
        tokio::fs::read(output_path(&fixture, &prepared).await?).await?,
        output
    );
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::Completed
    );
    Ok(())
}

#[tokio::test]
async fn mismatching_final_without_temp_is_preserved_as_ambiguous() -> TestResult {
    // Given: a crash-shaped direct intent has no verified source and an unrelated final file.
    let server = MockVidenoa::start().await?;
    let output = b"expected final publication".repeat(1024);
    let (fixture, prepared, destination) =
        publishing_task(&server, &output, PublicationIntent::direct()).await?;
    tokio::fs::remove_dir_all(fixture.temp_root.join(prepared.task_id.to_string())).await?;
    tokio::fs::write(&destination, b"unknown final bytes").await?;

    // When: recovery cannot prove the final from durable hash and size.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: the unknown final is untouched and automated retry is blocked.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert_eq!(tokio::fs::read(&destination).await?, b"unknown final bytes");
    assert_ambiguous(&fixture, &prepared).await?;
    Ok(())
}

async fn publishing_task(
    server: &MockVidenoa,
    output: &[u8],
    intent: PublicationIntent,
) -> TestResult<(Fixture, PreparedTask, PathBuf)> {
    let (fixture, prepared) = verified_task(server, output).await?;
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    LifecycleService::new(fixture.store.clone())
        .advance(
            &task,
            &attempt,
            AdvanceCommand::FinishVerification(intent),
            fixture.now,
        )
        .await?;
    let destination = output_path(&fixture, &prepared).await?;
    Ok((fixture, prepared, destination))
}

async fn assert_ambiguous(fixture: &Fixture, prepared: &PreparedTask) -> TestResult {
    assert_eq!(
        fixture
            .task(prepared.task_id)
            .await?
            .failure
            .map(|failure| failure.failure_code),
        Some(FailureCode::PublicationAmbiguous)
    );
    Ok(())
}
