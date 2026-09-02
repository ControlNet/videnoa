use videnoa_controller::domain::{FailureCode, TaskStatus};
use videnoa_controller::lifecycle::{AdvanceCommand, LifecycleService, PublicationIntent};
use videnoa_controller::scheduler::TransferCheckpointPoint;
use videnoa_controller::scheduler::{DownloadOutcome, PublicationOutcome};

use crate::checkpoints::CheckpointGate;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{assert_status, output_path, publish, verified_task};
use crate::transfer_support::{verified_path, zero_jitter, Fixture, TestResult};

#[cfg(target_os = "linux")]
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};

#[tokio::test]
async fn staging_replacement_after_verification_is_never_accepted() -> TestResult {
    // Given: production publication is paused after hashing the durable staging leaf.
    let server = MockVidenoa::start().await?;
    let output = b"verified staging identity".repeat(1024);
    let replacement = b"replacement after verification".repeat(1024);
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
    let staging_name = task
        .publication
        .destination_staging_name
        .ok_or_else(|| std::io::Error::other("staging name missing at checkpoint"))?;
    let destination = output_path(&fixture, &prepared).await?;
    let parent = destination
        .parent()
        .ok_or_else(|| std::io::Error::other("destination parent missing"))?;
    let staging = parent.join(&staging_name);
    let verified_staging = parent.join(format!("{staging_name}.verified-by-test"));
    tokio::fs::rename(&staging, &verified_staging).await?;
    tokio::fs::write(&staging, &replacement).await?;

    // When: the real no-replace finalization resumes on the replaced leaf.
    gate.release();
    let outcome = publication.await??;

    // Then: replacement bytes are preserved as ambiguous evidence, never accepted as completed.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert_eq!(tokio::fs::read(&destination).await?, replacement);
    assert_eq!(tokio::fs::read(&verified_staging).await?, output);
    let task = fixture.task(prepared.task_id).await?;
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(
        task.failure.map(|failure| failure.failure_code),
        Some(FailureCode::PublicationAmbiguous)
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
async fn matching_final_after_crash_converges_without_verified_source() -> TestResult {
    // Given: production publication reaches the post-rename checkpoint before lifecycle CAS.
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
    assert_eq!(tokio::fs::read(&destination).await?, output);
    publication.abort();
    let _ = publication.await;

    // When: a fresh executor performs startup-style publication reconciliation.
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
    // Given: production publication is paused after staging verification but before no-replace rename.
    let server = MockVidenoa::start().await?;
    let output = b"race-safe publication".repeat(1024);
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
    let staging_name = task
        .publication
        .destination_staging_name
        .ok_or_else(|| std::io::Error::other("staging name missing at checkpoint"))?;
    let destination = output_path(&fixture, &prepared).await?;
    let staging = destination
        .parent()
        .ok_or_else(|| std::io::Error::other("destination parent missing"))?
        .join(&staging_name);
    tokio::fs::write(&destination, b"racing unrelated output").await?;

    // When: the actual no-replace rename resumes after the destination race.
    gate.release();
    let outcome = publication.await??;

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

#[tokio::test]
async fn matching_final_with_owned_staging_is_ambiguous() -> TestResult {
    // Given: durable publication evidence names both a matching final and a remaining staging file.
    let server = MockVidenoa::start().await?;
    let output = b"contradictory publication".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    let staging_name = ".videnoa-contradiction.staging";
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
    tokio::fs::write(&destination, &output).await?;
    tokio::fs::write(&staging, &output).await?;

    // When: publication recovery inspects both durable names.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: contradictory ownership is terminal and both artifacts are preserved.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert_eq!(tokio::fs::read(&destination).await?, output);
    assert_eq!(tokio::fs::read(&staging).await?, output);
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
async fn non_regular_final_is_ambiguous_without_opening_it() -> TestResult {
    // Given: a directory occupies the final publication leaf.
    let server = MockVidenoa::start().await?;
    let output = b"regular bytes".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    LifecycleService::new(fixture.store.clone())
        .advance(
            &task,
            &attempt,
            AdvanceCommand::FinishVerification(PublicationIntent::new(
                ".videnoa-directory-final.staging",
            )),
            fixture.now,
        )
        .await?;
    let destination = output_path(&fixture, &prepared).await?;
    tokio::fs::create_dir(&destination).await?;

    // When: publication recovery inspects the final leaf.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: it terminates as ambiguity and preserves the directory.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert!(destination.is_dir());
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
async fn non_regular_staging_is_ambiguous_without_opening_it() -> TestResult {
    // Given: a directory occupies the durable hidden staging leaf.
    let server = MockVidenoa::start().await?;
    let output = b"regular bytes".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    let staging_name = ".videnoa-directory-staging.staging";
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
    tokio::fs::create_dir(&staging).await?;

    // When: publication recovery inspects the staging leaf.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: it terminates as ambiguity and preserves the directory.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert!(staging.is_dir());
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

#[cfg(target_os = "linux")]
#[tokio::test]
async fn publication_copies_across_filesystems_before_atomic_finalization() -> TestResult {
    // Given: verified temp and destination roots are on different mounted filesystems.
    let server = MockVidenoa::start().await?;
    let output_directory = tempfile::TempDir::new_in("/dev/shm")?;
    let fixture = Fixture::new_with_output_directory(&server, 1, 1, output_directory).await?;
    let output = b"cross filesystem publication".repeat(1024);
    let prepared = fixture.remote_completed(&server, &output).await?;
    let download = fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;
    assert!(matches!(download, DownloadOutcome::Verified(_)));
    assert_ne!(
        std::fs::metadata(&fixture.temp_root)?.dev(),
        std::fs::metadata(&fixture.output_root)?.dev()
    );

    // When: the production publication path copies into destination-owned staging.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: no EXDEV rename is attempted and the exact output completes.
    assert_eq!(outcome, PublicationOutcome::Completed);
    assert_eq!(
        tokio::fs::read(output_path(&fixture, &prepared).await?).await?,
        output
    );
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

    // When: production publication attempts to create destination staging.
    let outcome = publish(&fixture, &prepared).await;
    std::fs::set_permissions(&fixture.output_root, std::fs::Permissions::from_mode(0o700))?;
    let outcome = outcome?;

    // Then: publication fails retryably without a final artifact or source loss.
    assert_eq!(outcome, PublicationOutcome::Failed);
    let task = fixture.task(prepared.task_id).await?;
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(
        task.failure.as_ref().map(|failure| failure.failure_code),
        Some(FailureCode::PublicationFailed)
    );
    assert!(task.failure.is_some_and(|failure| failure.retryable));
    assert!(!output_path(&fixture, &prepared).await?.exists());
    assert!(verified_path(&fixture.temp_root, prepared.task_id).exists());
    Ok(())
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn fifo_staging_is_ambiguous_without_blocking() -> TestResult {
    // Given: durable publication intent points to a real FIFO staging node.
    let server = MockVidenoa::start().await?;
    let output = b"fifo publication evidence".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    let staging_name = ".videnoa-fifo.staging";
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
    rustix::fs::mknodat(
        rustix::fs::CWD,
        &staging,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::RWXU,
        rustix::fs::makedev(0, 0),
    )?;

    // When: production recovery classifies the opened handle.
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        publish(&fixture, &prepared),
    )
    .await??;

    // Then: the FIFO is preserved and classified promptly as ambiguity.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert!(std::fs::symlink_metadata(&staging)?.file_type().is_fifo());
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
