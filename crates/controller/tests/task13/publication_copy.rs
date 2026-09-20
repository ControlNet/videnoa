use std::os::unix::fs::MetadataExt;
use std::time::Duration;

use videnoa_controller::domain::{FailureCode, TaskStatus};
use videnoa_controller::scheduler::{DownloadOutcome, PublicationOutcome, TransferCheckpointPoint};

use crate::checkpoints::CheckpointGate;
use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{output_path, publish};
use crate::transfer_support::{verified_path, zero_jitter, Fixture, PreparedTask, TestResult};

async fn crossed(server: &MockVidenoa, bytes: &[u8]) -> TestResult<(Fixture, PreparedTask)> {
    let output = tempfile::TempDir::new_in("/dev/shm")?;
    let fixture = Fixture::new_with_output_directory(server, 1, 1, output).await?;
    assert_ne!(
        std::fs::metadata(&fixture.temp_root)?.dev(),
        std::fs::metadata(&fixture.output_root)?.dev()
    );
    let task = fixture.remote_completed(server, bytes).await?;
    assert!(matches!(
        fixture
            .executor()?
            .download(task.task_id, fixture.now, zero_jitter()?)
            .await?,
        DownloadOutcome::Verified(_)
    ));
    Ok((fixture, task))
}

#[tokio::test]
async fn cross_filesystem_move_publishes_verified_bytes_without_overwrite_or_sibling_staging(
) -> TestResult {
    let server = MockVidenoa::start().await?;
    let bytes = b"synthetic cross-filesystem output".repeat(8192);
    let (fixture, task) = crossed(&server, &bytes).await?;
    let runs = server.counters().await.get(Route::Run);
    assert_eq!(
        publish(&fixture, &task).await?,
        PublicationOutcome::Completed
    );
    assert_eq!(std::fs::read(output_path(&fixture, &task).await?)?, bytes);
    assert_eq!(std::fs::read_dir(&fixture.output_root)?.count(), 1);
    assert!(!verified_path(&fixture.temp_root, task.task_id).exists());
    assert_eq!(server.counters().await.get(Route::Run), runs);
    Ok(())
}

#[tokio::test]
async fn cross_filesystem_copy_crash_recovers_partial_and_complete_outputs_without_ai_replay(
) -> TestResult {
    for checkpoint in [
        TransferCheckpointPoint::PublicationCopyCreated,
        TransferCheckpointPoint::PublicationCopyStarted,
        TransferCheckpointPoint::PublicationCopyChunkWritten,
        TransferCheckpointPoint::PublicationCopyVerified,
        TransferCheckpointPoint::PublicationFinalized,
    ] {
        let server = MockVidenoa::start().await?;
        let bytes = b"synthetic interrupted move".repeat(8192);
        let (fixture, task) = crossed(&server, &bytes).await?;
        let destination = output_path(&fixture, &task).await?;
        let runs = server.counters().await.get(Route::Run);
        let gate = CheckpointGate::new(checkpoint);
        let executor = fixture.executor()?.with_checkpoint_observer(gate.clone());
        let id = task.task_id;
        let now = fixture.now;
        let pending =
            tokio::spawn(async move { executor.publish(id, now, zero_jitter().unwrap()).await });
        tokio::time::timeout(Duration::from_secs(5), gate.wait()).await??;
        let length = std::fs::metadata(&destination)?.len();
        if matches!(
            checkpoint,
            TransferCheckpointPoint::PublicationCopyCreated
                | TransferCheckpointPoint::PublicationCopyStarted
        ) {
            assert_eq!(length, 0);
        } else if checkpoint == TransferCheckpointPoint::PublicationCopyChunkWritten {
            assert!(length > 0 && length < bytes.len() as u64);
        } else {
            assert_eq!(length, bytes.len() as u64);
        }
        assert_eq!(std::fs::read_dir(&fixture.output_root)?.count(), 1);
        pending.abort();
        assert!(pending.await.unwrap_err().is_cancelled());
        assert_eq!(fixture.task(id).await?.status, TaskStatus::Publishing);
        if length == 0 {
            assert!(
                !destination.exists(),
                "cancelled copy retained an empty output"
            );
        }
        assert_eq!(
            publish(&fixture, &task).await?,
            PublicationOutcome::Completed
        );
        assert_eq!(std::fs::read(&destination)?, bytes);
        assert_eq!(server.counters().await.get(Route::Run), runs);
    }
    Ok(())
}

