use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use tower::ServiceExt;
use videnoa_controller::domain::{
    AttemptId, RemoteJobId, RemotePath, SubmissionKey, TaskId, WorkerId,
};
use videnoa_controller::lifecycle::{
    AdvanceCommand, LifecycleFailure, LifecycleService, ReserveCommand, SubmissionEvidence,
    UploadEvidence,
};
use videnoa_controller::persistence::Store;

use super::support::{json_body, Fixture, TestResult};
use super::task_support::task_body;

pub struct RetryRemote {
    pub address: SocketAddr,
    pub job_deletes: Arc<AtomicUsize>,
    pub workspace_deletes: Arc<AtomicUsize>,
    pub server: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}

pub fn retry_job(remote_job_id: RemoteJobId) -> Value {
    json!({
        "id": remote_job_id,
        "status": "cancelled",
        "created_at": "2026-09-03T00:00:00Z",
        "started_at": "2026-09-03T00:00:01Z",
        "completed_at": "2026-09-03T00:00:02Z",
        "progress": null,
        "error": null,
        "workflow_name": "anime-upscale",
        "workflow_source": "test",
        "params": {"input": "task/input.mkv", "output": "task/output.mp4"},
        "rerun_of_job_id": null,
        "duration_ms": 1000
    })
}

pub async fn retry_remote(response: Result<Value, StatusCode>) -> TestResult<RetryRemote> {
    let job_deletes = Arc::new(AtomicUsize::new(0));
    let job_delete_count = Arc::clone(&job_deletes);
    let workspace_deletes = Arc::new(AtomicUsize::new(0));
    let workspace_delete_count = Arc::clone(&workspace_deletes);
    let app = Router::new()
        .route(
            "/api/jobs/{id}",
            get(move || {
                let response = response.clone();
                async move {
                    match response {
                        Ok(job) => Json(job).into_response(),
                        Err(status) => status.into_response(),
                    }
                }
            })
            .delete(move || async move {
                job_delete_count.fetch_add(1, Ordering::SeqCst);
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/api/files/{task_id}",
            axum::routing::delete(move || async move {
                workspace_delete_count.fetch_add(1, Ordering::SeqCst);
                StatusCode::NO_CONTENT
            }),
        );
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    Ok(RetryRemote {
        address,
        job_deletes,
        workspace_deletes,
        server,
    })
}

pub async fn create_processing_failure(
    fixture: &Fixture,
    worker_id: WorkerId,
    remote_job_id: RemoteJobId,
) -> TestResult<TaskId> {
    let task = task_body(fixture);
    let mut request = Fixture::request("POST", "/api/tasks", Some(&task))?;
    request
        .headers_mut()
        .insert("idempotency-key", "task-14-processing-retry".parse()?);
    let task_id = json_body(fixture.router.clone().oneshot(request).await?).await?["id"]
        .as_str()
        .ok_or("task id missing")?
        .parse()?;
    let service = LifecycleService::new(fixture.store.clone());
    let attempt_id = AttemptId::random();
    service
        .reserve(&ReserveCommand {
            task_id,
            expected_task_version: 0,
            worker_id,
            attempt_id,
            submission_key: SubmissionKey::random(),
            reserved_at: chrono::Utc::now(),
        })
        .await
        .map_err(|error| std::io::Error::other(format!("reserve failed: {error}")))?;
    for command in [
        AdvanceCommand::StartUpload,
        AdvanceCommand::FinishUpload(UploadEvidence {
            remote_input_path: RemotePath::new("task/input.mkv"),
            remote_output_path: RemotePath::new("task/output.mp4"),
        }),
        AdvanceCommand::StartSubmission,
        AdvanceCommand::PersistSubmission(SubmissionEvidence {
            remote_job_id,
            remote_input_path: RemotePath::new("task/input.mkv"),
            remote_output_path: RemotePath::new("task/output.mp4"),
        }),
    ] {
        advance(&fixture.store, &service, task_id, attempt_id, command).await?;
    }
    let task = fixture.store.task(task_id).await?.ok_or("task missing")?;
    let attempt = fixture
        .store
        .attempt(attempt_id)
        .await?
        .ok_or("attempt missing")?;
    service
        .fail(
            &task,
            Some(&attempt),
            LifecycleFailure::restart_cancelled("worker restarted"),
            chrono::Utc::now(),
        )
        .await
        .map_err(|error| std::io::Error::other(format!("processing failure failed: {error}")))?;
    Ok(task_id)
}

async fn advance(
    store: &Store,
    service: &LifecycleService,
    task_id: TaskId,
    attempt_id: AttemptId,
    command: AdvanceCommand,
) -> TestResult {
    let task = store.task(task_id).await?.ok_or("task missing")?;
    let attempt = store.attempt(attempt_id).await?.ok_or("attempt missing")?;
    service
        .advance(&task, &attempt, command.clone(), chrono::Utc::now())
        .await
        .map_err(|error| std::io::Error::other(format!("advance {command:?} failed: {error}")))?;
    Ok(())
}
