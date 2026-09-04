use std::fs;

use axum::http::StatusCode;
use serde_json::json;
use tower::ServiceExt;

use super::support::{fixture, json_body, request, task_request, TestResult};

#[tokio::test]
async fn create_replay_conflict_history_and_detail_are_consistent() -> TestResult {
    let fixture = fixture().await?;
    let body = task_request(&fixture.input, &fixture.output, 7);
    let mut create = request("POST", "/api/tasks", Some(&body))?;
    create
        .headers_mut()
        .insert("idempotency-key", "stable-key".parse()?);
    let response = fixture.router.clone().oneshot(create).await?;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = json_body(response).await?;
    let id = created["id"].as_str().ok_or("task id missing")?;
    assert_eq!(created["input_extension"], "MKV");
    assert_eq!(created["output_extension"], "mp4");

    fs::remove_file(&fixture.input)?;
    let mut replay = request("POST", "/api/tasks", Some(&body))?;
    replay
        .headers_mut()
        .insert("idempotency-key", "stable-key".parse()?);
    let response = fixture.router.clone().oneshot(replay).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await?, created);

    let conflicting = task_request(&fixture.input, &fixture.output, 8);
    let mut conflict = request("POST", "/api/tasks", Some(&conflicting))?;
    conflict
        .headers_mut()
        .insert("idempotency-key", "stable-key".parse()?);
    let response = fixture.router.clone().oneshot(conflict).await?;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(response).await?["error"]["code"], "conflict");

    let response = fixture
        .router
        .clone()
        .oneshot(request("GET", "/api/tasks?status=queued&limit=10", None)?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let page = json_body(response).await?;
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["id"], id);

    let response = fixture
        .router
        .clone()
        .oneshot(request("GET", &format!("/api/tasks/{id}"), None)?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let detail = json_body(response).await?;
    assert_eq!(detail["task"], created);
    assert_eq!(detail["attempts"], json!([]));
    assert_eq!(detail["total"], 0);
    assert_eq!(detail["limit"], 100);
    assert_eq!(detail["offset"], 0);
    Ok(())
}

#[tokio::test]
async fn create_rejects_missing_key_invalid_paths_and_existing_output() -> TestResult {
    let fixture = fixture().await?;
    let body = task_request(&fixture.input, &fixture.output, 0);
    let response = fixture
        .router
        .clone()
        .oneshot(request("POST", "/api/tasks", Some(&body))?)
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    fs::write(&fixture.output, b"occupied")?;
    let mut existing = request("POST", "/api/tasks", Some(&body))?;
    existing
        .headers_mut()
        .insert("idempotency-key", "output-exists".parse()?);
    let response = fixture.router.clone().oneshot(existing).await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(response).await?["error"]["field_errors"][0]["field"],
        "output_path"
    );
    Ok(())
}