#[tokio::test]
async fn interrupted_move_never_overwrites_replaced_corrupt_or_symlinked_destination() -> TestResult
{
    for replacement in ["replaced", "corrupt", "symlink"] {
        let server = MockVidenoa::start().await?;
        let bytes = b"synthetic owned source".repeat(8192);
        let (fixture, task) = crossed(&server, &bytes).await?;
        let destination = output_path(&fixture, &task).await?;
        let gate = CheckpointGate::new(TransferCheckpointPoint::PublicationCopyChunkWritten);
        let executor = fixture.executor()?.with_checkpoint_observer(gate.clone());
        let id = task.task_id;
        let now = fixture.now;
        let pending =
            tokio::spawn(async move { executor.publish(id, now, zero_jitter().unwrap()).await });
        tokio::time::timeout(Duration::from_secs(5), gate.wait()).await??;
        pending.abort();
        assert!(pending.await.unwrap_err().is_cancelled());
        if replacement != "corrupt" {
            std::fs::rename(
                &destination,
                destination.with_extension("preserved-by-test"),
            )?;
        }
        if replacement == "symlink" {
            std::os::unix::fs::symlink(verified_path(&fixture.temp_root, id), &destination)?;
        } else {
            std::fs::write(&destination, b"unrelated destination bytes")?;
        }
        assert_eq!(publish(&fixture, &task).await?, PublicationOutcome::Failed);
        assert_eq!(
            fixture.task(id).await?.failure.unwrap().failure_code,
            FailureCode::PublicationAmbiguous
        );
        assert_eq!(std::fs::read(verified_path(&fixture.temp_root, id))?, bytes);
        if replacement != "symlink" {
            assert_eq!(std::fs::read(&destination)?, b"unrelated destination bytes");
        }
    }
    Ok(())
}

#[tokio::test]
async fn replacing_completed_copy_before_source_removal_preserves_verified_source() -> TestResult {
    let server = MockVidenoa::start().await?;
    let bytes = b"synthetic verified source".repeat(4096);
    let (fixture, task) = crossed(&server, &bytes).await?;
    let destination = output_path(&fixture, &task).await?;
    let gate = CheckpointGate::new(TransferCheckpointPoint::PublicationCopyVerified);
    let executor = fixture.executor()?.with_checkpoint_observer(gate.clone());
    let id = task.task_id;
    let now = fixture.now;
    let pending =
        tokio::spawn(async move { executor.publish(id, now, zero_jitter().unwrap()).await });
    tokio::time::timeout(Duration::from_secs(5), gate.wait()).await??;
    std::fs::rename(
        &destination,
        destination.with_extension("preserved-by-test"),
    )?;
    std::fs::write(&destination, b"unrelated destination")?;
    gate.release();
    assert_eq!(pending.await??, PublicationOutcome::Failed);
    assert_eq!(
        fixture.task(id).await?.failure.unwrap().failure_code,
        FailureCode::PublicationAmbiguous
    );
    assert_eq!(std::fs::read(&destination)?, b"unrelated destination");
    assert_eq!(std::fs::read(verified_path(&fixture.temp_root, id))?, bytes);
    Ok(())
}

#[tokio::test]
async fn cross_filesystem_move_preserves_existing_destination() -> TestResult {
    let server = MockVidenoa::start().await?;
    let bytes = b"synthetic verified source".repeat(1024);
    let (fixture, task) = crossed(&server, &bytes).await?;
    let destination = output_path(&fixture, &task).await?;
    std::fs::write(&destination, b"pre-existing destination")?;
    assert_eq!(publish(&fixture, &task).await?, PublicationOutcome::Failed);
    assert_eq!(std::fs::read(&destination)?, b"pre-existing destination");
    assert_eq!(
        std::fs::read(verified_path(&fixture.temp_root, task.task_id))?,
        bytes
    );
    Ok(())
}

