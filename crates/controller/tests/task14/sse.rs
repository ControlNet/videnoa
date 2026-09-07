use std::time::Duration;

use axum::http::StatusCode;
use futures_util::StreamExt;
use serde_json::json;
use tower::ServiceExt;
use videnoa_controller::persistence::SettingsUpdate;

use super::support::{Fixture, TestResult};
use super::task_support::{create_online_retry_worker, task_body};

#[tokio::test]
async fn task_creation_publishes_live_delta() -> TestResult {
    let fixture = Fixture::new().await?;
    let events = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/events", None)?)
        .await?;
    let mut stream = events.into_body().into_data_stream();
    let initial = stream.next().await.ok_or("missing initial refetch")??;
    assert!(String::from_utf8_lossy(&initial).contains("event: refetch"));
    let task = task_body(&fixture);
    let mut request = Fixture::request("POST", "/api/tasks", Some(&task))?;
    request
        .headers_mut()
        .insert("idempotency-key", "task-14-sse-create".parse()?);

    let created = fixture.router.clone().oneshot(request).await?;
    assert_eq!(created.status(), StatusCode::CREATED);
    let delta = stream.next().await.ok_or("missing task event")??;
    assert!(String::from_utf8_lossy(&delta).contains("task_updated"));
    Ok(())
}

#[tokio::test]
async fn scheduler_reservation_publishes_background_task_delta() -> TestResult {
    let fixture = Fixture::new().await?;
    let worker_id = create_online_retry_worker(&fixture, "127.0.0.1:9".parse()?).await?;
    let events = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/events", None)?)
        .await?;
    let mut stream = events.into_body().into_data_stream();
    let _ = stream.next().await.ok_or("missing initial refetch")??;
    let task = task_body(&fixture);
    let mut request = Fixture::request("POST", "/api/tasks", Some(&task))?;
    request
        .headers_mut()
        .insert("idempotency-key", "task-14-scheduler-sse".parse()?);
    let created = fixture.router.clone().oneshot(request).await?;
    assert_eq!(created.status(), StatusCode::CREATED);
    let _ = stream.next().await.ok_or("missing creation event")??;

    let assignment = fixture
        .scheduler
        .reserve_next(chrono::Utc::now())
        .await?
        .ok_or("missing assignment")?;
    assert_eq!(assignment.worker_id(), worker_id);
    let delta = tokio::time::timeout(Duration::from_secs(1), stream.next())
        .await?
        .ok_or("missing reservation event")??;
    let delta = String::from_utf8_lossy(&delta);
    assert!(delta.contains("task_updated"));
    assert!(delta.contains("\"status\":\"reserved\""));
    Ok(())
}

#[tokio::test]
async fn lagged_sse_subscriber_is_told_to_refetch_without_history_replay() -> TestResult {
    let fixture = Fixture::new().await?;
    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/events", None)?)
        .await?;
    let mut stream = response.into_body().into_data_stream();
    let initial = stream.next().await.ok_or("missing initial refetch")??;
    assert!(String::from_utf8_lossy(&initial).contains("event: refetch"));

    for index in 0..65 {
        let worker = json!({
            "name": format!("worker-{index}"),
            "api_url": format!("https://worker-{index}.example/api/"),
            "enabled": true,
            "compute_slots": 1
        });
        let created = fixture
            .router
            .clone()
            .oneshot(Fixture::request("POST", "/api/workers", Some(&worker))?)
            .await?;
        assert_eq!(created.status(), StatusCode::CREATED);
    }
    let lagged = stream.next().await.ok_or("missing lag refetch")??;
    assert!(String::from_utf8_lossy(&lagged).contains("event: refetch"));
    Ok(())
}

#[tokio::test]
async fn direct_scheduler_update_publishes_live_delta() -> TestResult {
    let fixture = Fixture::new().await?;
    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/events", None)?)
        .await?;
    let mut stream = response.into_body().into_data_stream();
    let _initial = stream.next().await.ok_or("missing initial refetch")??;
    let settings = fixture.store.config_manager().settings()?;
    let mut scheduler = settings.scheduler;
    scheduler.paused = true;

    fixture
        .scheduler
        .update_settings(SettingsUpdate {
            expected_version: settings.version,
            scheduler,
            timeouts: settings.timeouts,
            retry: settings.retry,
            updated_at: chrono::Utc::now(),
        })
        .await?;

    let event = stream.next().await.ok_or("missing scheduler delta")??;
    assert!(String::from_utf8_lossy(&event).contains("event: scheduler_updated"));
    Ok(())
}
