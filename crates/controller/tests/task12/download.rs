use chrono::Duration;
use sha2::{Digest, Sha256};
use videnoa_controller::domain::TaskStatus;
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
async fn slow_download_failure_schedules_backoff_from_failure_clock_and_samples_per_retry_jitter(
) -> TestResult {
    use std::sync::{
        atomic::{AtomicI64, AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration as StdDuration;
    use videnoa_controller::lifecycle::JitterSample;
    use videnoa_controller::recovery::{Reconciler, RecoveryConfig, ShutdownCoordinator};
    use videnoa_controller::remote::{PayloadLimits, RemoteTimeouts};

    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 2, 2).await?;
    let output = vec![23_u8; 20_000];
    let first = fixture.remote_completed(&server, &output).await?;
    let second = fixture.remote_completed(&server, &[]).await?;
    let reconciler = Reconciler::new(
        fixture.store.clone(),
        RecoveryConfig::new(
            fixture.paths.clone(),
            RemoteTimeouts::new(
                StdDuration::from_secs(1),
                StdDuration::from_secs(3),
                StdDuration::from_secs(1),
            )?,
            PayloadLimits::new(1024 * 1024, 4096)?,
            StdDuration::from_secs(10),
            StdDuration::from_secs(60),
            3,
        ),
        ShutdownCoordinator::new(),
    );
    let report1 = reconciler
        .reconcile_task_id(first.task_id, fixture.now)
        .await?;
    let report2 = reconciler
        .reconcile_task_id(second.task_id, fixture.now)
        .await?;
    server
        .set_fault(Fault::TruncateDownload { delivered_bytes: 5 })
        .await;

    // Simulate a clock that advances by 45 seconds between stage start (step 0)
    // and failure recording (step >= 1), longer than the 10s initial retry backoff,
    // plus distinct deterministic jitter samples (0 vs 10_000) per dispatched retry.
    let base_ts = fixture.now.timestamp();
    let calls = Arc::new(AtomicI64::new(0));
    let clock_calls = Arc::clone(&calls);
    let clock = Arc::new(move || {
        let step = clock_calls.fetch_add(1, Ordering::SeqCst);
        let offset = if step == 0 { 0 } else { 45 };
        chrono::DateTime::from_timestamp(base_ts + offset, 0).expect("valid timestamp")
    });
    let samples = [0_u16, 10_000_u16];
    let sample_idx = Arc::new(AtomicUsize::new(0));
    let jitter_idx = Arc::clone(&sample_idx);
    let jitter_source = Arc::new(move || {
        let idx = jitter_idx.fetch_add(1, Ordering::SeqCst);
        JitterSample::try_from(samples[idx % samples.len()]).expect("valid jitter")
    });

    let executor = videnoa_controller::scheduler::TransferExecutor::new(
        videnoa_controller::scheduler::TransferResources {
            store: fixture.store.clone(),
            paths: fixture.paths.clone(),
            coordinator: fixture.coordinator.clone(),
        },
        videnoa_controller::scheduler::TransferConfig {
            payload_limits: PayloadLimits::new(1024 * 1024, 4096)?,
            runtime_settings: videnoa_controller::scheduler::RuntimeSettings::new(
                &videnoa_controller::domain::TimeoutSettingsDto {
                    health_seconds: 1,
                    poll_seconds: 3,
                    transfer_seconds: 1,
                },
                &videnoa_controller::domain::RetrySettingsDto {
                    initial_seconds: 10,
                    maximum_seconds: 60,
                    max_attempts: 3,
                },
            )?,
        },
    )
    .with_clock(clock)
    .with_jitter_source(jitter_source);

    executor.dispatch_recovery_live(&report1).await?;
    executor.dispatch_recovery_live(&report2).await?;

    let first_task = fixture.task(first.task_id).await?;
    let second_task = fixture.task(second.task_id).await?;
    let first_due = first_task
        .retry
        .next_retry_at
        .ok_or_else(|| std::io::Error::other("missing first retry deadline"))?;
    let second_due = second_task
        .retry
        .next_retry_at
        .ok_or_else(|| std::io::Error::other("missing second retry deadline"))?;

    // Both deadlines are anchored at post-transfer failure time (t+45s), yielding
    // t+50s (lower bound, jitter=0) and t+55s (upper bound, jitter=10_000).
    assert_eq!(first_due, fixture.now + Duration::seconds(50));
    assert_eq!(second_due, fixture.now + Duration::seconds(55));
    assert_eq!(sample_idx.load(Ordering::SeqCst), 2);
    Ok(())
}