#[tokio::test]
async fn legacy_cross_mount_failure_upgrade_enables_only_publication_retry() -> TestResult {
    use videnoa_controller::domain::FailureStage;
    use videnoa_controller::lifecycle::{LifecycleFailure, LifecycleService};
    use videnoa_controller::persistence::{Database, DatabaseOptions};
    let server = MockVidenoa::start().await?;
    let bytes = b"synthetic legacy verified output".repeat(4096);
    let (fixture, prepared) = crossed(&server, &bytes).await?;
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture
        .store
        .current_attempt(prepared.task_id)
        .await?
        .unwrap();
    LifecycleService::new(fixture.store.clone())
        .fail(
            &task,
            Some(&attempt),
            LifecycleFailure::terminal(
                TaskStatus::Verifying,
                FailureStage::Publication,
                FailureCode::PublicationFailed,
                "atomic publication cannot cross filesystems",
            ),
            fixture.now,
        )
        .await?;
    let failed = fixture.task(prepared.task_id).await?;
    assert!(!failed.failure.as_ref().unwrap().retryable);
    // Synthetic legacy database: migration 0009 changes data only, so removing its
    // test bookkeeping entry reproduces an otherwise identical migration-0008 schema.
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version = 9")
        .execute(fixture.store.database().pool())
        .await?;
    let upgraded = Database::open(DatabaseOptions::new(
        fixture.directory.path().join("controller.sqlite3"),
    ))
    .await?;
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture
        .store
        .current_attempt(prepared.task_id)
        .await?
        .unwrap();
    assert_eq!(task.status, TaskStatus::Failed);
    assert!(task.failure.as_ref().unwrap().retryable);
    assert!(attempt.attempt.failure.as_ref().unwrap().retryable);
    assert_eq!(task.version, failed.version + 1);
    let runs = server.counters().await.get(Route::Run);
    LifecycleService::new(fixture.store.clone())
        .retry_downstream(&task, &attempt, fixture.now)
        .await?;
    assert_eq!(
        publish(&fixture, &prepared).await?,
        PublicationOutcome::Completed
    );
    assert_eq!(fixture.task(prepared.task_id).await?.attempt_count, 1);
    assert_eq!(
        std::fs::read(output_path(&fixture, &prepared).await?)?,
        bytes
    );
    assert_eq!(server.counters().await.get(Route::Run), runs);
    upgraded.pool().close().await;
    Ok(())
}

#[tokio::test]
async fn marker_creation_failure_removes_empty_output_and_allows_retry() -> TestResult {
    use videnoa_controller::lifecycle::LifecycleService;
    // Synthetic filesystem fault: a directory occupies the pending-marker leaf.
    let server = MockVidenoa::start().await?;
    let bytes = b"synthetic diagnostic source".repeat(1024);
    let (fixture, prepared) = crossed(&server, &bytes).await?;
    let workspace = fixture.temp_root.join(prepared.task_id.to_string());
    std::fs::create_dir(workspace.join("publication-copy.pending"))?;
    assert_eq!(
        publish(&fixture, &prepared).await?,
        PublicationOutcome::Failed
    );
    let failed = fixture.task(prepared.task_id).await?;
    let failure = failed.failure.as_ref().unwrap();
    assert_eq!(failure.failure_code, FailureCode::PublicationFailed);
    assert!(
        failure.message.contains("copy.create_pending_marker"),
        "{}",
        failure.message
    );
    assert!(failure.message.contains("io_kind="), "{}", failure.message);
    assert!(failure.message.contains("os_error="), "{}", failure.message);
    let destination = output_path(&fixture, &prepared).await?;
    assert!(!destination.exists());
    assert_eq!(
        std::fs::read(verified_path(&fixture.temp_root, prepared.task_id))?,
        bytes
    );
    let runs = server.counters().await.get(Route::Run);
    std::fs::remove_dir(workspace.join("publication-copy.pending"))?;
    LifecycleService::new(fixture.store.clone())
        .retry_downstream(
            &failed,
            &fixture.attempt(prepared.attempt_id).await?,
            fixture.now,
        )
        .await?;
    assert_eq!(
        publish(&fixture, &prepared).await?,
        PublicationOutcome::Completed
    );
    assert_eq!(std::fs::read(&destination)?, bytes);
    assert_eq!(server.counters().await.get(Route::Run), runs);
    Ok(())
}

