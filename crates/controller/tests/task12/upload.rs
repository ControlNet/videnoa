use videnoa_controller::domain::{FailureCode, TaskStatus};
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
    let outcome = fixture
        .executor()?
        .upload(prepared.task_id, fixture.now, JitterSample::try_from(0)?)
        .await?;

    // Then: the attempt remains uploading with durable bounded retry metadata and owned cleanup.
    assert!(
        matches!(
            &outcome,
            UploadOutcome::RetryScheduled { retry_count: 1, .. }
        ),
        "unexpected outcome: {outcome:?}"
    );
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    assert_eq!(task.status, TaskStatus::Uploading);
    assert_eq!(task.retry.retry_count, 1);
    assert_eq!(attempt.attempt.retry.retry_count, 1);
    assert_eq!(server.counters().await.get(Route::DeleteFile), 1);
    assert_eq!(server.counters().await.get(Route::Run), 0);
    Ok(())
}

#[tokio::test]
async fn paused_scheduler_cannot_commit_upload_admission() -> TestResult {
    // Given: a reserved task and a durably paused scheduler.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let prepared = fixture.reserved_task(vec![17_u8; 12_000]).await?;
    let settings = fixture.store.settings().await?;
    let mut scheduler = settings.scheduler;
    scheduler.paused = true;
    fixture
        .store
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
async fn changed_input_closes_upload_with_nonretryable_failure() -> TestResult {
    // Given: a reserved task whose rooted input is replaced before upload admission.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let prepared = fixture.reserved_task(vec![37_u8; 12_000]).await?;
    let input = fixture.task(prepared.task_id).await?.request.input_path;
    tokio::fs::remove_file(input.as_str()).await?;
    tokio::fs::write(input.as_str(), vec![41_u8; 12_000]).await?;

    // When: the executor reopens and verifies the durable input snapshot.
    let outcome = fixture
        .executor()?
        .upload(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;

    // Then: both rows close without issuing a PUT for changed input bytes.
    assert!(matches!(outcome, UploadOutcome::Failed));
    let task = fixture.task(prepared.task_id).await?;
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(
        task.failure.map(|failure| failure.failure_code),
        Some(FailureCode::InputChanged)
    );
    assert_eq!(server.counters().await.get(Route::Upload), 0);
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
