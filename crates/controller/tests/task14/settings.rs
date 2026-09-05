use std::fs;

use axum::http::StatusCode;
use serde_json::{json, Value};
use tower::ServiceExt;

use super::support::{json_body, Fixture, TestResult};
use super::task_support::create_api_task;
use super::tasks_actions::create_and_cancel_task;

#[tokio::test]
async fn settings_pause_counts_cancel_and_readiness_are_operational() -> TestResult {
    let fixture = Fixture::new().await?;
    update_runtime_settings(&fixture).await?;
    pause_and_reject_stale_resume(&fixture).await?;
    create_and_cancel_task(&fixture).await?;

    let counts = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/status-counts", None)?)
        .await?;
    let counts = json_body(counts).await?;
    assert_eq!(counts["total"], 1);
    assert!(counts["items"].as_array().is_some_and(|items| items
        .iter()
        .any(|item| item == &json!({"status": "cancelled", "count": 1}))));

    let readiness = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/readiness", None)?)
        .await?;
    assert_eq!(readiness.status(), StatusCode::OK);
    let readiness = json_body(readiness).await?;
    assert_eq!(readiness["status"], "ready");
    assert_eq!(readiness["checks"].as_array().map(Vec::len), Some(3));
    Ok(())
}

async fn update_runtime_settings(fixture: &Fixture) -> TestResult {
    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/settings", None)?)
        .await?;
    let settings = json_body(response).await?;
    assert_eq!(settings["version"], 0);
    assert_eq!(settings["paths"]["workspace"], json!(fixture.workspace));
    assert_eq!(
        settings["paths"]["data_root"],
        json!(fixture.workspace.join("data"))
    );
    assert_eq!(settings["paths"]["config_file"], json!(fixture.config_file));

    let updated = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "PUT",
            "/api/settings",
            Some(&settings_update(&settings, 11, 301)),
        )?)
        .await?;
    assert_eq!(updated.status(), StatusCode::OK);
    assert_eq!(json_body(updated).await?["version"], 1);
    assert_eq!(
        fixture.scheduler.runtime_settings().timeout_settings(),
        videnoa_controller::domain::TimeoutSettingsDto {
            health_seconds: 11,
            poll_seconds: 7,
            transfer_seconds: 301,
        }
    );
    assert_eq!(
        fixture.scheduler.runtime_settings().retry_settings(),
        videnoa_controller::domain::RetrySettingsDto {
            initial_seconds: 2,
            maximum_seconds: 30,
            max_attempts: 4,
        }
    );
    let document = fs::read_to_string(&fixture.config_file)?;
    assert!(document.contains("health_seconds = 11"));
    assert!(document.contains("poll_seconds = 7"));
    assert!(document.contains("max_attempts = 4"));
    Ok(())
}

async fn pause_and_reject_stale_resume(fixture: &Fixture) -> TestResult {
    let paused = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            "/api/scheduler/pause",
            Some(&json!({"version": 1})),
        )?)
        .await?;
    assert_eq!(paused.status(), StatusCode::OK);
    assert_eq!(json_body(paused).await?["scheduler"]["paused"], true);
    let stale = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            "/api/scheduler/resume",
            Some(&json!({"version": 1})),
        )?)
        .await?;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    Ok(())
}

#[tokio::test]
async fn invalid_settings_and_cancelled_retry_return_typed_conflicts() -> TestResult {
    let fixture = Fixture::new().await?;
    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/settings", None)?)
        .await?;
    let settings = json_body(response).await?;
    let invalid = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "PUT",
            "/api/settings",
            Some(&settings_update(
                &settings,
                0,
                settings["timeouts"]["transfer_seconds"]
                    .as_u64()
                    .ok_or("transfer timeout missing")?,
            )),
        )?)
        .await?;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(invalid).await?["error"]["field_errors"][0]["field"],
        "health_seconds"
    );
    let excessive = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "PUT",
            "/api/settings",
            Some(&settings_update(
                &settings,
                settings["timeouts"]["health_seconds"]
                    .as_u64()
                    .ok_or("health timeout missing")?,
                604_801,
            )),
        )?)
        .await?;
    assert_eq!(excessive.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(excessive).await?["error"]["field_errors"][0]["field"],
        "transfer_seconds"
    );

    let task_id = create_api_task(&fixture, "task-14-retry").await?;
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
    let retry = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            &format!("/api/tasks/{task_id}/retry"),
            Some(&json!({"version": 1})),
        )?)
        .await?;
    assert_eq!(retry.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(retry).await?["error"]["code"], "conflict");
    Ok(())
}

fn settings_update(settings: &Value, health_seconds: u64, transfer_seconds: u64) -> Value {
    json!({
        "version": settings["version"],
        "server": settings["server"],
        "auth": {
            "secure_cookie": settings["secure_cookie"],
            "session_absolute_seconds": settings["session_absolute_seconds"],
            "session_idle_seconds": settings["session_idle_seconds"]
        },
        "scheduler": settings["scheduler"],
        "timeouts": {
            "health_seconds": health_seconds,
            "poll_seconds": 7,
            "transfer_seconds": transfer_seconds
        },
        "retry": {
            "initial_seconds": 2,
            "maximum_seconds": 30,
            "max_attempts": 4
        }
    })
}
