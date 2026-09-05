use videnoa_controller::domain::{ComputeSlots, WorkerName, WorkerUpdateRequest};
use videnoa_controller::workers::{WorkerRegistry, WorkerRegistryErrorCode};

use super::support::{fixture, online, task, task_id, worker_request, TestResult};

#[tokio::test]
async fn registry_reports_normalized_duplicates_and_stale_writes() -> TestResult {
    // Given: one durable worker created through the registry boundary.
    let fixture = fixture().await?;
    let created = fixture
        .registry
        .create(
            worker_request(" Worker-A ", "HTTPS://WORKER.EXAMPLE:443/api", 2)?,
            fixture.now,
        )
        .await?;

    // When: normalized name/URL duplicates and a stale update are attempted.
    let duplicate_name = fixture
        .registry
        .create(
            worker_request("worker-a", "https://other.example/api/", 1)?,
            fixture.now,
        )
        .await
        .expect_err("normalized worker name must be unique");
    let duplicate_url = fixture
        .registry
        .create(
            worker_request("worker-b", "https://worker.example:443/api/", 1)?,
            fixture.now,
        )
        .await
        .expect_err("canonical worker URL must be unique");
    let update = WorkerUpdateRequest {
        version: created.version,
        name: WorkerName::new("worker-renamed"),
        api_url: created.api_url.clone(),
        enabled: true,
        compute_slots: ComputeSlots::try_from(3_u64)?,
    };
    fixture
        .registry
        .update(created.id, update.clone(), fixture.now)
        .await?;
    let stale = fixture
        .registry
        .update(created.id, update, fixture.now)
        .await
        .expect_err("stale worker update must conflict");

    // Then: each failure has a stable typed classification.
    assert_eq!(
        duplicate_name.code(),
        WorkerRegistryErrorCode::DuplicateName
    );
    assert_eq!(
        duplicate_url.code(),
        WorkerRegistryErrorCode::DuplicateApiUrl
    );
    assert_eq!(stale.code(), WorkerRegistryErrorCode::Conflict);
    assert_eq!(created.name.as_str(), "Worker-A");
    Ok(())
}

#[tokio::test]
async fn disabling_busy_worker_preserves_assignment_and_blocks_new_work() -> TestResult {
    // Given: an online two-slot worker with one durable assignment.
    let fixture = fixture().await?;
    let worker = fixture
        .registry
        .create(
            worker_request("worker-a", "https://worker.example/api/", 2)?,
            fixture.now,
        )
        .await?;
    online(&fixture, worker.id, worker.version, &["anime-upscale"]).await?;
    let scheduler = videnoa_controller::scheduler::Scheduler::load(fixture.store.clone()).await?;
    let first = task_id(101);
    let second = task_id(102);
    fixture
        .store
        .insert_task(&task(first, "anime-upscale", 10, fixture.now))
        .await?;
    fixture
        .store
        .insert_task(&task(second, "anime-upscale", 9, fixture.now))
        .await?;
    let assignment = scheduler
        .reserve_next(fixture.now)
        .await?
        .ok_or_else(|| std::io::Error::other("first assignment missing"))?;
    let current = fixture
        .registry
        .worker(worker.id)
        .await?
        .ok_or_else(|| std::io::Error::other("worker missing"))?;

    // When: the busy worker is disabled.
    fixture
        .registry
        .set_enabled(worker.id, current.version, false, fixture.now)
        .await?;

    // Then: its current assignment and capacity remain, but no second task is claimed.
    assert_eq!(assignment.task_id(), first);
    assert!(scheduler.reserve_next(fixture.now).await?.is_none());
    assert_eq!(fixture.store.worker_used_slots(worker.id).await?, 0);
    assert_eq!(
        fixture
            .store
            .task(first)
            .await?
            .ok_or_else(|| std::io::Error::other("assigned task missing"))?
            .worker_id,
        Some(worker.id)
    );
    Ok(())
}

