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
    assert_eq!(
        settings["paths"]["data_root"],
        json!(fixture.workspace.join("data"))
    );
    assert_eq!(
        settings["paths"]["cache_root"],
        json!(fixture.workspace.join("data"))
    );
    assert_eq!(settings["restart_required"], false);

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
        "paths": settings["paths"],
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

#[tokio::test]
async fn session_settings_have_no_fixed_day_limit() -> TestResult {
    let fixture = Fixture::new().await?;
    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/settings", None)?)
        .await?;
    let settings = json_body(response).await?;
    let mut update = settings_update(&settings, 11, 301);
    update["auth"]["session_absolute_seconds"] = json!(31_536_000);
    update["auth"]["session_idle_seconds"] = json!(2_592_000);
    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request("PUT", "/api/settings", Some(&update))?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let saved = json_body(response).await?;
    assert_eq!(saved["session_absolute_seconds"], 31_536_000);
    assert_eq!(saved["session_idle_seconds"], 2_592_000);
    let document = fs::read_to_string(&fixture.config_file)?;
    assert!(document.contains("session_absolute_seconds = 31536000"));
    assert!(document.contains("session_idle_seconds = 2592000"));
    Ok(())
}

#[tokio::test]
async fn settings_updates_keep_iroh_relays_from_the_config_file() -> TestResult {
    // Given: a Controller whose controller.toml lists a self-hosted relay.
    let fixture = Fixture::with_iroh_relays(&["https://relay.example.test"]).await?;
    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/settings", None)?)
        .await?;
    let settings = json_body(response).await?;

    // When: runtime settings are saved through the API, which rewrites the file.
    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "PUT",
            "/api/settings",
            Some(&settings_update(&settings, 12, 302)),
        )?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);

    // Then: the relay section the API does not manage is kept.
    let document = fs::read_to_string(&fixture.config_file)?;
    assert!(document.contains("health_seconds = 12"), "{document}");
    let reloaded =
        videnoa_controller::config::ControllerConfig::from_toml_in(&document, &fixture.workspace)?;
    assert_eq!(reloaded.iroh.relay_urls, ["https://relay.example.test"]);
    Ok(())
}

#[tokio::test]
async fn iroh_relays_are_edited_through_settings_and_apply_after_restart() -> TestResult {
    // Given: a Controller started on the public N0 network.
    let fixture = Fixture::new().await?;
    let router = &fixture.router;
    let get = || {
        let router = router.clone();
        async move {
            let response = router
                .oneshot(Fixture::request("GET", "/api/settings", None)?)
                .await?;
            json_body(response).await
        }
    };
    let put = |body: Value| {
        let router = router.clone();
        async move {
            let response = router
                .oneshot(Fixture::request("PUT", "/api/settings", Some(&body))?)
                .await?;
            let status = response.status();
            TestResult::Ok((status, json_body(response).await?))
        }
    };
    let settings = get().await?;
    assert_eq!(
        settings["iroh"],
        json!({"relay_urls": [], "use_public_relays": false, "restart_required": false})
    );

    // When: relays are saved.
    let mut update = settings_update(&settings, 10, 300);
    update["iroh"] = json!({
        "relay_urls": ["https://relay.example.test"],
        "use_public_relays": true
    });
    let (status, saved) = put(update).await?;

    // Then: they are durable and reported as pending until the next start.
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(
        saved["iroh"],
        json!({
            "relay_urls": ["https://relay.example.test"],
            "use_public_relays": true,
            "restart_required": true
        })
    );
    // The scheduler stays controllable: only the path restart locks it.
    assert_eq!(saved["restart_required"], false);
    let document = fs::read_to_string(&fixture.config_file)?;
    let reloaded =
        videnoa_controller::config::ControllerConfig::from_toml_in(&document, &fixture.workspace)?;
    assert_eq!(reloaded.iroh.relay_urls, ["https://relay.example.test"]);
    assert!(reloaded.iroh.use_public_relays);

    // An invalid URL is a field error and changes nothing.
    let mut invalid = settings_update(&saved, 10, 300);
    invalid["iroh"] = json!({"relay_urls": ["ftp://relay.example.test"]});
    let (status, body) = put(invalid).await?;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["field_errors"][0]["field"], "iroh");
    assert_eq!(
        get().await?["iroh"]["relay_urls"],
        json!(["https://relay.example.test"])
    );

    // Clearing them matches the running configuration again.
    let mut cleared = settings_update(&saved, 10, 300);
    cleared["iroh"] = json!({"relay_urls": []});
    let (status, body) = put(cleared).await?;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["iroh"]["restart_required"], false);
    Ok(())
}
