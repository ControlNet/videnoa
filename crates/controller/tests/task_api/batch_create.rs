use std::fs;

use axum::http::StatusCode;
use serde_json::{json, Value};
use tower::ServiceExt;

use super::support::{bearer_request, fixture, json_body, request, Fixture, TestResult};

pub(super) fn options() -> Value {
    json!({"input_pattern": "input/*.MKV", "output_mode": "directory",
        "output_directory": "output", "naming_mode": "insert_extension",
        "middle_extension": "AI", "workflow": "anime-upscale", "priority": 7})
}

pub(super) async fn task_count(fixture: &Fixture) -> TestResult<Value> {
    let response = fixture
        .router
        .clone()
        .oneshot(fixture.session.request("GET", "/api/tasks", None)?)
        .await?;
    Ok(json_body(response).await?["total"].clone())
}

#[tokio::test]
async fn batch_create_waits_for_all_tasks_and_defaults_to_api_without_key() -> TestResult {
    let fixture = fixture().await?;
    // Synthetic media exercises HTTP intake only, never video processing.
    fs::write(
        fixture.input.with_file_name("second.MKV"),
        b"synthetic second video",
    )?;
    let response = fixture
        .router
        .clone()
        .oneshot(bearer_request(
            "POST",
            "/api/tasks/batch",
            Some(&options()),
        )?)
        .await?;
    assert_eq!(response.status(), StatusCode::CREATED);
    let result = json_body(response).await?;
    assert_eq!(result["created"], 2);
    assert_eq!(result["failed"], 0);
    let items = result["items"].as_array().ok_or("missing items")?;
    assert_eq!(items.len(), 2);
    assert_ne!(items[0]["task"]["id"], items[1]["task"]["id"]);
    for item in items {
        assert!(item["error"].is_null());
        assert_eq!(item["task"]["source"], "api");
        assert!(item["task"]["source_reference"].is_null());
        assert_eq!(item["task"]["workflow"], "anime-upscale");
        assert_eq!(item["task"]["priority"], 7);
        assert!(item["task"]["output_path"]
            .as_str()
            .ok_or("missing output")?
            .ends_with(".AI.MKV"));
    }
    assert_eq!(task_count(&fixture).await?, 2);
    Ok(())
}

