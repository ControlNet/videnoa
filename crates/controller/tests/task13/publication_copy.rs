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
        if checkpoint == TransferCheckpointPoint::PublicationCopyStarted {
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
