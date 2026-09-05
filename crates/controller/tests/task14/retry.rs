use std::sync::atomic::Ordering;

use axum::http::StatusCode;
use serde_json::json;
use tower::ServiceExt;
use videnoa_controller::domain::RemoteJobId;

use super::retry_support::{create_processing_failure, retry_job, retry_remote};
use super::support::{json_body, Fixture, TestResult};
use super::task_support::create_online_retry_worker;

#[tokio::test]
async fn processing_retry_verifies_terminal_remote_cleanup() -> TestResult {
    let fixture = Fixture::new().await?;
    let remote_job_id = RemoteJobId::random();
    let remote = retry_remote(Ok(retry_job(remote_job_id))).await?;
    let worker_id = create_online_retry_worker(&fixture, remote.address).await?;
    let task_id = create_processing_failure(&fixture, worker_id, remote_job_id).await?;
    let failed = fixture.store.task(task_id).await?.ok_or("task missing")?;
    let old_attempt = fixture
        .store
        .current_attempt(task_id)
        .await?
        .ok_or("attempt missing")?;
    let retried = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            &format!("/api/tasks/{task_id}/retry"),
            Some(&json!({"version": failed.version})),
        )?)
        .await?;
    assert_eq!(retried.status(), StatusCode::OK);
    let response = json_body(retried).await?;
    assert_eq!(response["status"], "reserved");
    let new_attempt = fixture
        .store
        .current_attempt(task_id)
        .await?
        .ok_or("new attempt missing")?;
    assert_ne!(new_attempt.attempt.id, old_attempt.attempt.id);
    assert_eq!(response["attempt_id"], new_attempt.attempt.id.to_string());
    assert_eq!(fixture.store.task_attempts(task_id, 10).await?.len(), 2);
    assert_eq!(remote.workspace_deletes.load(Ordering::SeqCst), 1);
    assert_eq!(remote.job_deletes.load(Ordering::SeqCst), 0);
    remote.server.abort();
    Ok(())
}
