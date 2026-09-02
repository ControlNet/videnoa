use videnoa_controller::domain::TaskStatus;
use videnoa_controller::lifecycle::JitterSample;
use videnoa_controller::scheduler::UploadOutcome;

use crate::mock_videnoa::faults::Fault;
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
    server.set_fault(Fault::DisconnectBeforeAccept).await;

    // When: PUT is uncertain and exact stat proves a mismatch.
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
