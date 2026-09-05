use axum::http::StatusCode;
use serde_json::json;
use tower::ServiceExt;

use super::support::{json_body, Fixture, TestResult};
use super::task_support::task_body;

#[tokio::test]
async fn status_counts_materialize_every_status_for_empty_database() -> TestResult {
    // Given: a fresh Controller database with no tasks.
    let fixture = Fixture::new().await?;

    // When: the aggregate status endpoint is requested.
    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/status-counts", None)?)
        .await?;

    // Then: every status is present in lifecycle order with a zero count.
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await?,
        json!({
            "items": [
                {"status": "queued", "count": 0},
                {"status": "reserved", "count": 0},
                {"status": "uploading", "count": 0},
                {"status": "staged", "count": 0},
                {"status": "submitting", "count": 0},
                {"status": "processing", "count": 0},
                {"status": "remote_completed", "count": 0},
                {"status": "downloading", "count": 0},
                {"status": "verifying", "count": 0},
                {"status": "publishing", "count": 0},
                {"status": "remote_cleanup", "count": 0},
                {"status": "completed", "count": 0},
                {"status": "failed", "count": 0},
                {"status": "cancelled", "count": 0}
            ],
            "total": 0
        })
    );
    Ok(())
}

#[tokio::test]
async fn status_counts_zero_fill_partially_populated_database() -> TestResult {
    // Given: one queued task and one task represented as processing.
    let fixture = Fixture::new().await?;
    for key in ["task-14-count-queued", "task-14-count-processing"] {
        let task = task_body(&fixture);
        let mut request = Fixture::request("POST", "/api/tasks", Some(&task))?;
        request
            .headers_mut()
            .insert("idempotency-key", key.parse()?);
        assert_eq!(
            fixture.router.clone().oneshot(request).await?.status(),
            StatusCode::CREATED
        );
    }
    sqlx::query("UPDATE tasks SET status = 'processing' WHERE id = (SELECT id FROM tasks ORDER BY id LIMIT 1)")
        .execute(fixture.store.database().pool())
        .await?;

    // When: the aggregate status endpoint is requested.
    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/status-counts", None)?)
        .await?;

    // Then: sparse persisted rows are projected onto every deterministic category.
    assert_eq!(response.status(), StatusCode::OK);
    let counts = json_body(response).await?;
    assert_eq!(counts["items"].as_array().map(Vec::len), Some(14));
    assert_eq!(counts["total"], 2);
    assert_eq!(counts["items"][0], json!({"status": "queued", "count": 1}));
    assert_eq!(
        counts["items"][5],
        json!({"status": "processing", "count": 1})
    );
    assert!(counts["items"].as_array().is_some_and(|items| items
        .iter()
        .enumerate()
        .all(|(index, item)| index == 0 || index == 5 || item["count"] == 0)));
    Ok(())
}
