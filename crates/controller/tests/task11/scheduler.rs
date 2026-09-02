use chrono::Duration;
use videnoa_controller::domain::{
    ComputeSlots, ConcurrencyLimit, RetrySettingsDto, SchedulerStatus, TimeoutSettingsDto,
};
use videnoa_controller::lifecycle::DurableAction;
use videnoa_controller::persistence::SettingsUpdate;
use videnoa_controller::scheduler::{
    AssignmentClass, Scheduler, SchedulerErrorCode, TransferCoordinator, UploadPriority,
};

use super::support::{fixture, online, task, task_id, worker_id, worker_request, TestResult};

#[tokio::test]
async fn prefetch_is_bounded_and_idle_uploads_precede_optional_prefetch() -> TestResult {
    // Given: one three-slot worker and three equal-priority tasks.
    let fixture = fixture().await?;
    let worker = fixture
        .registry
        .create(
            worker_request("worker-a", "https://worker.example/api/", 3)?,
            fixture.now,
        )
        .await?;
    online(&fixture, worker.id, worker.version, &["anime-upscale"]).await?;
    for id in [task_id(31), task_id(32), task_id(33)] {
        fixture
            .store
            .insert_task(&task(id, "anime-upscale", 10, fixture.now))
            .await?;
    }
    let scheduler = Scheduler::load(fixture.store.clone()).await?;

    // When: the scheduler fills one idle feed plus the configured single prefetch.
    let first = scheduler
        .reserve_next(fixture.now)
        .await?
        .ok_or_else(|| std::io::Error::other("idle feed missing"))?;
    let second = scheduler
        .reserve_next(fixture.now)
        .await?
        .ok_or_else(|| std::io::Error::other("prefetch missing"))?;
    let bounded = scheduler.reserve_next(fixture.now).await?;
    let uploads = scheduler.upload_candidates(10).await?;

    // Then: optional prefetch is capped and the idle feed is first deterministically.
    assert_eq!(first.class(), AssignmentClass::IdleFeed);
    assert_eq!(second.class(), AssignmentClass::Prefetch);
    assert!(bounded.is_none());
    assert_eq!(uploads.len(), 2);
    assert_eq!(uploads[0].task_id(), first.task_id());
    assert_eq!(uploads[0].priority(), UploadPriority::IdleFeed);
    assert_eq!(uploads[1].task_id(), second.task_id());
    assert_eq!(uploads[1].priority(), UploadPriority::Prefetch);
    Ok(())
}

#[tokio::test]
async fn persisted_pause_blocks_only_new_reservation_upload_and_submission() -> TestResult {
    // Given: one durable staged reservation and default unpaused settings.
    let fixture = fixture().await?;
    let worker = fixture
        .registry
        .create(
            worker_request("worker-a", "https://worker.example/api/", 1)?,
            fixture.now,
        )
        .await?;
    online(&fixture, worker.id, worker.version, &["anime-upscale"]).await?;
    let existing = task_id(41);
    let queued = task_id(42);
    fixture
        .store
        .insert_task(&task(existing, "anime-upscale", 20, fixture.now))
        .await?;
    fixture
        .store
        .insert_task(&task(queued, "anime-upscale", 10, fixture.now))
        .await?;
    let scheduler = Scheduler::load(fixture.store.clone()).await?;
    scheduler
        .reserve_next(fixture.now)
        .await?
        .ok_or_else(|| std::io::Error::other("existing reservation missing"))?;
    let settings = fixture.store.settings().await?;
    let mut paused_status = settings.scheduler.clone();
    paused_status.paused = true;

    // When: pause is committed and a new Scheduler is loaded from the same database.
    scheduler
        .update_settings(SettingsUpdate {
            expected_version: settings.version,
            scheduler: paused_status,
            timeouts: settings.timeouts,
            retry: settings.retry,
            updated_at: fixture.now,
        })
        .await?;
    let restarted = Scheduler::load(fixture.store.clone()).await?;

    // Then: ambiguous/staged ownership remains while only forward-safe stages continue.
    assert!(restarted.reserve_next(fixture.now).await?.is_none());
    assert!(restarted.upload_candidates(10).await?.is_empty());
    assert!(!restarted.allows(DurableAction::Upload).await?);
    assert!(!restarted.allows(DurableAction::Submit).await?);
    for action in [
        DurableAction::Poll,
        DurableAction::Download,
        DurableAction::Verify,
        DurableAction::Publish,
        DurableAction::Cleanup,
    ] {
        assert!(restarted.allows(action).await?);
    }
    assert_eq!(fixture.store.worker_used_slots(worker.id).await?, 1);
    assert_eq!(
        fixture
            .store
            .task(existing)
            .await?
            .ok_or_else(|| std::io::Error::other("reserved task missing"))?
            .worker_id,
        Some(worker.id)
    );
    assert!(fixture
        .store
        .task(queued)
        .await?
        .ok_or_else(|| std::io::Error::other("queued task missing"))?
        .worker_id
        .is_none());
    Ok(())
}

