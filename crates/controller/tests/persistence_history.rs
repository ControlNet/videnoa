use std::error::Error;

use serde_json::json;
use tempfile::TempDir;
use videnoa_controller::domain::TaskListQuery;
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

async fn store() -> TestResult<(TempDir, Store)> {
    let directory = TempDir::new()?;
    let database = Database::open(DatabaseOptions::new(
        directory.path().join("controller.sqlite3"),
    ))
    .await?;
    Ok((directory, Store::new(database)))
}

async fn insert_task(
    store: &Store,
    id: &str,
    input: &str,
    output: &str,
    created: i64,
    updated: i64,
    completed: Option<i64>,
) -> TestResult {
    sqlx::query(
        "INSERT INTO tasks (
            id, status, input_path, output_path, input_extension, output_extension,
            workflow, priority, source, input_size, input_mtime_ms, progress_json,
            created_at_ms, updated_at_ms, completed_at_ms
         ) VALUES (?, ?, ?, ?, 'mkv', 'mp4', 'anime', 0, 'api', 1, 1,
            '{\"percent\":0}', ?, ?, ?)",
    )
    .bind(id)
    .bind(if completed.is_some() {
        "completed"
    } else {
        "processing"
    })
    .bind(input)
    .bind(output)
    .bind(created)
    .bind(updated)
    .bind(completed)
    .execute(store.database().pool())
    .await?;
    Ok(())
}

fn query(value: serde_json::Value) -> TestResult<TaskListQuery> {
    Ok(serde_json::from_value(value)?)
}

#[tokio::test]
async fn nullable_history_sorts_place_unknown_values_last() -> TestResult {
    // Given: one completed task and one active task with a shorter current elapsed time.
    let (_directory, store) = store().await?;
    insert_task(
        &store,
        "00000000-0000-4000-8000-000000000001",
        "/input/completed.mkv",
        "/output/completed.mp4",
        100,
        200,
        Some(200),
    )
    .await?;
    insert_task(
        &store,
        "00000000-0000-4000-8000-000000000002",
        "/input/active.mkv",
        "/output/active.mp4",
        300,
        301,
        None,
    )
    .await?;

    // When: nullable completion and duration fields are sorted ascending.
    for sort in ["completed_at", "duration"] {
        let page = store
            .task_page(&query(json!({"sort": sort, "direction": "asc"}))?)
            .await?;

        // Then: known values precede nulls and the active task is always last.
        assert_eq!(page.items.len(), 2);
        assert_eq!(
            page.items[1].id.to_string(),
            "00000000-0000-4000-8000-000000000002"
        );
    }
    Ok(())
}

#[tokio::test]
async fn path_search_is_case_insensitive_and_escapes_wildcards() -> TestResult {
    // Given: paths containing mixed case and literal SQL wildcard characters.
    let (_directory, store) = store().await?;
    insert_task(
        &store,
        "00000000-0000-4000-8000-000000000003",
        "/input/Season_100%/Episode.MKV",
        "/output/Season_100%/Episode.MP4",
        100,
        100,
        Some(100),
    )
    .await?;

    // When: basename/path terms differ in case and contain literal wildcard characters.
    let case_page = store
        .task_page(&query(json!({"search": "episode.mkv"}))?)
        .await?;
    let wildcard_page = store
        .task_page(&query(json!({"search": "Season_100%"}))?)
        .await?;

    // Then: both searches match exactly one stored path without wildcard expansion.
    assert_eq!(case_page.total, 1);
    assert_eq!(wildcard_page.total, 1);
    Ok(())
}

#[tokio::test]
async fn large_history_pages_are_bounded_stable_and_indexed() -> TestResult {
    let (_directory, store) = store().await?;
    sqlx::query(
        "WITH RECURSIVE sequence(value) AS (
            SELECT 1 UNION ALL SELECT value + 1 FROM sequence WHERE value < 20000
         )
         INSERT INTO tasks (
            id, status, input_path, output_path, input_extension, output_extension,
            workflow, priority, source, input_size, input_mtime_ms, progress_json,
            created_at_ms, updated_at_ms
         )
         SELECT printf('00000000-0000-4000-8000-%012x', value), 'queued',
            printf('/input/%05d.mkv', value), printf('/output/%05d.mp4', value),
            'mkv', 'mp4', 'anime', value % 10, 'api', 1, 1, '{\"percent\":0}',
            value, value
         FROM sequence",
    )
    .execute(store.database().pool())
    .await?;

    let query = query(json!({
        "status": "queued",
        "sort": "priority",
        "direction": "desc",
        "limit": 100,
        "offset": 100
    }))?;
    let first = store.task_page(&query).await?;
    let second = store.task_page(&query).await?;
    assert_eq!(first.total, 20_000);
    assert_eq!(first.items.len(), 100);
    assert_eq!(first.items, second.items);

    let plan = store.explain_task_page(&query).await?.join("\n");
    assert!(plan.contains("idx_tasks_status_created") || plan.contains("idx_tasks_queue"));
    Ok(())
}
