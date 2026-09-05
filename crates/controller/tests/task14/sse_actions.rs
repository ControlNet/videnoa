use std::time::Duration;

use axum::http::StatusCode;
use futures_util::StreamExt;
use serde_json::json;
use tower::ServiceExt;
use videnoa_controller::domain::RemoteJobId;

use super::retry_support::{create_processing_failure, retry_job, retry_remote};
use super::support::{Fixture, TestResult};
use super::task_support::{create_api_task, create_online_retry_worker};

#[tokio::test]
async fn cancellation_publishes_exactly_one_task_delta() -> TestResult {
    // Given: a queued task and an SSE subscriber caught up to current state.
    let fixture = Fixture::new().await?;
    let task_id = create_api_task(&fixture, "task-14-cancel-event").await?;
    let events = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/events", None)?)
        .await?;
    let mut stream = events.into_body().into_data_stream();
    let _initial = stream.next().await.ok_or("missing initial refetch")??;

    // When: cancellation is committed through the HTTP API.
    let cancelled = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            &format!("/api/tasks/{task_id}/cancel"),
            Some(&json!({"version": 0})),
        )?)
        .await?;

    // Then: the subscriber receives one task delta and no duplicate publication.
    assert_eq!(cancelled.status(), StatusCode::OK);
    let event = stream.next().await.ok_or("missing cancellation delta")??;
    assert!(String::from_utf8_lossy(&event).contains("event: task_updated"));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), stream.next())
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn processing_retry_publishes_exactly_one_task_delta() -> TestResult {
    // Given: a retryable processing failure and an SSE subscriber caught up to current state.
    let fixture = Fixture::new().await?;
    let remote_job_id = RemoteJobId::random();
    let remote = retry_remote(Ok(retry_job(remote_job_id))).await?;
    let worker_id = create_online_retry_worker(&fixture, remote.address).await?;
    let task_id = create_processing_failure(&fixture, worker_id, remote_job_id).await?;
    let failed = fixture.store.task(task_id).await?.ok_or("task missing")?;
    let events = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/events", None)?)
        .await?;
    let mut stream = events.into_body().into_data_stream();
    let _initial = stream.next().await.ok_or("missing initial refetch")??;

    // When: a replacement processing attempt is committed through the HTTP API.
    let retried = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            &format!("/api/tasks/{task_id}/retry"),
            Some(&json!({"version": failed.version})),
        )?)
        .await?;

    // Then: the subscriber receives one task delta and no duplicate publication.
    assert_eq!(retried.status(), StatusCode::OK);
    let event = stream.next().await.ok_or("missing retry delta")??;
    assert!(String::from_utf8_lossy(&event).contains("event: task_updated"));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), stream.next())
            .await
            .is_err()
    );
    remote.server.abort();
    Ok(())
}
