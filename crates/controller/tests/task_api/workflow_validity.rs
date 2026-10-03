use axum::http::StatusCode;
use chrono::Utc;
use serde_json::Value;
use tower::ServiceExt;
use videnoa_controller::domain::{
    ComputeSlots, InvalidWorkflow, WorkerApiUrl, WorkerCapabilities, WorkerId, WorkerName,
    WorkflowKind, WorkflowName, WorkflowSummary,
};
use videnoa_controller::persistence::{NewWorker, WorkerHealthUpdate};

use super::batch_create::{options, task_count};
use super::support::{bearer_request, fixture, json_body, task_request, Fixture, TestResult};

const REASON: &str = "workflow validation failed: unknown node type 'Blur'";

/// Records a probed worker that lists `anime-upscale` as valid or invalid.
async fn probed_worker(fixture: &Fixture, name: &str, valid: bool) -> TestResult {
    let id = WorkerId::random();
    let now = Utc::now();
    fixture
        .store
        .insert_worker(&NewWorker {
            password: None,
            id,
            name: WorkerName::new(name),
            api_url: WorkerApiUrl::parse(&format!("http://{name}.test:3000"))?,
            enabled: true,
            online: false,
            compute_slots: ComputeSlots::try_from(1)?,
            created_at: now,
        })
        .await?;
    let version = fixture
        .store
        .worker(id)
        .await?
        .ok_or("worker missing")?
        .version;
    let workflow = WorkflowName::new("anime-upscale");
    let (workflows, invalid_workflows) = if valid {
        let summary = WorkflowSummary {
            name: workflow,
            kind: WorkflowKind::Preset,
        };
        (vec![summary], Vec::new())
    } else {
        let invalid = InvalidWorkflow {
            name: workflow,
            kind: WorkflowKind::Preset,
            reason: REASON.to_owned(),
        };
        (Vec::new(), vec![invalid])
    };
    fixture
        .store
        .update_worker_health(&WorkerHealthUpdate {
            id,
            expected_version: version,
            online: true,
            capabilities: WorkerCapabilities {
                workflows,
                invalid_workflows,
                refreshed_at: Some(now),
            },
            last_seen_at: Some(now),
            health_retry_count: 0,
            next_health_check_at: None,
            last_error: None,
            updated_at: now,
        })
        .await?;
    Ok(())
}

async fn post(fixture: &Fixture, uri: &str, body: &Value) -> TestResult<(StatusCode, Value)> {
    let response = fixture
        .router
        .clone()
        .oneshot(bearer_request("POST", uri, Some(body))?)
        .await?;
    let status = response.status();
    Ok((status, json_body(response).await?))
}

fn assert_workflow_rejected(status: StatusCode, body: &Value) {
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let field_error = &body["error"]["field_errors"][0];
    assert_eq!(field_error["field"], "workflow");
    assert_eq!(field_error["code"], "invalid_value");
}

#[tokio::test]
async fn creation_rejects_a_workflow_every_worker_reports_invalid() -> TestResult {
    // Given: the only worker that has the workflow reports that it would reject it.
    let fixture = fixture().await?;
    probed_worker(&fixture, "broken", false).await?;
    let single = task_request(&fixture.input, &fixture.output, 0);

    // When / Then: single, preview and batch creation fail before any task exists.
    let (status, body) = post(&fixture, "/api/tasks", &single).await?;
    assert_workflow_rejected(status, &body);
    let (status, body) = post(&fixture, "/api/tasks/batch-preview", &options()).await?;
    assert_workflow_rejected(status, &body);
    let (status, body) = post(&fixture, "/api/tasks/batch", &options()).await?;
    assert_workflow_rejected(status, &body);
    assert_eq!(task_count(&fixture).await?, 0);

    // Given: another worker can run it.
    probed_worker(&fixture, "healthy", true).await?;

    // Then: creation is accepted again.
    let (status, body) = post(&fixture, "/api/tasks", &single).await?;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    Ok(())
}

#[tokio::test]
async fn creation_accepts_a_workflow_no_worker_has_reported() -> TestResult {
    // Given: no worker has reported anything about the workflow yet.
    let fixture = fixture().await?;

    // When: a task is created.
    let body = task_request(&fixture.input, &fixture.output, 0);
    let (status, body) = post(&fixture, "/api/tasks", &body).await?;

    // Then: it queues until a worker can run it, as before.
    assert_eq!(status, StatusCode::CREATED, "{body}");
    Ok(())
}
