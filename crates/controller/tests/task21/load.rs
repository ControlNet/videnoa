use std::sync::mpsc;

use axum::body::to_bytes;
use serde_json::Value;
use tempfile::TempDir;
use tokio::sync::oneshot;
use tower::ServiceExt;
use videnoa_controller::domain::{PageRequest, TaskId};
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};

use super::support::{fixture, request, TestResult};

const SEEDED_ROWS: u64 = 20_000;
const MAXIMUM_PAGE_BYTES: usize = 512 * 1024;
const MAXIMUM_DETAIL_BYTES: usize = 1024 * 1024;
const MAXIMUM_DETAIL_ATTEMPTS: usize = 500;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attempt_page_count_and_items_share_one_sqlite_snapshot() -> TestResult {
    // Given: a page reader paused after establishing its count snapshot and a separate WAL writer.
    let directory = TempDir::new()?;
    let path = directory.path().join("controller.sqlite3");
    let database = Database::open(DatabaseOptions::new(&path).with_max_connections(1)).await?;
    let writer = Database::open(DatabaseOptions::new(&path).with_max_connections(1)).await?;
    let store = Store::new(database);
    let task_id: TaskId = "00000000-0000-4000-8000-000000000001".parse()?;
    seed_snapshot_attempts(&store).await?;
    let (count_started_sender, count_started) = oneshot::channel();
    let (count_release, count_release_receiver) = mpsc::channel();
    let mut count_started_sender = Some(count_started_sender);
    let mut connection = store.database().pool().acquire().await?;
    {
        let mut handle = connection.lock_handle().await?;
        handle.set_progress_handler(100, move || {
            if let Some(sender) = count_started_sender.take() {
                let _ = sender.send(());
                let _ = count_release_receiver.recv();
            }
            true
        });
    }
    drop(connection);
    let reader = store.clone();
    let page_task = tokio::spawn(async move {
        let page = reader
            .task_attempt_page(task_id, PageRequest::try_new(Some(1), 100)?)
            .await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(page)
    });

    // When: attempt 101 commits while the count statement is reading the original 100 rows.
    count_started.await?;
    insert_snapshot_attempt(writer.pool(), 101).await?;
    count_release.send(())?;
    let page = page_task.await??;

    // Then: total and items describe the same pre-insert snapshot.
    assert_eq!(page.total, 100);
    assert!(page.items.is_empty());
    Ok(())
}

#[tokio::test]
async fn history_load_keeps_task_and_attempt_responses_bounded() -> TestResult {
    // Given: 20,000 tasks and 20,000 attempts attached to one long-lived task.
    let fixture = fixture().await?;
    seed_history(&fixture.store).await?;

    // When: stable filtered pages and the adversarial task detail are loaded twice.
    let first = response(
        &fixture.router,
        "/api/tasks?status=completed&sort=created_at&direction=asc&limit=500&offset=500",
    )
    .await?;
    let second = response(
        &fixture.router,
        "/api/tasks?status=completed&sort=created_at&direction=asc&limit=500&offset=500",
    )
    .await?;
    let detail = response(
        &fixture.router,
        "/api/tasks/00000000-0000-4000-8000-000000000001",
    )
    .await?;
    let attempt_page = response(
        &fixture.router,
        "/api/tasks/00000000-0000-4000-8000-000000000001?limit=500&offset=500",
    )
    .await?;
    let page: Value = serde_json::from_slice(&first)?;
    let detail_value = serde_json::from_slice::<Value>(&detail)?;
    let attempts = detail_value["attempts"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("task detail attempts missing"))?
        .len();
    let attempt_page_value = serde_json::from_slice::<Value>(&attempt_page)?;
    let paged_attempts = attempt_page_value["attempts"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("task detail attempt page missing"))?;

    // Then: pages are stable and every serialized response remains within one API page bound.
    assert_eq!(page["total"], SEEDED_ROWS);
    assert_eq!(page["items"].as_array().map(Vec::len), Some(500));
    assert_eq!(first, second);
    assert_eq!(detail_value["total"], SEEDED_ROWS);
    assert_eq!(detail_value["limit"], 100);
    assert_eq!(detail_value["offset"], 0);
    assert_eq!(detail_value["attempts"][0]["attempt_number"], SEEDED_ROWS);
    assert_eq!(detail_value["attempts"][99]["attempt_number"], 19_901);
    assert_eq!(attempt_page_value["total"], SEEDED_ROWS);
    assert_eq!(attempt_page_value["limit"], 500);
    assert_eq!(attempt_page_value["offset"], 500);
    assert_eq!(paged_attempts.len(), 500);
    assert_eq!(paged_attempts[0]["attempt_number"], 19_500);
    assert_eq!(paged_attempts[499]["attempt_number"], 19_001);
    assert!(
        first.len() <= MAXIMUM_PAGE_BYTES,
        "page_bytes={}",
        first.len()
    );
    eprintln!(
        "task21_load seeded_tasks={SEEDED_ROWS} seeded_attempts={SEEDED_ROWS} page_bytes={} detail_bytes={} detail_attempts={attempts} attempt_page_bytes={}",
        first.len(),
        detail.len(),
        attempt_page.len()
    );
    assert!(
        attempts <= MAXIMUM_DETAIL_ATTEMPTS,
        "detail_attempts={attempts} exceeds {MAXIMUM_DETAIL_ATTEMPTS}"
    );
    assert!(
        detail.len() <= MAXIMUM_DETAIL_BYTES,
        "detail_bytes={} exceeds {MAXIMUM_DETAIL_BYTES}",
        detail.len()
    );
    assert!(
        attempt_page.len() <= MAXIMUM_DETAIL_BYTES,
        "attempt_page_bytes={} exceeds {MAXIMUM_DETAIL_BYTES}",
        attempt_page.len()
    );
    Ok(())
}