#[tokio::test]
async fn batch_preview_error_prevents_every_task_from_being_created() -> TestResult {
    let fixture = fixture().await?;
    fs::copy(&fixture.input, fixture.input.with_file_name("aaa.MKV"))?;
    let occupied = fixture.output.with_file_name("source.AI.MKV");
    fs::write(&occupied, b"existing output fixture")?;
    let response = fixture
        .router
        .clone()
        .oneshot(
            fixture
                .session
                .request("POST", "/api/tasks/batch", Some(&options()))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let result = json_body(response).await?;
    assert_eq!(result["created"], 0);
    assert_eq!(result["failed"], 1);
    let items = result["items"].as_array().ok_or("missing items")?;
    assert_eq!(items.len(), 2);
    assert!(items[0]["error"].is_null());
    assert!(items[1]["error"].is_object());
    assert!(items.iter().all(|item| item["task"].is_null()));
    assert_eq!(task_count(&fixture).await?, 0);
    assert_eq!(fs::read(&occupied)?, b"existing output fixture");
    Ok(())
}

#[tokio::test]
async fn batch_create_rejects_duplicates_no_matches_and_invalid_options() -> TestResult {
    let fixture = fixture().await?;
    let root = fixture.input.parent().ok_or("missing input parent")?;
    fs::create_dir(root.join("season"))?;
    fs::copy(&fixture.input, root.join("season/source.MKV"))?;
    for (field, value) in [
        ("input_pattern", json!("input/**/*.MKV")),
        ("input_pattern", json!("input/absent*.MKV")),
        ("priority", json!(101)),
        ("input_pattern", json!("../*")),
        ("unknown", json!(true)),
    ] {
        let mut body = options();
        body[field] = value;
        let response = fixture
            .router
            .clone()
            .oneshot(
                fixture
                    .session
                    .request("POST", "/api/tasks/batch", Some(&body))?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{field}");
        assert_eq!(task_count(&fixture).await?, 0);
    }
    Ok(())
}

#[tokio::test]
async fn batch_create_requires_auth_and_session_csrf() -> TestResult {
    let fixture = fixture().await?;
    let response = fixture
        .router
        .clone()
        .oneshot(request("POST", "/api/tasks/batch", Some(&options()))?)
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    for header in ["x-csrf-token", "origin"] {
        let mut request = fixture
            .session
            .request("POST", "/api/tasks/batch", Some(&options()))?;
        request.headers_mut().remove(header);
        assert_eq!(
            fixture.router.clone().oneshot(request).await?.status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(task_count(&fixture).await?, 0);
    Ok(())
}

#[tokio::test]
async fn batch_create_rejects_invalid_keys_and_excessive_matches_without_writes() -> TestResult {
    let fixture = fixture().await?;
    let mut request = fixture
        .session
        .request("POST", "/api/tasks/batch", Some(&options()))?;
    request.headers_mut().insert("idempotency-key", "".parse()?);
    let response = fixture.router.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(response).await?["error"]["field_errors"][0]["field"],
        "idempotency_key"
    );
    let root = fixture.input.parent().ok_or("missing input parent")?;
    for index in 0..500 {
        fs::write(
            root.join(format!("{index}.MKV")),
            b"synthetic scan limit fixture",
        )?;
    }
    let response = fixture
        .router
        .clone()
        .oneshot(
            fixture
                .session
                .request("POST", "/api/tasks/batch", Some(&options()))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(task_count(&fixture).await?, 0);
    Ok(())
}

#[tokio::test]
async fn batch_create_reports_partial_database_failure_after_preview() -> TestResult {
    use sqlx::Connection;

    let fixture = fixture().await?;
    fs::copy(&fixture.input, fixture.input.with_file_name("aaa.MKV"))?;
    let database = fixture
        .input
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("missing workspace")?
        .join("controller.sqlite3");
    let mut connection = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(database),
    )
    .await?;
    // Test-only fault injection: preview succeeds, but the second task insert fails.
    sqlx::query("CREATE TRIGGER fail_second_task BEFORE INSERT ON tasks WHEN NEW.input_path LIKE '%source.MKV' BEGIN SELECT RAISE(ABORT, 'test-only insert failure'); END")
        .execute(&mut connection).await?;
    let response = fixture
        .router
        .clone()
        .oneshot(
            fixture
                .session
                .request("POST", "/api/tasks/batch", Some(&options()))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::MULTI_STATUS);
    let result = json_body(response).await?;
    assert_eq!(result["created"], 1);
    assert_eq!(result["failed"], 1);
    assert!(result["items"][0]["task"]["id"].is_string());
    assert_eq!(result["items"][1]["error"]["code"], "internal_error");
    assert_eq!(task_count(&fixture).await?, 1);
    connection.close().await?;
    Ok(())
}

#[tokio::test]
async fn batch_source_reference_reaches_preview_and_every_persisted_task() -> TestResult {
    let fixture = fixture().await?;
    fs::copy(&fixture.input, fixture.input.with_file_name("second.MKV"))?;
    let mut body = options();
    body["source_reference"] = json!("ani-rss:test-only/动画:S03E11");
    for (route, status) in [
        ("/api/tasks/batch-preview", StatusCode::OK),
        ("/api/tasks/batch", StatusCode::CREATED),
    ] {
        let response = fixture
            .router
            .clone()
            .oneshot(fixture.session.request("POST", route, Some(&body))?)
            .await?;
        assert_eq!(response.status(), status);
        let response = json_body(response).await?;
        let items = response["items"].as_array().ok_or("missing items")?;
        assert_eq!(items.len(), 2);
        for item in items {
            assert_eq!(
                item["request"]["source_reference"],
                body["source_reference"]
            );
            if status == StatusCode::CREATED {
                assert_eq!(item["task"]["source_reference"], body["source_reference"]);
                let id = item["task"]["id"].as_str().ok_or("missing task id")?;
                let detail = fixture
                    .router
                    .clone()
                    .oneshot(
                        fixture
                            .session
                            .request("GET", &format!("/api/tasks/{id}"), None)?,
                    )
                    .await?;
                assert_eq!(
                    json_body(detail).await?["task"]["source_reference"],
                    body["source_reference"]
                );
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn invalid_batch_source_reference_rejects_before_any_task_creation() -> TestResult {
    let fixture = fixture().await?;
    for value in [json!(""), json!("界".repeat(171))] {
        let mut body = options();
        body["source_reference"] = value;
        for route in ["/api/tasks/batch-preview", "/api/tasks/batch"] {
            let response = fixture
                .router
                .clone()
                .oneshot(fixture.session.request("POST", route, Some(&body))?)
                .await?;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(
                json_body(response).await?["error"]["field_errors"][0]["field"],
                "source_reference"
            );
        }
        assert_eq!(task_count(&fixture).await?, 0);
    }
    Ok(())
}
