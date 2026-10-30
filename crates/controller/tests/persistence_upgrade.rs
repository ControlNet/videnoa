//! Upgrades databases written by released Controllers to the current schema.
//!
//! Each fixture is a `sqlite3 .dump` of a database that the migrations of that
//! release tag created (`git show <tag>:crates/controller/migrations/...`, run by
//! SQLx), seeded with representative rows. The recorded migration checksums make
//! an edited released migration fail here, as it would for a real user.

use std::error::Error;
use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tempfile::TempDir;
use videnoa_controller::domain::{
    FailureCode, PageRequest, TaskId, TaskListQuery, TaskStatus, WorkerId,
};
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};

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