async fn response(router: &axum::Router, uri: &str) -> TestResult<Vec<u8>> {
    let response = router.clone().oneshot(request(uri)?).await?;
    if !response.status().is_success() {
        return Err(
            std::io::Error::other(format!("request {uri} returned {}", response.status())).into(),
        );
    }
    Ok(to_bytes(response.into_body(), 32 * 1024 * 1024)
        .await?
        .to_vec())
}

async fn seed_history(store: &videnoa_controller::persistence::Store) -> TestResult {
    sqlx::query(
        "WITH RECURSIVE sequence(value) AS (
            SELECT 1 UNION ALL SELECT value + 1 FROM sequence WHERE value < 20000
         )
         INSERT INTO tasks (
            id, status, input_path, output_path, input_extension, output_extension,
            workflow, priority, source, input_size, input_mtime_ms, progress_json,
            attempt_count, created_at_ms, updated_at_ms, completed_at_ms
         )
         SELECT printf('00000000-0000-4000-8000-%012d', value), 'completed',
            printf('/input/%05d.mkv', value), printf('/output/%05d.mp4', value),
            'mkv', 'mp4', CASE value % 3 WHEN 0 THEN 'anime' WHEN 1 THEN 'restore' ELSE 'interpolate' END,
            value % 11, CASE value % 2 WHEN 0 THEN 'api' ELSE 'manual' END,
            value * 1024, value, '{\"percent\":100}',
            CASE value WHEN 1 THEN 20000 ELSE 0 END,
            1700000000000 + value, 1700000000100 + value, 1700000000100 + value
         FROM sequence",
    )
    .execute(store.database().pool())
    .await?;
    sqlx::query(
        "WITH RECURSIVE sequence(value) AS (
            SELECT 1 UNION ALL SELECT value + 1 FROM sequence WHERE value < 20000
         )
         INSERT INTO task_attempts (
            id, task_id, attempt_no, status, submission_key, progress_json,
            created_at_ms, updated_at_ms, started_at_ms, completed_at_ms
         )
         SELECT printf('10000000-0000-4000-8000-%012d', value),
            '00000000-0000-4000-8000-000000000001', value, 'completed',
            printf('20000000-0000-4000-8000-%012d', value), '{\"percent\":100}',
            1700000000000 + value, 1700000000100 + value,
            1700000000000 + value, 1700000000100 + value
         FROM sequence",
    )
    .execute(store.database().pool())
    .await?;
    Ok(())
}

async fn seed_snapshot_attempts(store: &Store) -> TestResult {
    sqlx::query(
        "INSERT INTO tasks (
            id, status, input_path, output_path, input_extension, output_extension,
            workflow, priority, source, input_size, input_mtime_ms, progress_json,
            attempt_count, created_at_ms, updated_at_ms
         ) VALUES (?, 'processing', '/input/snapshot.mkv', '/output/snapshot.mp4',
            'mkv', 'mp4', 'anime', 0, 'api', 1, 1, '{\"percent\":0}', 100, 1, 1)",
    )
    .bind("00000000-0000-4000-8000-000000000001")
    .execute(store.database().pool())
    .await?;
    sqlx::query(
        "WITH RECURSIVE sequence(value) AS (
            SELECT 1 UNION ALL SELECT value + 1 FROM sequence WHERE value < 100
         )
         INSERT INTO task_attempts (
            id, task_id, attempt_no, status, submission_key, progress_json,
            created_at_ms, updated_at_ms
         )
         SELECT printf('10000000-0000-4000-8000-%012d', value),
            '00000000-0000-4000-8000-000000000001', value, 'processing',
            printf('20000000-0000-4000-8000-%012d', value), '{\"percent\":0}', value, value
         FROM sequence",
    )
    .execute(store.database().pool())
    .await?;
    Ok(())
}

async fn insert_snapshot_attempt(pool: &sqlx::SqlitePool, attempt_no: i64) -> TestResult {
    sqlx::query(
        "INSERT INTO task_attempts (
            id, task_id, attempt_no, status, submission_key, progress_json,
            created_at_ms, updated_at_ms
         ) VALUES ('10000000-0000-4000-8000-000000000101',
            '00000000-0000-4000-8000-000000000001', ?, 'processing',
            '20000000-0000-4000-8000-000000000101', '{\"percent\":0}', ?, ?)",
    )
    .bind(attempt_no)
    .bind(attempt_no)
    .bind(attempt_no)
    .execute(pool)
    .await?;
    Ok(())
}