#[tokio::test]
async fn deletion_conflicts_for_active_and_historical_references() -> TestResult {
    // Given: one referenced worker and one never-used worker.
    let fixture = fixture().await?;
    let referenced = fixture
        .registry
        .create(
            worker_request("worker-a", "https://worker-a.example/api/", 1)?,
            fixture.now,
        )
        .await?;
    online(
        &fixture,
        referenced.id,
        referenced.version,
        &["anime-upscale"],
    )
    .await?;
    let unused = fixture
        .registry
        .create(
            worker_request("worker-b", "https://worker-b.example/api/", 1)?,
            fixture.now,
        )
        .await?;
    let task_id = task_id(201);
    fixture
        .store
        .insert_task(&task(task_id, "anime-upscale", 10, fixture.now))
        .await?;
    let scheduler = videnoa_controller::scheduler::Scheduler::load(fixture.store.clone()).await?;
    scheduler
        .reserve_next(fixture.now)
        .await?
        .ok_or_else(|| std::io::Error::other("assignment missing"))?;

    // When: deletion is attempted while active, after cancellation, and for an unused worker.
    let active = fixture
        .registry
        .delete(referenced.id, 1)
        .await
        .expect_err("active reference must conflict");
    let task = fixture
        .store
        .task(task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("task missing"))?;
    let attempt = fixture
        .store
        .current_attempt(task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("attempt missing"))?;
    videnoa_controller::lifecycle::LifecycleService::new(fixture.store.clone())
        .request_cancellation(&task, Some(&attempt), fixture.now)
        .await?;
    let historical = fixture
        .registry
        .delete(referenced.id, 1)
        .await
        .expect_err("historical reference must conflict");
    fixture.registry.delete(unused.id, unused.version).await?;

    // Then: references are typed conflicts and the unused worker is removed.
    assert_eq!(active.code(), WorkerRegistryErrorCode::Referenced);
    assert_eq!(historical.code(), WorkerRegistryErrorCode::Referenced);
    assert!(fixture.registry.worker(unused.id).await?.is_none());
    Ok(())
}

#[tokio::test]
async fn health_refresh_is_atomic_and_capacity_reduction_cannot_hide_usage() -> TestResult {
    // Given: a worker refreshed online with one compatible workflow and one assignment.
    let fixture = fixture().await?;
    let worker = fixture
        .registry
        .create(
            worker_request("worker-a", "https://worker.example/api/", 2)?,
            fixture.now,
        )
        .await?;
    online(&fixture, worker.id, worker.version, &["anime-upscale"]).await?;
    let stale_health = fixture
        .registry
        .refresh_health(videnoa_controller::persistence::WorkerHealthUpdate {
            id: worker.id,
            expected_version: worker.version,
            online: true,
            capabilities: super::support::capabilities(&["other"], fixture.now),
            last_seen_at: Some(fixture.now),
            health_retry_count: 0,
            next_health_check_at: None,
            last_error: None,
            updated_at: fixture.now,
        })
        .await;
    let task_id = task_id(301);
    fixture
        .store
        .insert_task(&task(task_id, "anime-upscale", 10, fixture.now))
        .await?;
    let scheduler = videnoa_controller::scheduler::Scheduler::load(fixture.store.clone()).await?;
    scheduler
        .reserve_next(fixture.now)
        .await?
        .ok_or_else(|| std::io::Error::other("assignment missing"))?;
    let current = fixture
        .registry
        .worker(worker.id)
        .await?
        .ok_or_else(|| std::io::Error::other("worker missing"))?;

    // When: capacity is reduced below durable usage.
    let reduction = fixture
        .registry
        .update(
            worker.id,
            WorkerUpdateRequest {
                version: current.version,
                name: current.name.clone(),
                api_url: current.api_url.clone(),
                enabled: current.enabled,
                compute_slots: ComputeSlots::try_from(1_u64)?,
            },
            fixture.now,
        )
        .await;

    // Then: stale health loses its CAS and compatible durable evidence remains authoritative.
    assert_eq!(
        stale_health
            .expect_err("stale health refresh must conflict")
            .code(),
        WorkerRegistryErrorCode::Conflict
    );
    assert!(reduction.is_ok());
    let capacity = fixture.registry.capacity(worker.id).await?;
    assert_eq!(capacity.used_slots, 0);
    assert_eq!(capacity.available_slots, 1);
    Ok(())
}

fn _registry_type_is_public(_: WorkerRegistry) {}
