use std::sync::atomic::Ordering;

use axum::http::StatusCode;
use serde_json::{json, Value};
use tower::ServiceExt;
use videnoa_controller::domain::RemoteJobId;

use super::retry_support::{create_processing_failure, retry_job, retry_remote};
use super::support::{json_body, Fixture, TestResult};
use super::task_support::create_online_retry_worker;

#[derive(Clone, Copy)]
enum RetryEvidenceFault {
    WrongJobId,
    WrongWorkflow,
    WrongInput,
    WrongOutput,
    MissingParams,
    NullParams,
    Nonterminal,
    NotFound,
    Unavailable,
}

#[tokio::test]
async fn processing_retry_rejects_wrong_remote_job_id() -> TestResult {
    assert_processing_retry_rejected(RetryEvidenceFault::WrongJobId).await
}

#[tokio::test]
async fn processing_retry_rejects_wrong_remote_workflow() -> TestResult {
    assert_processing_retry_rejected(RetryEvidenceFault::WrongWorkflow).await
}

#[tokio::test]
async fn processing_retry_rejects_wrong_remote_input() -> TestResult {
    assert_processing_retry_rejected(RetryEvidenceFault::WrongInput).await
}

#[tokio::test]
async fn processing_retry_rejects_wrong_remote_output() -> TestResult {
    assert_processing_retry_rejected(RetryEvidenceFault::WrongOutput).await
}

#[tokio::test]
async fn processing_retry_rejects_missing_remote_params() -> TestResult {
    assert_processing_retry_rejected(RetryEvidenceFault::MissingParams).await
}

#[tokio::test]
async fn processing_retry_rejects_null_remote_params() -> TestResult {
    assert_processing_retry_rejected(RetryEvidenceFault::NullParams).await
}

#[tokio::test]
async fn processing_retry_rejects_nonterminal_remote_job() -> TestResult {
    assert_processing_retry_rejected(RetryEvidenceFault::Nonterminal).await
}

#[tokio::test]
async fn processing_retry_rejects_missing_remote_job() -> TestResult {
    assert_processing_retry_rejected(RetryEvidenceFault::NotFound).await
}

#[tokio::test]
async fn processing_retry_reports_unavailable_remote_worker() -> TestResult {
    assert_processing_retry_rejected(RetryEvidenceFault::Unavailable).await
}

async fn assert_processing_retry_rejected(fault: RetryEvidenceFault) -> TestResult {
    // Given: durable processing evidence and a contradictory, incomplete, or unavailable remote.
    let fixture = Fixture::new().await?;
    let remote_job_id = RemoteJobId::random();
    let mut job = retry_job(remote_job_id);
    let (response, expected_status, expected_code) = match fault {
        RetryEvidenceFault::WrongJobId => {
            job["id"] = json!(RemoteJobId::random());
            (Ok(job), StatusCode::CONFLICT, "remote_state_ambiguous")
        }
        RetryEvidenceFault::WrongWorkflow => {
            job["workflow_name"] = json!("other-workflow");
            (Ok(job), StatusCode::CONFLICT, "remote_state_ambiguous")
        }
        RetryEvidenceFault::WrongInput => {
            job["params"]["input"] = json!("other/input.mkv");
            (Ok(job), StatusCode::CONFLICT, "remote_state_ambiguous")
        }
        RetryEvidenceFault::WrongOutput => {
            job["params"]["output"] = json!("other/output.mp4");
            (Ok(job), StatusCode::CONFLICT, "remote_state_ambiguous")
        }
        RetryEvidenceFault::MissingParams => {
            job.as_object_mut()
                .ok_or("job object missing")?
                .remove("params");
            (Ok(job), StatusCode::CONFLICT, "remote_state_ambiguous")
        }
        RetryEvidenceFault::NullParams => {
            job["params"] = Value::Null;
            (Ok(job), StatusCode::CONFLICT, "remote_state_ambiguous")
        }
        RetryEvidenceFault::Nonterminal => {
            job["status"] = json!("running");
            (Ok(job), StatusCode::CONFLICT, "conflict")
        }
        RetryEvidenceFault::NotFound => (
            Err(StatusCode::NOT_FOUND),
            StatusCode::CONFLICT,
            "remote_state_ambiguous",
        ),
        RetryEvidenceFault::Unavailable => (
            Err(StatusCode::SERVICE_UNAVAILABLE),
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
        ),
    };
    let remote = retry_remote(response).await?;
    let worker_id = create_online_retry_worker(&fixture, remote.address).await?;
    let task_id = create_processing_failure(&fixture, worker_id, remote_job_id).await?;
    let failed = fixture.store.task(task_id).await?.ok_or("task missing")?;

    // When: retry is requested for the failed processing attempt.
    let retry = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            &format!("/api/tasks/{task_id}/retry"),
            Some(&json!({"version": failed.version})),
        )?)
        .await?;

    // Then: cleanup and replacement attempt creation do not occur.
    assert_eq!(retry.status(), expected_status);
    assert_eq!(json_body(retry).await?["error"]["code"], expected_code);
    assert_eq!(remote.workspace_deletes.load(Ordering::SeqCst), 0);
    assert_eq!(remote.job_deletes.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.store.task_attempts(task_id, 10).await?.len(), 1);
    remote.server.abort();
    Ok(())
}
