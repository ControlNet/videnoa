use std::time::Duration;

use axum::http::StatusCode;
use futures_util::StreamExt;
use serde_json::json;
use tower::ServiceExt;
use videnoa_controller::domain::WorkerCapabilities;
use videnoa_controller::persistence::WorkerHealthUpdate;
use videnoa_controller::workers::WorkerRegistry;

use super::support::{json_body, Fixture, TestResult};

#[tokio::test]
async fn worker_crud_uses_optimistic_versions_and_publishes_live_delta() -> TestResult {
    let fixture = Fixture::new().await?;
    let events = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/events", None)?)
        .await?;
    assert_eq!(events.status(), StatusCode::OK);
    let mut stream = events.into_body().into_data_stream();
    let initial = stream.next().await.ok_or("missing refetch event")??;
    assert!(String::from_utf8_lossy(&initial).contains("event: refetch"));

    let create = json!({
        "name": "worker-a", "api_url": "https://worker.example/api/",
        "enabled": true, "compute_slots": 2
    });
    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request("POST", "/api/workers", Some(&create))?)
        .await?;
    assert_eq!(response.status(), StatusCode::CREATED);
    let worker = json_body(response).await?;
    let id = worker["id"].as_str().ok_or("worker id missing")?;
    assert_eq!(worker["version"], 0);
    let delta = stream.next().await.ok_or("missing worker event")??;
    assert!(String::from_utf8_lossy(&delta).contains("worker_updated"));

    let update = json!({
        "version": 0, "name": "worker-renamed", "api_url": "https://worker.example/api/",
        "enabled": false, "compute_slots": 3
    });
    let uri = format!("/api/workers/{id}");
    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request("PUT", &uri, Some(&update))?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await?["version"], 1);
    let stale = fixture
        .router
        .clone()
        .oneshot(Fixture::request("PUT", &uri, Some(&update))?)
        .await?;
    assert_eq!(stale.status(), StatusCode::CONFLICT);

    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/workers", None)?)
        .await?;
    let workers = json_body(response).await?;
    assert_eq!(workers["total"], 1);
    assert_eq!(workers["items"][0]["name"], "worker-renamed");
    let deleted = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "DELETE",
            &format!("/api/workers/{id}?version=1"),
            None,
        )?)
        .await?;
    assert_eq!(deleted.status(), StatusCode::OK);
    assert_eq!(json_body(deleted).await?["deleted"], true);
    Ok(())
}

#[tokio::test]
async fn worker_health_refresh_publishes_background_delta() -> TestResult {
    let fixture = Fixture::new().await?;
    let worker_id =
        super::task_support::create_online_retry_worker(&fixture, "127.0.0.1:9".parse()?).await?;
    let events = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/events", None)?)
        .await?;
    let mut stream = events.into_body().into_data_stream();
    let _ = stream.next().await.ok_or("missing initial refetch")??;
    let worker = fixture
        .store
        .worker(worker_id)
        .await?
        .ok_or("worker missing")?;
    let now = chrono::Utc::now();

    WorkerRegistry::new(fixture.store.clone())
        .refresh_health(WorkerHealthUpdate {
            id: worker_id,
            expected_version: worker.version,
            online: false,
            capabilities: WorkerCapabilities {
                workflows: Vec::new(),
                refreshed_at: Some(now),
            },
            last_seen_at: worker.last_seen_at,
            health_retry_count: 1,
            next_health_check_at: Some(now),
            last_error: Some("health check failed".to_owned()),
            updated_at: now,
        })
        .await?;
    let delta = tokio::time::timeout(Duration::from_secs(1), stream.next())
        .await?
        .ok_or("missing worker health event")??;
    let delta = String::from_utf8_lossy(&delta);
    assert!(delta.contains("worker_updated"));
    assert!(delta.contains("\"online\":false"));
    Ok(())
}
