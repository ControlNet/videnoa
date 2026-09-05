use videnoa_controller::domain::{FailureCode, TaskStatus};
use videnoa_controller::scheduler::{PublicationOutcome, TransferCheckpointPoint};

use crate::checkpoints::CheckpointGate;
use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{assert_status, output_path, publish, verified_task};
use crate::transfer_support::{verified_path, zero_jitter, Fixture, TestResult};

#[cfg(target_os = "linux")]
use std::os::unix::fs::{MetadataExt, PermissionsExt};

#[tokio::test]
async fn output_root_contains_no_controller_file_before_final_publication() -> TestResult {
    // Given: verified bytes are retained under temp_root until the final rename.
    let server = MockVidenoa::start().await?;
    let output = b"clean output root publication".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    assert!(std::fs::read_dir(&fixture.output_root)?.next().is_none());
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::Verifying
    );
    let gate = CheckpointGate::new(TransferCheckpointPoint::BeforeDestinationStaging);
    let executor = fixture.executor()?.with_checkpoint_observer(gate.clone());
    let task_id = prepared.task_id;
    let now = fixture.now;
    let publication = tokio::spawn(async move {
        let outcome = executor.publish(task_id, now, zero_jitter()?).await?;
        TestResult::Ok(outcome)
    });
    gate.wait().await?;

    // When: the output root is inspected immediately before publication.
    let entries = std::fs::read_dir(&fixture.output_root)?.collect::<Result<Vec<_>, _>>()?;

    // Then: no Controller intermediate is visible, and only the exact final appears afterward.
    assert!(entries.is_empty());
    gate.release();
    assert_eq!(publication.await??, PublicationOutcome::Completed);
    assert_eq!(std::fs::read_dir(&fixture.output_root)?.count(), 1);
    assert_eq!(
        tokio::fs::read(output_path(&fixture, &prepared).await?).await?,
        output
    );
    Ok(())
}

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
async fn racing_destination_preserves_final_and_verified_source() -> TestResult {
    // Given: direct publication is paused immediately before the no-replace rename.
    let server = MockVidenoa::start().await?;
    let output = b"race-safe publication".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let gate = CheckpointGate::new(TransferCheckpointPoint::BeforeDestinationStaging);
    let executor = fixture.executor()?.with_checkpoint_observer(gate.clone());
    let task_id = prepared.task_id;
    let now = fixture.now;
    let publication = tokio::spawn(async move {
        let outcome = executor.publish(task_id, now, zero_jitter()?).await?;
        TestResult::Ok(outcome)
    });
    gate.wait().await?;
    let destination = output_path(&fixture, &prepared).await?;
    tokio::fs::write(&destination, b"racing unrelated output").await?;

    // When: the atomic rename observes the collision.
    gate.release();
    let outcome = publication.await??;

    // Then: neither artifact is overwritten or deleted and ownership is ambiguous.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert_eq!(
        tokio::fs::read(&destination).await?,
        b"racing unrelated output"
    );
    assert_eq!(
        tokio::fs::read(verified_path(&fixture.temp_root, prepared.task_id)).await?,
        output
    );
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

#[tokio::test]
async fn verified_source_replacement_before_rename_never_reaches_output_root() -> TestResult {
    // Given: publication is paused before its final source revalidation and rename.
    let server = MockVidenoa::start().await?;
    let output = b"owned verified output".repeat(1024);
    let replacement = b"unowned replacement".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let gate = CheckpointGate::new(TransferCheckpointPoint::BeforeDestinationStaging);
    let executor = fixture.executor()?.with_checkpoint_observer(gate.clone());
    let task_id = prepared.task_id;
    let now = fixture.now;
    let publication = tokio::spawn(async move {
        let outcome = executor.publish(task_id, now, zero_jitter()?).await?;
        TestResult::Ok(outcome)
    });
    gate.wait().await?;
    let verified = verified_path(&fixture.temp_root, prepared.task_id);
    let preserved = verified.with_extension("verified-preserved-by-test");
    tokio::fs::rename(&verified, &preserved).await?;
    tokio::fs::write(&verified, &replacement).await?;

    // When: publication resumes and re-hashes the current source leaf.
    gate.release();
    let outcome = publication.await??;

    // Then: replacement bytes remain in temp and no final path is created.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert!(!output_path(&fixture, &prepared).await?.exists());
    assert_eq!(tokio::fs::read(&verified).await?, replacement);
    assert_eq!(tokio::fs::read(&preserved).await?, output);
    Ok(())
}

