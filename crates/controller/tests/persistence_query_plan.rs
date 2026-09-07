use std::error::Error;
use std::fmt::Write as _;

use serde_json::json;
use tempfile::TempDir;
use videnoa_controller::domain::TaskListQuery;
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

async fn seeded_store() -> TestResult<(TempDir, Store)> {
    let directory = TempDir::new()?;
    let database = Database::open(
        DatabaseOptions::new(directory.path().join("controller.sqlite3")).with_max_connections(4),
    )
    .await?;
    let store = Store::new(database);
    sqlx::query(
        "INSERT INTO workers (
            id, name, api_url, enabled, online, compute_slots, capabilities_json,
            created_at_ms, updated_at_ms
         ) VALUES (
            '00000000-0000-4000-8000-000000099999', 'seed-worker',
            'https://worker.example/', 1, 1, 8,
            '{\"workflows\":[],\"refreshed_at\":null}', 0, 0
         )",
    )
    .execute(store.database().pool())
    .await?;
    sqlx::query(
        "WITH RECURSIVE sequence(value) AS (
            SELECT 1 UNION ALL SELECT value + 1 FROM sequence WHERE value < 20000
         )
         INSERT INTO tasks (
            id, status, input_path, output_path, input_extension, output_extension,
            workflow, priority, source, source_reference, input_size, input_mtime_ms,
            worker_id, progress_json, failure_stage, failure_code, failure_message,
            failure_retryable, created_at_ms, updated_at_ms, completed_at_ms
         )
         SELECT
            printf('00000000-0000-4000-8000-%012d', value),
            CASE value % 4 WHEN 0 THEN 'queued' WHEN 1 THEN 'completed'
                WHEN 2 THEN 'failed' ELSE 'processing' END,
            printf('/input/season/episode-%05d.mkv', value),
            printf('/output/season/episode-%05d.mp4', value),
            'mkv', 'mp4',
            CASE value % 3 WHEN 0 THEN 'anime' WHEN 1 THEN 'interpolate' ELSE 'restore' END,
            value % 5,
            CASE value % 2 WHEN 0 THEN 'api' ELSE 'manual' END,
            printf('seed:%d', value), value * 1024, value,
            CASE WHEN value % 4 = 3 THEN '00000000-0000-4000-8000-000000099999' END,
            '{\"percent\":0.0,\"processed_frames\":null,\"total_frames\":null,\"frames_per_second\":null,\"eta_seconds\":null,\"bytes_transferred\":null,\"bytes_total\":null}',
            CASE WHEN value % 4 = 2 THEN 'processing' END,
            CASE WHEN value % 4 = 2 THEN 'processing_failed' END,
            CASE WHEN value % 4 = 2 THEN 'seed failure' END,
            CASE WHEN value % 4 = 2 THEN 1 END,
            1700000000000 + (value / 10),
            1700000000005 + (value / 10),
            CASE WHEN value % 4 = 1 THEN 1700000000100 + (value / 10) END
         FROM sequence",
    )
    .execute(store.database().pool())
    .await?;
    Ok((directory, store))
}

fn query(value: serde_json::Value) -> TestResult<TaskListQuery> {
    Ok(serde_json::from_value(value)?)
}

#[tokio::test]
async fn twenty_thousand_rows_use_planned_indexes_and_stable_bounded_pages() -> TestResult {
    // Given: 20,000 durable rows with repeated priorities, timestamps, filters, and sort values.
    let (_directory, store) = seeded_store().await?;
    let first_query = query(json!({"limit": 50, "offset": 0}))?;
    let second_query = query(json!({"limit": 50, "offset": 50}))?;

    // When: bounded pages, counts, filters, and representative query plans are requested.
    let first = store.task_page(&first_query).await?;
    let second = store.task_page(&second_query).await?;
    let failed_query = query(json!({
        "status": "failed", "workflow": "anime", "source": "api",
        "sort": "created_at", "direction": "asc", "limit": 100, "offset": 0
    }))?;
    let failed = store.task_page(&failed_query).await?;
    let expected_failed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE status = 'failed' AND workflow = 'anime' AND source = 'api'",
    )
    .fetch_one(store.database().pool())
    .await?;
    let plans = index_plans(&store).await?;

    // Then: SQL applies limits/counts/filters, ID tie-breaks pages, and every planned index is visible.
    assert_eq!(first.total, 20_000);
    assert_eq!(first.items.len(), 50);
    assert_eq!(second.items.len(), 50);
    assert_eq!(failed.total, u64::try_from(expected_failed)?);
    assert!(first
        .items
        .iter()
        .all(|item| !second.items.iter().any(|next| next.id == item.id)));
    assert!(first.items.windows(2).all(|pair| {
        pair[0].request.priority > pair[1].request.priority
            || (pair[0].request.priority == pair[1].request.priority
                && (pair[0].created_at, pair[0].id) <= (pair[1].created_at, pair[1].id))
    }));
    for index in [
        "idx_tasks_priority_sort",
        "idx_tasks_status_created",
        "idx_tasks_completed",
        "idx_tasks_worker",
        "idx_tasks_workflow",
        "idx_tasks_source",
        "idx_tasks_failure_stage",
        "idx_tasks_duration_sort",
        "idx_tasks_queue",
        "idx_tasks_recovery",
    ] {
        assert!(plans.contains(index), "missing index {index}\n{plans}");
    }
    if let Some(path) = std::env::var_os("VIDENOA_SCHEMA_EVIDENCE") {
        std::fs::write(
            path,
            schema_evidence(&store, &plans, first.total, failed.total).await?,
        )?;
    }
    Ok(())
}

async fn index_plans(store: &Store) -> TestResult<String> {
    let cases = [
        json!({}),
        json!({"status":"failed","sort":"created_at","direction":"asc"}),
        json!({"sort":"completed_at","direction":"desc"}),
        json!({"worker_id":"00000000-0000-4000-8000-000000099999","sort":"created_at"}),
        json!({"workflow":"anime","sort":"created_at"}),
        json!({"source":"api","sort":"created_at"}),
        json!({"failure_stage":"processing","sort":"created_at"}),
        json!({"sort":"duration","direction":"desc"}),
    ];
    let mut output = String::new();
    for value in cases {
        let request = query(value)?;
        for detail in store.explain_task_page(&request).await? {
            writeln!(output, "{detail}")?;
        }
    }
    for detail in store.explain_queue_plan().await? {
        writeln!(output, "{detail}")?;
    }
    for detail in store.explain_recovery_plan().await? {
        writeln!(output, "{detail}")?;
    }
    Ok(output)
}

async fn schema_evidence(
    store: &Store,
    plans: &str,
    total: u64,
    failed: u64,
) -> TestResult<String> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT type, name, sql FROM sqlite_schema
         WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
    )
    .fetch_all(store.database().pool())
    .await?;
    let mut output = format!("seeded_rows={total}\nfiltered_failed_rows={failed}\n\nSCHEMA\n");
    for (kind, name, sql) in rows {
        writeln!(output, "{kind} {name}\n{sql}\n")?;
    }
    writeln!(output, "QUERY PLANS\n{plans}")?;
    Ok(output)
}