#[tokio::test]
async fn changed_parent_failure_cleans_original_empty_output_and_preserves_replacement(
) -> TestResult {
    use videnoa_controller::config::PathConfig;
    use videnoa_controller::lifecycle::LifecycleService;
    use videnoa_controller::paths::PathCapabilities;
    // Synthetic directory replacement on tmpfs reproduces the reported identity failure.
    let server = MockVidenoa::start().await?;
    let bytes = b"synthetic parent replacement output".repeat(1024);
    let (mut fixture, task) = crossed(&server, &bytes).await?;
    fixture.paths = PathCapabilities::open(&PathConfig {
        input_roots: vec![fixture.input_root.clone()],
        output_roots: vec![fixture.output_root.parent().unwrap().to_path_buf()],
        data_root: fixture.directory.path().join("data"),
        temp_root: fixture.temp_root.clone(),
    })?;
    let destination = output_path(&fixture, &task).await?;
    let preserved = tempfile::TempDir::new_in("/dev/shm")?;
    let old_parent = preserved.path().join("original-parent");
    let gate = CheckpointGate::new(TransferCheckpointPoint::PublicationCopyCreated);
    let executor = fixture.executor()?.with_checkpoint_observer(gate.clone());
    let id = task.task_id;
    let now = fixture.now;
    let pending =
        tokio::spawn(async move { executor.publish(id, now, zero_jitter().unwrap()).await });
    gate.wait().await?;
    assert_eq!(std::fs::metadata(&destination)?.len(), 0);
    std::fs::rename(&fixture.output_root, &old_parent)?;
    std::fs::create_dir(&fixture.output_root)?;
    // A different empty file at the same visible path must not be removed by rollback.
    std::fs::write(&destination, [])?;
    gate.release();
    assert_eq!(pending.await??, PublicationOutcome::Failed);
    let failed = fixture.task(id).await?;
    assert_eq!(
        failed.failure.as_ref().unwrap().message,
        "copy.validate_destination: output_parent_changed"
    );
    assert!(!old_parent.join(destination.file_name().unwrap()).exists());
    assert_eq!(std::fs::metadata(&destination)?.len(), 0);
    assert_eq!(std::fs::read(verified_path(&fixture.temp_root, id))?, bytes);
    assert!(!fixture
        .temp_root
        .join(id.to_string())
        .join("publication-copy.evidence")
        .exists());
    // Remove only the externally created synthetic conflict before retrying the same attempt.
    std::fs::remove_file(&destination)?;
    let runs = server.counters().await.get(Route::Run);
    LifecycleService::new(fixture.store.clone())
        .retry_downstream(
            &failed,
            &fixture.attempt(task.attempt_id).await?,
            fixture.now,
        )
        .await?;
    assert_eq!(
        publish(&fixture, &task).await?,
        PublicationOutcome::Completed
    );
    assert_eq!(std::fs::read(&destination)?, bytes);
    assert_eq!(server.counters().await.get(Route::Run), runs);
    Ok(())
}

#[tokio::test]
async fn changed_parent_after_copy_starts_cannot_publish_a_matching_replacement() -> TestResult {
    use videnoa_controller::config::PathConfig;
    use videnoa_controller::paths::PathCapabilities;
    // Synthetic replacement with identical bytes proves identity checks are retained.
    let server = MockVidenoa::start().await?;
    let bytes = b"synthetic matching replacement output".repeat(1024);
    let (mut fixture, task) = crossed(&server, &bytes).await?;
    fixture.paths = PathCapabilities::open(&PathConfig {
        input_roots: vec![fixture.input_root.clone()],
        output_roots: vec![fixture.output_root.parent().unwrap().to_path_buf()],
        data_root: fixture.directory.path().join("data"),
        temp_root: fixture.temp_root.clone(),
    })?;
    let destination = output_path(&fixture, &task).await?;
    let preserved = tempfile::TempDir::new_in("/dev/shm")?;
    let old_parent = preserved.path().join("original-parent");
    let gate = CheckpointGate::new(TransferCheckpointPoint::PublicationCopyStarted);
    let executor = fixture.executor()?.with_checkpoint_observer(gate.clone());
    let id = task.task_id;
    let now = fixture.now;
    let pending =
        tokio::spawn(async move { executor.publish(id, now, zero_jitter().unwrap()).await });
    gate.wait().await?;
    std::fs::rename(&fixture.output_root, &old_parent)?;
    std::fs::create_dir(&fixture.output_root)?;
    std::fs::write(&destination, &bytes)?;
    gate.release();
    assert_eq!(pending.await??, PublicationOutcome::Failed);
    let failed = fixture.task(id).await?;
    assert_eq!(
        failed.failure.as_ref().unwrap().message,
        "copy.final_identity_or_content_mismatch: evidence_conflict"
    );
    assert_eq!(std::fs::read(&destination)?, bytes);
    assert_eq!(
        std::fs::read(old_parent.join(destination.file_name().unwrap()))?,
        bytes
    );
    assert_eq!(std::fs::read(verified_path(&fixture.temp_root, id))?, bytes);
    Ok(())
}
