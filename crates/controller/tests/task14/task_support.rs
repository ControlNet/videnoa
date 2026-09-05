use std::net::SocketAddr;

use axum::http::StatusCode;
use serde_json::json;
use tower::ServiceExt;
use videnoa_controller::domain::{
    TaskId, WorkerCapabilities, WorkerId, WorkflowKind, WorkflowName, WorkflowSummary,
};
use videnoa_controller::persistence::WorkerHealthUpdate;

use super::support::{json_body, Fixture, TestResult};

pub fn task_body(fixture: &Fixture) -> serde_json::Value {
    json!({
        "input_path": fixture.input,
        "output_path": fixture.output,
        "workflow": "anime-upscale",
        "priority": 0,
        "source": "api",
        "source_reference": null
    })
}

pub async fn create_api_task(fixture: &Fixture, idempotency_key: &str) -> TestResult<TaskId> {
    let task = task_body(fixture);
    let mut request = Fixture::request("POST", "/api/tasks", Some(&task))?;
    request
        .headers_mut()
        .insert("idempotency-key", idempotency_key.parse()?);
    let created = fixture.router.clone().oneshot(request).await?;
    assert_eq!(created.status(), StatusCode::CREATED);
    Ok(json_body(created).await?["id"]
        .as_str()
        .ok_or("task id missing")?
        .parse()?)
}

pub async fn create_online_retry_worker(
    fixture: &Fixture,
    address: SocketAddr,
) -> TestResult<WorkerId> {
    let worker = json!({
        "name": "retry-worker",
        "api_url": format!("http://{address}/"),
        "enabled": true,
        "compute_slots": 1
    });
    let created = fixture
        .router
        .clone()
        .oneshot(Fixture::request("POST", "/api/workers", Some(&worker))?)
        .await?;
    let worker_id = json_body(created).await?["id"]
        .as_str()
        .ok_or("worker id missing")?
        .parse()?;
    let now = chrono::Utc::now();
    fixture
        .store
        .update_worker_health(&WorkerHealthUpdate {
            id: worker_id,
            expected_version: 0,
            online: true,
            capabilities: WorkerCapabilities {
                workflows: vec![WorkflowSummary {
                    name: WorkflowName::new("anime-upscale"),
                    kind: WorkflowKind::Workflow,
                }],
                refreshed_at: Some(now),
            },
            last_seen_at: Some(now),
            health_retry_count: 0,
            next_health_check_at: None,
            last_error: None,
            updated_at: now,
        })
        .await?;
    Ok(worker_id)
}

pub async fn install_post_update_corruption(fixture: &Fixture) -> TestResult {
    sqlx::query(
        "CREATE TRIGGER task14_corrupt_progress AFTER UPDATE ON tasks
         WHEN NEW.progress_json != '{\"percent\":0,\"unexpected\":true}'
         BEGIN UPDATE tasks SET progress_json = '{\"percent\":0,\"unexpected\":true}' WHERE id = NEW.id; END",
    )
    .execute(fixture.store.database().pool())
    .await?;
    Ok(())
}
