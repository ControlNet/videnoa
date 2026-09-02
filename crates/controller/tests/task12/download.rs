use chrono::Duration;
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use videnoa_controller::domain::{AttemptId, FailureCode, TaskStatus};
use videnoa_controller::scheduler::{DownloadOutcome, TransferError};

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

#[tokio::test]
async fn successful_download_clears_retry_metadata_on_task_and_attempt() -> TestResult {
    // Given: a confirmed output whose first download schedules a durable retry.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let output = vec![13_u8; 20_000];
    let prepared = fixture.remote_completed(&server, &output).await?;
    server
        .set_fault(Fault::TruncateDownload { delivered_bytes: 5 })
        .await;
    let first = fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;
    assert!(matches!(
        first,
        DownloadOutcome::RetryScheduled { retry_count: 1, .. }
    ));

    // When: the retry succeeds after its durable deadline.
    fixture
        .executor()?
        .download(
            prepared.task_id,
            fixture.now + Duration::seconds(2),
            zero_jitter()?,
        )
        .await?;

    // Then: both paired rows clear the completed stage's retry metadata.
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    assert_eq!(task.retry.retry_count, 0);
    assert_eq!(task.retry.next_retry_at, None);
    assert_eq!(attempt.attempt.retry.retry_count, 0);
    assert_eq!(attempt.attempt.retry.next_retry_at, None);
    Ok(())
}

#[tokio::test]
async fn download_retry_is_not_admitted_before_durable_deadline() -> TestResult {
    // Given: a failed download with a future durable retry deadline.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let output = vec![19_u8; 20_000];
    let prepared = fixture.remote_completed(&server, &output).await?;
    server
        .set_fault(Fault::TruncateDownload { delivered_bytes: 5 })
        .await;
    fixture
        .executor()?
        .download(
            prepared.task_id,
            fixture.now,
            videnoa_controller::lifecycle::JitterSample::try_from(10_000)?,
        )
        .await?;
    let before = server.counters().await.get(Route::Download);

    // When: dispatch retries at the same instant, before the persisted deadline.
    let second = fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await;

    // Then: admission is deferred without issuing another GET.
    assert!(matches!(second, Err(TransferError::RetryNotDue)));
    assert_eq!(server.counters().await.get(Route::Download), before);
    Ok(())
}

#[tokio::test]
async fn restart_reuses_matching_verified_artifact_after_rename_before_cas() -> TestResult {
    // Given: a downloading task with the exact verified artifact from a crashed prior process.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let output = vec![29_u8; 20_000];
    let prepared = fixture.remote_completed(&server, &output).await?;
    fixture.mark_downloading(&prepared).await?;
    let verified = verified_path(&fixture.temp_root, prepared.task_id);
    tokio::fs::create_dir_all(
        verified
            .parent()
            .ok_or_else(|| std::io::Error::other("verified parent missing"))?,
    )
    .await?;
    tokio::fs::write(&verified, &output).await?;

    // When: restart repeats download and reaches the existing verified path.
    let outcome = fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;

    // Then: identical evidence is preserved and the lifecycle CAS completes.
    assert!(matches!(outcome, DownloadOutcome::Verified(_)));
    assert_eq!(tokio::fs::read(&verified).await?, output);
    assert!(!part_path(&fixture.temp_root, prepared.task_id).exists());
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::Verifying
    );
    Ok(())
}

#[tokio::test]
async fn restart_replaces_mismatching_verified_artifact_before_cas() -> TestResult {
    // Given: a downloading task with stale bytes at the fixed verified path.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let output = vec![31_u8; 20_000];
    let prepared = fixture.remote_completed(&server, &output).await?;
    fixture.mark_downloading(&prepared).await?;
    let verified = verified_path(&fixture.temp_root, prepared.task_id);
    tokio::fs::create_dir_all(
        verified
            .parent()
            .ok_or_else(|| std::io::Error::other("verified parent missing"))?,
    )
    .await?;
    tokio::fs::write(&verified, b"stale verified bytes").await?;

    // When: restart installs the newly downloaded verified artifact.
    let outcome = fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;

    // Then: the fixed path contains only the exact current remote output.
    assert!(matches!(outcome, DownloadOutcome::Verified(_)));
    assert_eq!(tokio::fs::read(&verified).await?, output);
    assert!(!part_path(&fixture.temp_root, prepared.task_id).exists());
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::Verifying
    );
    Ok(())
}

#[tokio::test]
async fn missing_remote_job_evidence_fails_before_download_network_access() -> TestResult {
    // Given: a remote-completed attempt whose durable job identity is missing.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let output = vec![53_u8; 20_000];
    let prepared = fixture.remote_completed(&server, &output).await?;
    update_attempt(
        &fixture,
        prepared.attempt_id,
        "UPDATE task_attempts SET remote_job_id = NULL WHERE id = ?",
    )
    .await?;
    let stat_before = server.counters().await.get(Route::Stat);
    let get_before = server.counters().await.get(Route::Download);

    // When: download admission validates the paired remote evidence.
    let outcome = fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;

    // Then: ambiguity is terminal before stat or GET can observe another output.
    assert!(matches!(outcome, DownloadOutcome::Failed));
    let task = fixture.task(prepared.task_id).await?;
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(
        task.failure.map(|failure| failure.failure_code),
        Some(FailureCode::RemoteStateAmbiguous)
    );
    assert_eq!(server.counters().await.get(Route::Stat), stat_before);
    assert_eq!(server.counters().await.get(Route::Download), get_before);
    Ok(())
}

#[tokio::test]
async fn contradictory_remote_output_evidence_fails_before_download_network_access() -> TestResult {
    // Given: a remote-completed attempt whose output is not the input's exact opaque sibling.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let output = vec![59_u8; 20_000];
    let prepared = fixture.remote_completed(&server, &output).await?;
    update_attempt(
        &fixture,
        prepared.attempt_id,
        "UPDATE task_attempts SET remote_output_path = 'other/output.mp4' WHERE id = ?",
    )
    .await?;
    let stat_before = server.counters().await.get(Route::Stat);
    let get_before = server.counters().await.get(Route::Download);

    // When: download admission validates exact durable workflow-path evidence.
    let outcome = fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;

    // Then: ambiguity closes both rows without stat or GET.
    assert!(matches!(outcome, DownloadOutcome::Failed));
    let task = fixture.task(prepared.task_id).await?;
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(
        task.failure.map(|failure| failure.failure_code),
        Some(FailureCode::RemoteStateAmbiguous)
    );
    assert_eq!(server.counters().await.get(Route::Stat), stat_before);
    assert_eq!(server.counters().await.get(Route::Download), get_before);
    Ok(())
}

async fn update_attempt(fixture: &Fixture, attempt_id: AttemptId, sql: &str) -> TestResult {
    let options =
        SqliteConnectOptions::new().filename(fixture.directory.path().join("controller.sqlite3"));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    sqlx::query(sql)
        .bind(attempt_id.to_string())
        .execute(&pool)
        .await?;
    pool.close().await;
    Ok(())
}
