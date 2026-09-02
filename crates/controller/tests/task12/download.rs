use sha2::{Digest, Sha256};
use videnoa_controller::domain::TaskStatus;
use videnoa_controller::scheduler::DownloadOutcome;

use crate::mock_videnoa::faults::Fault;
use crate::mock_videnoa::journal::{HeaderValueSnapshot, Route};
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{part_path, verified_path, zero_jitter, Fixture, TestResult};

#[tokio::test]
async fn truncated_download_is_discarded_and_keeps_compute_identity() -> TestResult {
    // Given: one confirmed completed job and a GET that truncates after five bytes.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let output = vec![11_u8; 20_000];
    let prepared = fixture.remote_completed(&server, &output).await?;
    let job_id = fixture.remote_job_id(prepared.attempt_id).await?;
    let submission_key = fixture
        .attempt(prepared.attempt_id)
        .await?
        .attempt
        .submission_key;
    server
        .set_fault(Fault::TruncateDownload { delivered_bytes: 5 })
        .await;

    // When: the download stage receives a truncated body.
    let outcome = fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;

    // Then: the partial is gone, retry is durable, and compute identity was not replayed.
    assert!(matches!(
        outcome,
        DownloadOutcome::RetryScheduled { retry_count: 1, .. }
    ));
    assert!(!part_path(&fixture.temp_root, prepared.task_id).exists());
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    assert_eq!(attempt.attempt.remote_job_id, Some(job_id));
    assert_eq!(attempt.attempt.submission_key, submission_key);
    assert_eq!(server.counters().await.get(Route::Run), 1);
    Ok(())
}

#[tokio::test]
async fn zero_length_download_is_rejected_before_verification() -> TestResult {
    // Given: a confirmed completed job whose remote output is empty.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let prepared = fixture.remote_completed(&server, &[]).await?;

    // When: the download stage observes exact zero length.
    let outcome = fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;

    // Then: it schedules a retry and never exposes a verified artifact.
    assert!(matches!(
        outcome,
        DownloadOutcome::RetryScheduled { retry_count: 1, .. }
    ));
    assert!(!verified_path(&fixture.temp_root, prepared.task_id).exists());
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::Downloading
    );
    Ok(())
}

#[tokio::test]
async fn downloading_restart_truncates_part_and_never_uses_range() -> TestResult {
    // Given: a downloading task with stale oversized part bytes from a prior process.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let output = b"verified output bytes from zero".repeat(700);
    let prepared = fixture.remote_completed(&server, &output).await?;
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    videnoa_controller::lifecycle::LifecycleService::new(fixture.store.clone())
        .advance(
            &task,
            &attempt,
            videnoa_controller::lifecycle::AdvanceCommand::StartDownload,
            fixture.now,
        )
        .await?;
    let part = part_path(&fixture.temp_root, prepared.task_id);
    tokio::fs::create_dir_all(
        part.parent()
            .ok_or_else(|| std::io::Error::other("part parent missing"))?,
    )
    .await?;
    tokio::fs::write(&part, vec![99_u8; output.len() + 500]).await?;

    // When: restart dispatch runs download again from durable Downloading.
    let outcome = fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;

    // Then: the verified file is exact, hash/length are durable, and no Range header was sent.
    let DownloadOutcome::Verified(artifact) = outcome else {
        return Err(std::io::Error::other("download did not verify").into());
    };
    assert_eq!(tokio::fs::read(&artifact.path).await?, output);
    assert_eq!(artifact.size, u64::try_from(output.len())?);
    assert_eq!(
        artifact.sha256.as_bytes(),
        Sha256::digest(&output).as_slice()
    );
    let task = fixture.task(prepared.task_id).await?;
    assert_eq!(task.status, TaskStatus::Verifying);
    assert_eq!(task.publication.expected_output_size, Some(artifact.size));
    assert_eq!(
        task.publication.expected_output_sha256,
        Some(artifact.sha256)
    );
    let download = server
        .journal()
        .await
        .into_iter()
        .find(|entry| entry.route == Route::Download)
        .ok_or_else(|| std::io::Error::other("download journal missing"))?;
    assert!(download.headers.iter().all(|header| {
        header.name != "range" || header.value == HeaderValueSnapshot::Bytes(Vec::new())
    }));
    Ok(())
}
