//! Upgrades databases written by released Controllers to the current schema.
//!
//! Each fixture is a `sqlite3 .dump` of a database that the migrations of that
//! release tag created (`git show <tag>:crates/controller/migrations/...`, run by
//! `SQLx`), seeded with representative rows. The recorded migration checksums make
//! an edited released migration fail here, as it would for a real user.

use std::error::Error;
use std::num::NonZeroU32;
use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tempfile::TempDir;
use videnoa_controller::config::{ControllerConfig, HistoryConfig};
use videnoa_controller::domain::{
    FailureCode, PageRequest, TaskId, TaskListQuery, TaskStatus, WorkerId, WorkerName,
};
use videnoa_controller::paths::PathCapabilities;
use videnoa_controller::persistence::{Database, DatabaseOptions, Store, WorkerDeleteOutcome};
use videnoa_controller::tasks::HistoryRetention;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

const COMPLETED: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const FAILED_PROCESSING: &str = "aaaaaaaa-0000-4000-8000-000000000002";
const PROCESSING: &str = "aaaaaaaa-0000-4000-8000-000000000003";
const QUEUED: &str = "aaaaaaaa-0000-4000-8000-000000000004";
const CANCELLED: &str = "aaaaaaaa-0000-4000-8000-000000000005";
const OUTPUT_EXISTS: &str = "aaaaaaaa-0000-4000-8000-000000000006";
const PINNED: &str = "aaaaaaaa-0000-4000-8000-000000000007";
const FIXTURE_WORKER: &str = "11111111-1111-4111-8111-111111111111";
const RETIRED_WORKER: &str = "22222222-2222-4222-8222-222222222222";

async fn upgraded(release: &str) -> TestResult<(TempDir, Store)> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/released_databases")
        .join(format!("{release}.sql"));
    let dump = std::fs::read_to_string(fixture)?;
    let directory = TempDir::new()?;
    let path = directory.path().join("controller.sqlite3");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await?;
    sqlx::raw_sql(&dump).execute(&pool).await?;
    pool.close().await;
    let database = Database::open(DatabaseOptions::new(&path)).await?;
    Ok((directory, Store::new(database)))
}

fn id<T: std::str::FromStr>(value: &str) -> TestResult<T>
where
    T::Err: Error + Send + Sync + 'static,
{
    Ok(value.parse()?)
}

async fn assert_released_rows_survive(store: &Store, expected_tasks: u64) -> TestResult {
    let applied: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(store.database().pool())
            .await?;
    let shipped =
        std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations"))?.count();
    assert_eq!(usize::try_from(applied)?, shipped);

    let page = store.task_page(&TaskListQuery::default()).await?;
    assert_eq!(page.total, expected_tasks);
    for (task, status) in [
        (COMPLETED, TaskStatus::Completed),
        (FAILED_PROCESSING, TaskStatus::Failed),
        (PROCESSING, TaskStatus::Processing),
        (QUEUED, TaskStatus::Queued),
        (CANCELLED, TaskStatus::Cancelled),
        (OUTPUT_EXISTS, TaskStatus::Failed),
    ] {
        let record = store
            .task(id::<TaskId>(task)?)
            .await?
            .ok_or("released task is missing")?;
        assert_eq!(record.status, status, "{task}");
    }
    let failed = store
        .task(id::<TaskId>(OUTPUT_EXISTS)?)
        .await?
        .ok_or("released task is missing")?;
    let failure = failed.failure.ok_or("released failure is missing")?;
    assert_eq!(failure.failure_code, FailureCode::OutputExists);
    assert!(!failure.retryable);

    let processing = store
        .task(id::<TaskId>(PROCESSING)?)
        .await?
        .ok_or("released task is missing")?;
    assert_eq!(processing.worker_id, Some(id::<WorkerId>(FIXTURE_WORKER)?));
    assert!(processing.remote_job_id.is_some());
    let attempts = store
        .task_attempt_page(processing.id, PageRequest::default())
        .await?;
    assert_eq!(attempts.total, 1);
    assert_eq!(attempts.items[0].attempt.status, TaskStatus::Processing);

    let workers = store.workers().await?;
    assert_eq!(workers.len(), 2);
    Ok(())
}

#[tokio::test]
async fn database_written_by_v0_1_9_upgrades_to_the_current_schema() -> TestResult {
    let (_directory, store) = upgraded("v0.1.9").await?;

    assert_released_rows_survive(&store, 6).await?;
    let queued = store
        .task(id::<TaskId>(QUEUED)?)
        .await?
        .ok_or("released task is missing")?;
    assert_eq!(queued.requested_worker_id, None);
    Ok(())
}

