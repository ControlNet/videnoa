use std::sync::atomic::Ordering;

use axum::http::StatusCode;
use serde_json::{json, Value};
use tower::ServiceExt;
use videnoa_controller::domain::{
    AttemptId, FailureCode, FailureStage, RemoteJobId, RemotePath, SubmissionKey, TaskId,
    TaskStatus, WorkerId,
};
use videnoa_controller::lifecycle::{
    AdvanceCommand, LifecycleFailure, LifecycleService, ReserveCommand, UploadEvidence,
};

use super::retry_support::{create_processing_failure, retry_job, retry_remote};
use super::support::{json_body, Fixture, TestResult};
use super::task_support::{create_online_retry_worker, create_online_worker};

#[tokio::test]
async fn processing_retry_queues_when_the_worker_has_no_free_slot() -> TestResult {
    // Given: a processing failure on a Worker whose slot and prefetch are taken.
    let fixture = Fixture::new().await?;
    let remote_job_id = RemoteJobId::random();
    let remote = retry_remote(Ok(retry_job(remote_job_id))).await?;
    let worker_id = create_online_retry_worker(&fixture, remote.address).await?;
    let task_id = create_processing_failure(&fixture, worker_id, remote_job_id).await?;
    for name in ["busy-a.mp4", "busy-b.mp4"] {
        reserve_other_task(&fixture, worker_id, name).await?;
    }
    assert!(fixture.store.scheduler_candidate().await?.is_none());

    // When: the failed stage is retried.
    let (status, body) = retry(&fixture, task_id, None).await?;

    // Then: the task waits in the queue for the same Worker.
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "queued");
    assert_eq!(body["attempt_id"], Value::Null);
    let task = fixture.store.task(task_id).await?.ok_or("task missing")?;
    assert_eq!(task.status, TaskStatus::Queued);
    assert_eq!(task.worker_id, None);
    assert_eq!(task.requested_worker_id, Some(worker_id));
    assert!(task.failure.is_none());
    assert_eq!(fixture.store.task_attempts(task_id, 10).await?.len(), 1);
    assert_eq!(remote.workspace_deletes.load(Ordering::SeqCst), 1);
    assert_eq!(
        task_json(&fixture, task_id).await?["requested_worker_id"],
        worker_id.to_string()
    );

    // A free Worker does not take a task requested for another one.
    let idle = retry_remote(Ok(retry_job(RemoteJobId::random()))).await?;
    create_online_worker(&fixture, idle.address, "idle-worker", &["anime-upscale"]).await?;
    assert!(fixture.store.scheduler_candidate().await?.is_none());
    remote.server.abort();
    idle.server.abort();
    Ok(())
}

#[tokio::test]
async fn processing_retry_on_another_worker_cleans_the_original_first() -> TestResult {
    // Given: a processing failure on one Worker and a second capable Worker.
    let fixture = Fixture::new().await?;
    let remote_job_id = RemoteJobId::random();
    let original = retry_remote(Ok(retry_job(remote_job_id))).await?;
    let original_id = create_online_retry_worker(&fixture, original.address).await?;
    let task_id = create_processing_failure(&fixture, original_id, remote_job_id).await?;
    let other = retry_remote(Ok(retry_job(RemoteJobId::random()))).await?;
    let other_id =
        create_online_worker(&fixture, other.address, "other-worker", &["anime-upscale"]).await?;

    // When: the retry asks for the second Worker.
    let (status, body) = retry(&fixture, task_id, Some(other_id)).await?;

    // Then: the original workspace is deleted and only the chosen Worker is eligible.
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "queued");
    assert_eq!(original.workspace_deletes.load(Ordering::SeqCst), 1);
    assert_eq!(other.workspace_deletes.load(Ordering::SeqCst), 0);
    let task = fixture.store.task(task_id).await?.ok_or("task missing")?;
    assert_eq!(task.requested_worker_id, Some(other_id));
    let candidate = fixture
        .store
        .scheduler_candidate()
        .await?
        .ok_or("candidate missing")?;
    assert_eq!(candidate.task_id, task_id);
    assert_eq!(candidate.worker_id, other_id);
    original.server.abort();
    other.server.abort();
    Ok(())
}

#[tokio::test]
async fn retry_rejects_a_worker_that_cannot_run_the_workflow() -> TestResult {
    // Given: a processing failure and a Worker without the task's workflow.
    let fixture = Fixture::new().await?;
    let remote_job_id = RemoteJobId::random();
    let remote = retry_remote(Ok(retry_job(remote_job_id))).await?;
    let worker_id = create_online_retry_worker(&fixture, remote.address).await?;
    let task_id = create_processing_failure(&fixture, worker_id, remote_job_id).await?;
    let other = retry_remote(Ok(retry_job(RemoteJobId::random()))).await?;
    let unable = create_online_worker(&fixture, other.address, "unable", &["other"]).await?;

    // When / Then: that Worker and an unknown one are field errors.
    for target in [unable, WorkerId::random()] {
        let (status, body) = retry(&fixture, task_id, Some(target)).await?;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"]["field_errors"][0]["field"], "worker_id");
    }
    // Nothing was cleaned and the task is still failed.
    assert_eq!(remote.workspace_deletes.load(Ordering::SeqCst), 0);
    let task = fixture.store.task(task_id).await?.ok_or("task missing")?;
    assert_eq!(task.status, TaskStatus::Failed);
    remote.server.abort();
    other.server.abort();
    Ok(())
}

