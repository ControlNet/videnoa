use std::fs;

use axum::http::StatusCode;
use serde_json::json;
use tower::ServiceExt;

use super::support::{fixture, json_body, task_request, TestResult};

#[tokio::test]
async fn create_replay_conflict_history_and_detail_are_consistent() -> TestResult {
    let fixture = fixture().await?;
    let body = task_request(&fixture.input, &fixture.output, 7);
    let mut create = fixture.session.request("POST", "/api/tasks", Some(&body))?;
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
    let mut replay = fixture.session.request("POST", "/api/tasks", Some(&body))?;
    replay
        .headers_mut()
        .insert("idempotency-key", "stable-key".parse()?);
    let response = fixture.router.clone().oneshot(replay).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await?, created);

    let conflicting = task_request(&fixture.input, &fixture.output, 8);
    let mut conflict = fixture
        .session
        .request("POST", "/api/tasks", Some(&conflicting))?;
    conflict
        .headers_mut()
        .insert("idempotency-key", "stable-key".parse()?);
    let response = fixture.router.clone().oneshot(conflict).await?;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(response).await?["error"]["code"], "conflict");

    let response = fixture
        .router
        .clone()
        .oneshot(
            fixture
                .session
                .request("GET", "/api/tasks?status=queued&limit=10", None)?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let page = json_body(response).await?;
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["id"], id);

    let response = fixture
        .router
        .clone()
        .oneshot(
            fixture
                .session
                .request("GET", &format!("/api/tasks/{id}"), None)?,
        )
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
        .oneshot(fixture.session.request("POST", "/api/tasks", Some(&body))?)
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    fs::write(&fixture.output, b"occupied")?;
    let mut existing = fixture.session.request("POST", "/api/tasks", Some(&body))?;
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

#[tokio::test]
async fn absolute_external_media_paths_are_persisted_without_rebasing() -> TestResult {
    let fixture = fixture().await?;
    let external = tempfile::TempDir::new()?;
    let input = external.path().join("E08.mkv");
    let output = external.path().join("E08.AI.mp4");
    fs::write(&input, b"synthetic external media")?;
    let body = task_request(&input, &output, 0);
    let mut request = fixture.session.request("POST", "/api/tasks", Some(&body))?;
    request
        .headers_mut()
        .insert("idempotency-key", "external-paths".parse()?);
    let response = fixture.router.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = json_body(response).await?;
    assert_eq!(created["input_path"], json!(input));
    assert_eq!(created["output_path"], json!(output));
    assert_eq!(created["input_extension"], "mkv");
    assert_eq!(created["output_extension"], "mp4");
    assert!(!output.exists());
    assert_eq!(fs::read_dir(external.path())?.count(), 1);
    Ok(())
}

#[tokio::test]
async fn https_session_can_enable_secure_cookies_then_requires_https_proof() -> TestResult {
    use axum::http::header;
    let fixture = fixture().await?;
    let response = fixture
        .router
        .clone()
        .oneshot(fixture.session.request("GET", "/api/settings", None)?)
        .await?;
    let settings = json_body(response).await?;
    let mut body = json!({
        "version": settings["version"], "server": settings["server"],
        "auth": { "secure_cookie": true,
            "session_absolute_seconds": settings["session_absolute_seconds"],
            "session_idle_seconds": settings["session_idle_seconds"] },
        "scheduler": settings["scheduler"], "timeouts": settings["timeouts"], "retry": settings["retry"]
    });
    let mut change = fixture
        .session
        .request("PUT", "/api/settings", Some(&body))?;
    change
        .headers_mut()
        .insert(header::ORIGIN, "https://controller.test".parse()?);
    let response = fixture.router.clone().oneshot(change).await?;
    assert_eq!(response.status(), StatusCode::OK);
    body["version"] = json_body(response).await?["version"].clone();
    // The existing security contract invalidates cookies issued under the old policy.
    let old = fixture
        .session
        .request("PUT", "/api/settings", Some(&body))?;
    assert_eq!(
        fixture.router.clone().oneshot(old).await?.status(),
        StatusCode::UNAUTHORIZED
    );
    let session = super::support::SessionClient::login(&fixture.router, true).await?;
    let insecure = session.request("PUT", "/api/settings", Some(&body))?;
    assert_eq!(
        fixture.router.clone().oneshot(insecure).await?.status(),
        StatusCode::FORBIDDEN
    );
    let mut secure = session.request("PUT", "/api/settings", Some(&body))?;
    secure
        .headers_mut()
        .insert(header::ORIGIN, "https://controller.test".parse()?);
    assert_eq!(
        fixture.router.clone().oneshot(secure).await?.status(),
        StatusCode::OK
    );
    Ok(())
}

#[tokio::test]
async fn relative_task_paths_persist_workspace_absolute_locations() -> TestResult {
    let fixture = fixture().await?;
    let body = task_request(
        std::path::Path::new("input/source.MKV"),
        std::path::Path::new("output/relative.mp4"),
        0,
    );
    let mut request = fixture.session.request("POST", "/api/tasks", Some(&body))?;
    request
        .headers_mut()
        .insert("idempotency-key", "relative-paths".parse()?);
    let response = fixture.router.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = json_body(response).await?;
    assert_eq!(created["input_path"], json!(fixture.input));
    assert_eq!(
        created["output_path"],
        json!(fixture.output.with_file_name("relative.mp4"))
    );
    Ok(())
}
