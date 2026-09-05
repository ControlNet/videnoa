use axum::http::StatusCode;
use serde_json::json;
use tower::ServiceExt;
use videnoa_controller::domain::RemoteJobId;

use super::retry_support::{create_processing_failure, retry_job, retry_remote};
use super::support::{json_body, Fixture, TestResult};
use super::task_support::{
    create_api_task, create_online_retry_worker, install_post_update_corruption,
};

pub async fn create_and_cancel_task(fixture: &Fixture) -> TestResult {
    let task_id = create_api_task(fixture, "task-14-cancel").await?;
    let cancelled = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            &format!("/api/tasks/{task_id}/cancel"),
            Some(&json!({"version": 0})),
        )?)
        .await?;
    assert_eq!(cancelled.status(), StatusCode::OK);
    assert_eq!(json_body(cancelled).await?["status"], "cancelled");
    Ok(())
}

#[tokio::test]
async fn cancellation_response_does_not_depend_on_post_commit_reload() -> TestResult {
    // Given: a queued task whose row becomes unreadable only after an update commits.
    let fixture = Fixture::new().await?;
    let task_id = create_api_task(&fixture, "task-14-cancel-post-commit").await?;
    install_post_update_corruption(&fixture).await?;

    // When: cancellation commits successfully.
    let cancelled = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            &format!("/api/tasks/{task_id}/cancel"),
            Some(&json!({"version": 0})),
        )?)
        .await?;

    // Then: the response reports committed success without a fallible reload.
    assert_eq!(cancelled.status(), StatusCode::OK);
    assert_eq!(json_body(cancelled).await?["status"], "cancelled");
    Ok(())
}

#[tokio::test]
async fn processing_retry_response_does_not_depend_on_post_commit_reload() -> TestResult {
    // Given: a retryable processing failure whose row becomes unreadable after retry commits.
    let fixture = Fixture::new().await?;
    let remote_job_id = RemoteJobId::random();
    let remote = retry_remote(Ok(retry_job(remote_job_id))).await?;
    let worker_id = create_online_retry_worker(&fixture, remote.address).await?;
    let task_id = create_processing_failure(&fixture, worker_id, remote_job_id).await?;
    let failed = fixture.store.task(task_id).await?.ok_or("task missing")?;
    install_post_update_corruption(&fixture).await?;

    // When: the replacement attempt commits successfully.
    let retried = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            &format!("/api/tasks/{task_id}/retry"),
            Some(&json!({"version": failed.version})),
        )?)
        .await?;

    // Then: the response reports committed success without a fallible reload.
    assert_eq!(retried.status(), StatusCode::OK);
    assert_eq!(json_body(retried).await?["status"], "reserved");
    remote.server.abort();
    Ok(())
}