#[tokio::test]
async fn database_written_by_v0_1_10_upgrades_to_the_current_schema() -> TestResult {
    let (_directory, store) = upgraded("v0.1.10").await?;

    assert_released_rows_survive(&store, 7).await?;
    let pinned = store
        .task(id::<TaskId>(PINNED)?)
        .await?
        .ok_or("released task is missing")?;
    assert_eq!(pinned.status, TaskStatus::Queued);
    assert_eq!(
        pinned.requested_worker_id,
        Some(id::<WorkerId>(RETIRED_WORKER)?)
    );
    Ok(())
}

/// Migration 0014 advises deleting the Worker a retry pinned a task to; a
/// released database must let that happen and unpin the task.
#[tokio::test]
async fn deleting_the_worker_a_released_retry_pinned_unpins_the_task() -> TestResult {
    let (_directory, store) = upgraded("v0.1.10").await?;
    let retired = id::<WorkerId>(RETIRED_WORKER)?;
    let version = store
        .worker(retired)
        .await?
        .ok_or("released worker is missing")?
        .version;

    let outcome = store.delete_worker(retired, version).await?;

    assert_eq!(outcome, WorkerDeleteOutcome::Deleted);
    let pinned = store
        .task(id::<TaskId>(PINNED)?)
        .await?
        .ok_or("released task is missing")?;
    assert_eq!(pinned.status, TaskStatus::Queued);
    assert_eq!(pinned.requested_worker_id, None);
    let attempt = store
        .current_attempt(pinned.id)
        .await?
        .ok_or("released attempt is missing")?;
    assert_eq!(attempt.attempt.worker_id, None);
    assert_eq!(
        attempt.attempt.worker_name.as_ref().map(WorkerName::as_str),
        Some("retired-worker")
    );
    // The Worker with an active task stays.
    let busy = id::<WorkerId>(FIXTURE_WORKER)?;
    let version = store.worker(busy).await?.ok_or("worker missing")?.version;
    assert_eq!(
        store.delete_worker(busy, version).await?,
        WorkerDeleteOutcome::Referenced
    );
    Ok(())
}

/// Released history is old enough to purge, so it shows what retention keeps.
#[tokio::test]
async fn released_history_is_purged_only_when_retention_is_set() -> TestResult {
    // Given: v0.1.10 history from 2023, with a temp folder the output_exists
    // failure left behind.
    let (directory, store) = upgraded("v0.1.10").await?;
    let config = ControllerConfig::for_workspace(directory.path())?;
    std::fs::create_dir_all(&config.paths.temp_root)?;
    let paths = PathCapabilities::open(&config.paths)?;
    let leftover = config.paths.temp_root.join(OUTPUT_EXISTS);
    std::fs::create_dir(&leftover)?;
    std::fs::write(leftover.join("output.mp4.verified"), b"enhanced-video")?;
    let now = chrono::Utc::now();

    // When: the sweep runs with retention off.
    let off = HistoryRetention::new(store.clone(), paths.clone(), HistoryConfig::default())
        .sweep(now)
        .await?;

    // Then: every task stays, but the expired batch receipt goes.
    assert_eq!((off.expired_batch_receipts, off.purged_tasks), (1, 0));
    assert_eq!(count(&store, "batch_idempotency").await?, 0);
    assert_eq!(count(&store, "tasks").await?, 7);

    // When: history is kept for 30 days.
    let thirty_days = HistoryConfig {
        retention_days: NonZeroU32::new(30),
    };
    let on = HistoryRetention::new(store.clone(), paths, thirty_days)
        .sweep(now)
        .await?;

    // Then: finished tasks go with their attempts, key and temp folder; a
    // failure Retry can still resume and unfinished tasks stay.
    assert_eq!(on.purged_tasks, 3);
    let kept: Vec<String> = sqlx::query_scalar("SELECT id FROM tasks ORDER BY id")
        .fetch_all(store.database().pool())
        .await?;
    assert_eq!(kept, [FAILED_PROCESSING, PROCESSING, QUEUED, PINNED]);
    let orphaned: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_attempts WHERE task_id NOT IN (SELECT id FROM tasks)",
    )
    .fetch_one(store.database().pool())
    .await?;
    assert_eq!(orphaned, 0);
    assert_eq!(count(&store, "task_idempotency").await?, 0);
    assert!(!leftover.exists());
    Ok(())
}

async fn count(store: &Store, table: &str) -> TestResult<i64> {
    Ok(sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(store.database().pool())
        .await?)
}
