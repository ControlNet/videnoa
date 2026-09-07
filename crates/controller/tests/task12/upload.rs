use videnoa_controller::domain::TaskStatus;
use videnoa_controller::lifecycle::JitterSample;
use videnoa_controller::persistence::SettingsUpdate;
use videnoa_controller::scheduler::UploadOutcome;

use crate::mock_videnoa::faults::{DeleteOutcome, Fault};
use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{zero_jitter, Fixture, TestResult};

#[tokio::test]
async fn upload_persists_exact_opaque_paths_after_exact_stat() -> TestResult {
    // Given: a rooted input larger than the configured transfer chunk.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let prepared = fixture.reserved_task(vec![3_u8; 20_000]).await?;

    // When: the upload stage streams and confirms remote stat evidence.
    let outcome = fixture
        .executor()?
        .upload(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;

    // Then: exact opaque workflow paths are durable only after byte-exact stat proof.
    let UploadOutcome::Staged(evidence) = outcome else {
        return Err(std::io::Error::other("upload did not stage").into());
    };
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    assert_eq!(task.status, TaskStatus::Staged);
    assert_eq!(
        attempt.attempt.remote_input_path,
        Some(evidence.remote_input_path)
    );
    assert_eq!(
        attempt.attempt.remote_output_path,
        Some(evidence.remote_output_path)
    );
    assert_eq!(server.counters().await.get(Route::Upload), 1);
    assert_eq!(server.counters().await.get(Route::Stat), 1);
    Ok(())
}

#[tokio::test]
async fn legacy_input_without_content_identity_uploads_current_content() -> TestResult {
    // Given: a pre-migration reserved task whose content identity is absent.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let prepared = fixture.reserved_task(vec![4_u8; 12_000]).await?;
    sqlx::query("UPDATE tasks SET input_content_identity = NULL WHERE id = ?")
        .bind(prepared.task_id.to_string())
        .execute(fixture.store.database().pool())
        .await?;

    // Synthetic test-only bytes differ from the original admission content.
    rewrite_preserving_metadata(&fixture, prepared.task_id).await?;

    // When: upload admission evaluates the legacy durable snapshot.
    let outcome = fixture
        .executor()?
        .upload(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;

    // Then: legacy work uploads the current content.
    assert!(matches!(outcome, UploadOutcome::Staged(_)));
    assert_eq!(server.counters().await.get(Route::Upload), 1);
    Ok(())
}

#[tokio::test]
async fn upload_mismatch_deletes_only_owned_partial_and_retries_from_zero() -> TestResult {
    // Given: an uploading task whose owned remote target contains the wrong length.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let prepared = fixture.reserved_task(vec![5_u8; 12_000]).await?;
    fixture.mark_uploading(&prepared).await?;
    server
        .store_file(&format!("{}/input.mkv", prepared.task_id), &[9_u8; 7])
        .await?;
    // When: restart reconciliation proves the prior PUT left a mismatch.
    let first = fixture
        .executor()?
        .upload(prepared.task_id, fixture.now, JitterSample::try_from(0)?)
        .await?;

    // Then: the first invocation persists cleanup and a bounded retry.
    assert!(
        matches!(&first, UploadOutcome::RetryScheduled { retry_count: 1, .. }),
        "unexpected outcome: {first:?}"
    );
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    assert_eq!(task.status, TaskStatus::Uploading);
    assert_eq!(task.retry.retry_count, 1);
    assert_eq!(attempt.attempt.retry.retry_count, 1);
    assert_eq!(server.counters().await.get(Route::DeleteFile), 1);
    assert_eq!(server.counters().await.get(Route::Run), 0);

    // When: the durable deadline elapses and restart reconciliation runs again.
    let second = fixture
        .executor()?
        .upload(
            prepared.task_id,
            fixture.now + chrono::Duration::seconds(1),
            zero_jitter()?,
        )
        .await?;

    // Then: one fresh root-confined PUT starts at byte zero and reaches staged.
    assert!(matches!(second, UploadOutcome::Staged(_)));
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::Staged
    );
    assert_eq!(server.counters().await.get(Route::Upload), 1);
    assert_eq!(server.counters().await.get(Route::DeleteFile), 1);
    Ok(())
}

#[tokio::test]
async fn paused_scheduler_cannot_commit_upload_admission() -> TestResult {
    // Given: a reserved task and a durably paused scheduler.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let prepared = fixture.reserved_task(vec![17_u8; 12_000]).await?;
    let settings = fixture.store.config_manager().settings()?;
    let mut scheduler = settings.scheduler;
    scheduler.paused = true;
    fixture
        .store
        .config_manager()
        .update_settings(&SettingsUpdate {
            expected_version: settings.version,
            scheduler,
            timeouts: settings.timeouts,
            retry: settings.retry,
            updated_at: fixture.now,
        })
        .await?;

    // When: a stale production candidate attempts to begin upload.
    let result = fixture
        .executor()?
        .upload(prepared.task_id, fixture.now, zero_jitter()?)
        .await;

    // Then: the durable transition rejects admission before any remote request.
    assert!(result.is_err());
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::Reserved
    );
    assert_eq!(server.counters().await.get(Route::Upload), 0);
    Ok(())
}