#[tokio::test]
async fn requeued_task_can_be_cancelled() -> TestResult {
    // Given: a retried task waiting in the queue next to its failed attempt.
    let fixture = Fixture::new().await?;
    let remote_job_id = RemoteJobId::random();
    let remote = retry_remote(Ok(retry_job(remote_job_id))).await?;
    let worker_id = create_online_retry_worker(&fixture, remote.address).await?;
    let task_id = create_processing_failure(&fixture, worker_id, remote_job_id).await?;
    let (status, body) = retry(&fixture, task_id, None).await?;
    assert_eq!(status, StatusCode::OK, "{body}");
    let queued = fixture.store.task(task_id).await?.ok_or("task missing")?;

    // When: it is cancelled.
    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            &format!("/api/tasks/{task_id}/cancel"),
            Some(&json!({"version": queued.version})),
        )?)
        .await?;

    // Then: it is cancelled without touching the earlier attempt.
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await?["status"], "cancelled");
    let attempts = fixture.store.task_attempts(task_id, 10).await?;
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].attempt.status, TaskStatus::Failed);
    remote.server.abort();
    Ok(())
}

#[tokio::test]
async fn rejected_submission_moves_to_another_worker_after_cleanup() -> TestResult {
    // Given: a Worker rejected the submission of an uploaded input.
    let fixture = Fixture::new().await?;
    let original = retry_remote(Ok(retry_job(RemoteJobId::random()))).await?;
    let original_id = create_online_retry_worker(&fixture, original.address).await?;
    let task_id = create_submission_rejection(&fixture, original_id).await?;
    let other = retry_remote(Ok(retry_job(RemoteJobId::random()))).await?;
    let other_id =
        create_online_worker(&fixture, other.address, "other-worker", &["anime-upscale"]).await?;

    // When: the retry asks for another Worker.
    let (status, body) = retry(&fixture, task_id, Some(other_id)).await?;

    // Then: the uploaded input is deleted from the original and the task queues.
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "queued");
    assert_eq!(original.workspace_deletes.load(Ordering::SeqCst), 1);
    let task = fixture.store.task(task_id).await?.ok_or("task missing")?;
    assert_eq!(task.requested_worker_id, Some(other_id));
    assert_eq!(fixture.store.task_attempts(task_id, 10).await?.len(), 1);
    original.server.abort();
    other.server.abort();
    Ok(())
}

async fn retry(
    fixture: &Fixture,
    task_id: TaskId,
    worker_id: Option<WorkerId>,
) -> TestResult<(StatusCode, Value)> {
    let task = fixture.store.task(task_id).await?.ok_or("task missing")?;
    let mut body = json!({"version": task.version});
    if let Some(worker_id) = worker_id {
        body["worker_id"] = json!(worker_id);
    }
    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            &format!("/api/tasks/{task_id}/retry"),
            Some(&body),
        )?)
        .await?;
    let status = response.status();
    Ok((status, json_body(response).await?))
}

async fn task_json(fixture: &Fixture, task_id: TaskId) -> TestResult<Value> {
    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "GET",
            &format!("/api/tasks/{task_id}"),
            None,
        )?)
        .await?;
    Ok(json_body(response).await?["task"].clone())
}

/// Creates another task and reserves it on `worker_id` to occupy capacity.
async fn reserve_other_task(fixture: &Fixture, worker_id: WorkerId, output: &str) -> TestResult {
    let body = json!({
        "input_path": fixture.input,
        "output_path": fixture.output.with_file_name(output),
        "workflow": "anime-upscale",
        "priority": 0,
        "source": "api",
        "source_reference": null
    });
    let mut request = Fixture::request("POST", "/api/tasks", Some(&body))?;
    request
        .headers_mut()
        .insert("idempotency-key", output.parse()?);
    let created = fixture.router.clone().oneshot(request).await?;
    assert_eq!(created.status(), StatusCode::CREATED);
    let task_id: TaskId = json_body(created).await?["id"]
        .as_str()
        .ok_or("task id missing")?
        .parse()?;
    LifecycleService::new(fixture.store.clone())
        .reserve(&ReserveCommand {
            task_id,
            expected_task_version: 0,
            worker_id,
            attempt_id: AttemptId::random(),
            submission_key: SubmissionKey::random(),
            reserved_at: chrono::Utc::now(),
        })
        .await
        .map_err(|error| std::io::Error::other(format!("reserve failed: {error}")))?;
    Ok(())
}

async fn create_submission_rejection(fixture: &Fixture, worker_id: WorkerId) -> TestResult<TaskId> {
    let task_id = super::task_support::create_api_task(fixture, "submission-rejection").await?;
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
    ] {
        let task = fixture.store.task(task_id).await?.ok_or("task missing")?;
        let attempt = fixture
            .store
            .attempt(attempt_id)
            .await?
            .ok_or("attempt missing")?;
        service
            .advance(&task, &attempt, command, chrono::Utc::now())
            .await
            .map_err(|error| std::io::Error::other(format!("advance failed: {error}")))?;
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
            LifecycleFailure::terminal(
                TaskStatus::Submitting,
                FailureStage::Submission,
                FailureCode::RemoteSubmissionFailed,
                "test Worker rejected the workflow",
            ),
            chrono::Utc::now(),
        )
        .await
        .map_err(|error| std::io::Error::other(format!("fail failed: {error}")))?;
    Ok(task_id)
}
