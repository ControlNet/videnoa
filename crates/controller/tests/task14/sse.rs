use std::time::Duration;

use axum::body::BodyDataStream;
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
    // Creating the workers can outlast the 30 s auth recheck, and a keep-alive
    // comment may be sent while that recheck runs.
    let lagged = loop {
        let chunk = stream.next().await.ok_or("missing lag refetch")??;
        let text = String::from_utf8_lossy(&chunk).into_owned();
        if !text.trim_start().starts_with(':') {
            break text;
        }
    };
    assert!(lagged.contains("event: refetch"), "{lagged}");
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

/// A stream whose session was revoked ends at the next auth recheck, even
/// while events keep arriving: they must not starve the recheck.
#[tokio::test]
async fn revoked_session_stream_ends_at_the_recheck_despite_queued_events() -> TestResult {
    use axum::http::header;
    use videnoa_controller::auth::{AuthService, SESSION_COOKIE};
    use videnoa_controller::config::ControllerConfig;
    use videnoa_controller::domain::SecretString;

    let fixture = Fixture::new().await?;
    let config = ControllerConfig::for_workspace(&fixture.workspace)?;
    let auth = AuthService::new(config.auth, fixture.store.clone())?;
    let session = auth
        .login(
            "127.0.0.1".parse()?,
            &SecretString::new(super::support::PASSWORD),
            chrono::Utc::now(),
        )
        .await?;
    let mut request = Fixture::request("GET", "/api/events", None)?;
    request.headers_mut().remove(header::AUTHORIZATION);
    request.headers_mut().insert(
        header::COOKIE,
        format!("{SESSION_COOKIE}={}", session.token).parse()?,
    );
    let response = fixture.router.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.into_body().into_data_stream();
    let _initial = stream.next().await.ok_or("missing initial refetch")??;

    fixture
        .store
        .revoke_session(session.response.session.id, chrono::Utc::now())
        .await?;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(31)).await;
    for _ in 0..3 {
        let settings = fixture.store.config_manager().settings()?;
        let mut scheduler = settings.scheduler;
        scheduler.paused = !scheduler.paused;
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
    }

    // Keep-alive comments may come before the end of the body.
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        assert!(
            !String::from_utf8_lossy(&chunk).contains("scheduler_updated"),
            "a revoked session received an event after its recheck was due"
        );
    }
    Ok(())
}

async fn subscribe(fixture: &Fixture) -> TestResult<BodyDataStream> {
    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/events", None)?)
        .await?;
    let mut stream = response.into_body().into_data_stream();
    let initial = stream.next().await.ok_or("missing initial refetch")??;
    assert!(String::from_utf8_lossy(&initial).contains("event: refetch"));
    Ok(stream)
}

async fn create_worker(fixture: &Fixture, name: &str) -> TestResult {
    let worker = json!({
        "name": name,
        "api_url": format!("https://{name}.example/api/"),
        "enabled": true,
        "compute_slots": 1
    });
    let created = fixture
        .router
        .clone()
        .oneshot(Fixture::request("POST", "/api/workers", Some(&worker))?)
        .await?;
    assert_eq!(created.status(), StatusCode::CREATED);
    Ok(())
}

#[tokio::test]
async fn durable_change_is_read_once_and_shared_by_every_subscriber() -> TestResult {
    let fixture = Fixture::new().await?;
    let mut streams = Vec::new();
    for _ in 0..3 {
        streams.push(subscribe(&fixture).await?);
    }
    let reads = fixture.events.durable_change_reads();

    create_worker(&fixture, "shared-worker").await?;

    let mut deliveries = Vec::new();
    for stream in &mut streams {
        let event = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await?
            .ok_or("missing worker event")??;
        deliveries.push(event);
    }
    assert!(String::from_utf8_lossy(&deliveries[0]).contains("event: worker_updated"));
    assert!(deliveries.iter().all(|event| event == &deliveries[0]));
    assert_eq!(fixture.events.durable_change_reads() - reads, 1);
    Ok(())
}

#[tokio::test]
async fn durable_change_without_subscribers_is_not_read() -> TestResult {
    let fixture = Fixture::new().await?;

    create_worker(&fixture, "unobserved-worker").await?;

    assert_eq!(fixture.events.durable_change_reads(), 0);
    Ok(())
}