#[tokio::test]
async fn replaced_input_uploads_current_size_and_content() -> TestResult {
    // Given: a reserved task whose rooted input is replaced before upload admission.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let prepared = fixture.reserved_task(vec![37_u8; 12_000]).await?;
    let input = fixture.task(prepared.task_id).await?.request.input_path;
    tokio::fs::remove_file(input.as_str()).await?;
    tokio::fs::write(input.as_str(), vec![41_u8; 15_000]).await?;

    // When: the executor opens the current input for upload.
    let outcome = fixture
        .executor()?
        .upload(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;

    // Then: replacement is accepted and the current size is durable for recovery.
    assert!(matches!(outcome, UploadOutcome::Staged(_)));
    let task = fixture.task(prepared.task_id).await?;
    assert_eq!(task.status, TaskStatus::Staged);
    assert_eq!(task.input_size, 15_000);
    assert!(task.failure.is_none());
    assert_eq!(server.counters().await.get(Route::Upload), 1);
    Ok(())
}

#[tokio::test]
async fn failed_partial_cleanup_still_persists_upload_retry() -> TestResult {
    // Given: restart finds a mismatched owned partial whose DELETE returns 500.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let prepared = fixture.reserved_task(vec![43_u8; 12_000]).await?;
    fixture.mark_uploading(&prepared).await?;
    server
        .store_file(&format!("{}/input.mkv", prepared.task_id), &[47_u8; 7])
        .await?;
    server
        .set_fault(Fault::DeleteScript(vec![DeleteOutcome::ServerError]))
        .await;

    // When: reconciliation cannot remove the mismatched remote partial.
    let outcome = fixture
        .executor()?
        .upload(prepared.task_id, fixture.now, zero_jitter()?)
        .await;

    // Then: cleanup failure remains an explicit durable upload retry.
    assert!(outcome.is_err());
    assert_eq!(fixture.task(prepared.task_id).await?.retry.retry_count, 1);
    assert_eq!(server.counters().await.get(Route::DeleteFile), 1);
    Ok(())
}

#[tokio::test]
async fn changed_bytes_with_identical_metadata_upload_successfully() -> TestResult {
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let prepared = fixture.reserved_task(vec![37_u8; 12_000]).await?;
    rewrite_preserving_metadata(&fixture, prepared.task_id).await?;
    let outcome = fixture
        .executor()?
        .upload(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;
    assert!(matches!(outcome, UploadOutcome::Staged(_)));
    let task = fixture.task(prepared.task_id).await?;
    assert!(task.failure.is_none());
    assert_eq!(server.counters().await.get(Route::Upload), 1);
    Ok(())
}

async fn rewrite_preserving_metadata(
    fixture: &Fixture,
    task_id: videnoa_controller::domain::TaskId,
) -> TestResult {
    use std::io::Write;
    let task = fixture.task(task_id).await?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(task.request.input_path.as_str())?;
    let modified = file.metadata()?.modified()?;
    // Synthetic replacement content, deliberately preserving inode, size and exact mtime.
    file.write_all(&vec![99_u8; usize::try_from(task.input_size)?])?;
    file.set_times(std::fs::FileTimes::new().set_modified(modified))?;
    drop(file);
    let current = fixture.paths.open_input(task.request.input_path.as_str())?;
    assert_eq!(
        Some(videnoa_controller::persistence::InputIdentity::new(
            current.snapshot().platform_identity()
        )),
        task.input_identity
    );
    assert_eq!(current.snapshot().length, task.input_size);
    assert_eq!(current.snapshot().modified, modified);
    if let Some(expected) = task.input_content_identity {
        assert_ne!(
            videnoa_controller::persistence::InputContentIdentity::new(
                current.snapshot().content_identity()
            ),
            expected
        );
    }
    Ok(())
}

#[tokio::test]
async fn resized_upload_recovery_uses_persisted_current_size() -> TestResult {
    use crate::mock_videnoa::faults::ResponseFault;
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let prepared = fixture.reserved_task(vec![3_u8; 12_000]).await?;
    let input = fixture.task(prepared.task_id).await?.request.input_path;
    // Synthetic test-only content changes length after intake.
    tokio::fs::write(input.as_str(), vec![4_u8; 15_000]).await?;
    server
        .set_fault(Fault::Response(ResponseFault {
            route: Route::Stat,
            status: 500,
            body: Vec::new(),
        }))
        .await;
    let first = fixture
        .executor()?
        .upload(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;
    assert!(matches!(first, UploadOutcome::RetryScheduled { .. }));
    assert_eq!(fixture.task(prepared.task_id).await?.input_size, 15_000);
    let journal = server.journal().await;
    let uploaded = journal
        .iter()
        .find(|entry| entry.route == Route::Upload)
        .ok_or("upload missing")?;
    assert_eq!(uploaded.body, vec![4_u8; 15_000]);
    let second = fixture
        .executor()?
        .upload(
            prepared.task_id,
            fixture.now + chrono::Duration::seconds(1),
            zero_jitter()?,
        )
        .await?;
    assert!(matches!(second, UploadOutcome::Staged(_)));
    assert_eq!(server.counters().await.get(Route::Upload), 1);
    Ok(())
}

#[tokio::test]
async fn historical_input_changed_retries_with_current_file() -> TestResult {
    use videnoa_controller::domain::{FailureCode, FailureStage};
    use videnoa_controller::lifecycle::{LifecycleFailure, LifecycleService};
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let prepared = fixture.reserved_task(vec![3_u8; 12_000]).await?;
    fixture.mark_uploading(&prepared).await?;
    let service = LifecycleService::new(fixture.store.clone());
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    service
        .fail(
            &task,
            Some(&attempt),
            LifecycleFailure::terminal(
                TaskStatus::Uploading,
                FailureStage::Upload,
                FailureCode::InputChanged,
                "historical test-only input change",
            ),
            fixture.now,
        )
        .await?;
    let failed = fixture.task(prepared.task_id).await?;
    assert!(!failed.failure.as_ref().ok_or("failure missing")?.retryable);
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    service
        .retry_downstream(&failed, &attempt, fixture.now)
        .await?;
    tokio::fs::write(task.request.input_path.as_str(), vec![4_u8; 15_000]).await?;
    let outcome = fixture
        .executor()?
        .upload(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;
    assert!(matches!(outcome, UploadOutcome::Staged(_)));
    assert_eq!(fixture.task(prepared.task_id).await?.input_size, 15_000);
    let journal = server.journal().await;
    let uploaded = journal
        .iter()
        .find(|entry| entry.route == Route::Upload)
        .ok_or("upload missing")?;
    assert_eq!(uploaded.body, vec![4_u8; 15_000]);
    Ok(())
}

#[tokio::test]
async fn missing_input_still_fails_before_upload() -> TestResult {
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let prepared = fixture.reserved_task(vec![3_u8; 12_000]).await?;
    let task = fixture.task(prepared.task_id).await?;
    tokio::fs::remove_file(task.request.input_path.as_str()).await?;
    let outcome = fixture
        .executor()?
        .upload(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;
    assert!(matches!(outcome, UploadOutcome::Failed));
    assert_eq!(
        fixture
            .task(prepared.task_id)
            .await?
            .failure
            .ok_or("failure missing")?
            .failure_code,
        videnoa_controller::domain::FailureCode::InputUnavailable
    );
    assert_eq!(server.counters().await.get(Route::Upload), 0);
    Ok(())
}