#[tokio::test]
async fn upload_and_download_limits_are_independent_with_one_upload_per_worker() -> TestResult {
    // Given: two upload permits, one download permit, and three worker IDs.
    let limits = TransferCoordinator::new(2, 1)?;
    let first_worker = worker_id(51);
    let second_worker = worker_id(52);
    let third_worker = worker_id(53);

    // When: both upload slots and the independent download slot are occupied.
    let first_upload = limits
        .try_upload(first_worker)
        .ok_or_else(|| std::io::Error::other("first upload permit missing"))?;
    assert!(limits.try_upload(first_worker).is_none());
    let second_upload = limits
        .try_upload(second_worker)
        .ok_or_else(|| std::io::Error::other("second upload permit missing"))?;
    assert!(limits.try_upload(third_worker).is_none());
    let download = limits
        .try_download()
        .ok_or_else(|| std::io::Error::other("download permit missing"))?;
    assert!(limits.try_download().is_none());

    // Then: releasing one upload never releases or consumes the download pool.
    drop(first_upload);
    let third_upload = limits
        .try_upload(third_worker)
        .ok_or_else(|| std::io::Error::other("third upload permit missing"))?;
    assert!(limits.try_download().is_none());
    drop((second_upload, third_upload, download));
    assert!(limits.try_download().is_some());
    Ok(())
}

#[tokio::test]
async fn scheduler_settings_update_has_typed_stale_conflict_and_reconfigures_limits() -> TestResult
{
    // Given: one loaded scheduler and a settings snapshot.
    let fixture = fixture().await?;
    let scheduler = Scheduler::load(fixture.store.clone()).await?;
    let settings = fixture.store.settings().await?;
    let update = SettingsUpdate {
        expected_version: settings.version,
        scheduler: SchedulerStatus {
            paused: false,
            default_compute_slots: ComputeSlots::try_from(1_u64)?,
            prefetch_per_worker: 1,
            max_concurrent_uploads: ConcurrencyLimit::try_from(2_u64)?,
            max_concurrent_downloads: ConcurrencyLimit::try_from(3_u64)?,
        },
        timeouts: TimeoutSettingsDto {
            health_seconds: 10,
            poll_seconds: 5,
            transfer_seconds: 300,
        },
        retry: RetrySettingsDto {
            initial_seconds: 1,
            maximum_seconds: 60,
            max_attempts: 5,
        },
        updated_at: fixture.now + Duration::seconds(1),
    };

    // When: one settings CAS commits and the same snapshot is replayed.
    scheduler.update_settings(update.clone()).await?;
    let stale = scheduler
        .update_settings(update)
        .await
        .expect_err("stale settings update must conflict");

    // Then: conflict is typed and the live coordinator uses independent new bounds.
    assert_eq!(stale.code(), SchedulerErrorCode::Conflict);
    let first = scheduler
        .transfers()
        .try_upload(worker_id(61))
        .ok_or_else(|| std::io::Error::other("first upload missing"))?;
    let second = scheduler
        .transfers()
        .try_upload(worker_id(62))
        .ok_or_else(|| std::io::Error::other("second upload missing"))?;
    assert!(scheduler.transfers().try_upload(worker_id(63)).is_none());
    let downloads = [
        scheduler.transfers().try_download(),
        scheduler.transfers().try_download(),
        scheduler.transfers().try_download(),
    ];
    assert!(downloads.iter().all(Option::is_some));
    assert!(scheduler.transfers().try_download().is_none());
    drop((first, second, downloads));
    Ok(())
}