#[tokio::test]
async fn crash_after_rename_recovers_without_ai_replay() -> TestResult {
    // Given: publication reaches the post-rename checkpoint before lifecycle CAS.
    let server = MockVidenoa::start().await?;
    let output = b"crash recovered publication".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let gate = CheckpointGate::new(TransferCheckpointPoint::PublicationFinalized);
    let executor = fixture.executor()?.with_checkpoint_observer(gate.clone());
    let task_id = prepared.task_id;
    let now = fixture.now;
    let publication = tokio::spawn(async move {
        let outcome = executor.publish(task_id, now, zero_jitter()?).await?;
        TestResult::Ok(outcome)
    });
    gate.wait().await?;
    let destination = output_path(&fixture, &prepared).await?;
    let run_requests = server.counters().await.get(Route::Run);
    assert_eq!(tokio::fs::read(&destination).await?, output);
    publication.abort();
    let _ = publication.await;

    // When: a fresh executor reconciles the matching final with no verified source.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: cleanup completes without another remote compute request.
    assert_eq!(outcome, PublicationOutcome::Completed);
    assert_eq!(server.counters().await.get(Route::Run), run_requests);
    assert_eq!(tokio::fs::read(&destination).await?, output);
    assert_status(&fixture, &prepared, TaskStatus::Completed).await?;
    Ok(())
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn cross_filesystem_output_is_rejected_without_copy_fallback() -> TestResult {
    // Given: temp and output roots are on different mounted filesystems.
    let server = MockVidenoa::start().await?;
    let output_directory = tempfile::TempDir::new_in("/dev/shm")?;
    let temp_directory = tempfile::TempDir::new()?;
    assert_ne!(
        std::fs::metadata(temp_directory.path())?.dev(),
        std::fs::metadata(output_directory.path())?.dev()
    );

    // When: Controller starts normally, then evaluates an output on another filesystem.
    let fixture = Fixture::new_with_output_directory(&server, 1, 1, output_directory).await?;
    let result = fixture
        .paths
        .open_output(fixture.output_root.join("result.mp4"));

    // Then: the task gets a typed capability error without a copy or visible staging file.
    assert!(matches!(
        result,
        Err(videnoa_controller::paths::PathError::CrossFilesystemPublication { .. })
    ));
    assert_eq!(std::fs::read_dir(&fixture.output_root)?.count(), 0);
    Ok(())
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn destination_permission_denial_preserves_verified_bytes() -> TestResult {
    // Given: a verified task whose destination root becomes non-writable.
    let server = MockVidenoa::start().await?;
    let output = b"permission denied publication".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    std::fs::set_permissions(&fixture.output_root, std::fs::Permissions::from_mode(0o500))?;

    // When: direct publication attempts to open its retained destination parent.
    let outcome = publish(&fixture, &prepared).await;
    std::fs::set_permissions(&fixture.output_root, std::fs::Permissions::from_mode(0o700))?;
    let outcome = outcome?;

    // Then: publication fails without a final artifact or source loss.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert!(!output_path(&fixture, &prepared).await?.exists());
    assert!(verified_path(&fixture.temp_root, prepared.task_id).exists());
    Ok(())
}

#[tokio::test]
async fn external_output_crash_after_rename_recovers_without_ai_replay() -> TestResult {
    // Given: publication reaches the post-rename checkpoint before lifecycle CAS.
    let server = MockVidenoa::start().await?;
    let output = b"crash recovered publication".repeat(1024);
    let media = tempfile::TempDir::new()?;
    let mut fixture = Fixture::new_with_output_directory(&server, 1, 1, media).await?;
    fixture.paths = videnoa_controller::paths::PathCapabilities::open(
        &videnoa_controller::config::PathConfig {
            input_roots: vec![fixture.directory.path().to_path_buf()],
            output_roots: vec![fixture.directory.path().to_path_buf()],
            data_root: fixture.directory.path().join("data"),
            temp_root: fixture.temp_root.clone(),
        },
    )?;
    assert!(!fixture.output_root.starts_with(fixture.directory.path()));
    let prepared = fixture.remote_completed(&server, &output).await?;
    assert!(matches!(
        fixture
            .executor()?
            .download(prepared.task_id, fixture.now, zero_jitter()?)
            .await?,
        videnoa_controller::scheduler::DownloadOutcome::Verified(_)
    ));
    assert_eq!(std::fs::read_dir(&fixture.output_root)?.count(), 0);
    let gate = CheckpointGate::new(TransferCheckpointPoint::PublicationFinalized);
    let executor = fixture.executor()?.with_checkpoint_observer(gate.clone());
    let task_id = prepared.task_id;
    let now = fixture.now;
    let publication = tokio::spawn(async move {
        let outcome = executor.publish(task_id, now, zero_jitter()?).await?;
        TestResult::Ok(outcome)
    });
    gate.wait().await?;
    let destination = output_path(&fixture, &prepared).await?;
    let run_requests = server.counters().await.get(Route::Run);
    assert_eq!(tokio::fs::read(&destination).await?, output);
    assert_eq!(std::fs::read_dir(&fixture.output_root)?.count(), 1);
    publication.abort();
    let _ = publication.await;

    // When: a fresh executor reconciles the matching final with no verified source.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: cleanup completes without another remote compute request.
    assert_eq!(outcome, PublicationOutcome::Completed);
    assert_eq!(server.counters().await.get(Route::Run), run_requests);
    assert_eq!(tokio::fs::read(&destination).await?, output);
    assert_eq!(std::fs::read_dir(&fixture.output_root)?.count(), 1);
    assert_status(&fixture, &prepared, TaskStatus::Completed).await?;
    Ok(())
}
