use std::error::Error;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::extract::connect_info::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use futures_util::StreamExt;
use serde_json::{json, Value};
use tempfile::TempDir;
use tower::ServiceExt;
use videnoa_controller::auth::{hash_password, AuthService};
use videnoa_controller::config::{AuthConfig, ControllerConfig, PathConfig};
use videnoa_controller::domain::{
    AttemptId, RemoteJobId, RemotePath, SubmissionKey, TaskId, WorkerCapabilities, WorkflowKind,
    WorkflowName, WorkflowSummary,
};
use videnoa_controller::lifecycle::{
    AdvanceCommand, LifecycleFailure, LifecycleService, ReserveCommand, SubmissionEvidence,
    UploadEvidence,
};
use videnoa_controller::operations::{EventHub, OperationsDependencies, OperationsState};
use videnoa_controller::paths::PathCapabilities;
use videnoa_controller::persistence::{
    Database, DatabaseOptions, SettingsUpdate, Store, WorkerHealthUpdate,
};
use videnoa_controller::remote::PayloadLimits;
use videnoa_controller::scheduler::Scheduler;
use videnoa_controller::tasks::TaskService;
use videnoa_controller::workers::WorkerRegistry;
use videnoa_controller::{controller_app_router, FrontendAssets};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
const PASSWORD: &str = "test-only-operations-password";

struct Fixture {
    _directory: TempDir,
    router: Router,
    input: PathBuf,
    output: PathBuf,
    scheduler: Scheduler,
    store: Store,
    hash_file: PathBuf,
}

struct RetryRemote {
    address: SocketAddr,
    job_deletes: Arc<AtomicUsize>,
    workspace_deletes: Arc<AtomicUsize>,
    server: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}

#[derive(Clone, Copy)]
enum RetryEvidenceFault {
    WrongJobId,
    WrongWorkflow,
    WrongInput,
    WrongOutput,
    MissingParams,
    NullParams,
    Nonterminal,
    NotFound,
    Unavailable,
}

impl Fixture {
    async fn new() -> TestResult<Self> {
        let directory = TempDir::new()?;
        let input_root = directory.path().join("input");
        let output_root = directory.path().join("output");
        let data_root = directory.path().join("data");
        let temp_root = directory.path().join("temp");
        for path in [&input_root, &output_root, &data_root, &temp_root] {
            fs::create_dir(path)?;
        }
        let input = input_root.join("source.mkv");
        let output = output_root.join("result.mp4");
        fs::write(&input, b"video")?;
        let hash_file = directory.path().join("admin-password.phc");
        fs::write(&hash_file, hash_password(PASSWORD)?)?;

        let database =
            Database::open(DatabaseOptions::new(data_root.join("controller.sqlite3"))).await?;
        let store = Store::new(database);
        let auth_config = AuthConfig {
            password_hash_file: hash_file.clone(),
            secure_cookie: false,
            session_absolute: Duration::from_secs(86_400),
            session_idle: Duration::from_secs(3_600),
        };
        let path_config = PathConfig {
            input_roots: vec![input_root],
            output_roots: vec![output_root],
            data_root,
            temp_root,
        };
        let config = ControllerConfig {
            auth: auth_config.clone(),
            paths: path_config.clone(),
            ..ControllerConfig::default()
        };
        let auth = AuthService::new(auth_config, store.clone())?;
        let paths = PathCapabilities::open(&path_config)?;
        let scheduler = Scheduler::load(store.clone()).await?;
        let events = EventHub::new();
        let operations = OperationsState::new(OperationsDependencies {
            auth: auth.clone(),
            store: store.clone(),
            scheduler: scheduler.clone(),
            paths: paths.clone(),
            config,
            events: events.clone(),
            payload_limits: PayloadLimits::new(1024 * 1024, 4096)?,
        });
        let tasks = TaskService::new(store.clone(), paths);
        let router = controller_app_router(&assets(directory.path())?, auth, tasks, operations);
        Ok(Self {
            _directory: directory,
            router,
            input,
            output,
            scheduler,
            store,
            hash_file,
        })
    }

    fn request(method: &str, uri: &str, body: Option<&Value>) -> TestResult<Request<Body>> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {PASSWORD}"));
        let body = match body {
            Some(value) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(serde_json::to_vec(value)?)
            }
            None => Body::empty(),
        };
        let mut request = builder.body(body)?;
        request.extensions_mut().insert(ConnectInfo(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            40_000,
        )));
        Ok(request)
    }
}

#[cfg(debug_assertions)]
fn assets(directory: &Path) -> TestResult<FrontendAssets> {
    let assets = directory.join("assets");
    fs::create_dir(&assets)?;
    fs::write(assets.join("index.html"), "<main>controller</main>")?;
    Ok(FrontendAssets::from_dist(assets)?)
}

#[cfg(not(debug_assertions))]
fn assets(_: &Path) -> TestResult<FrontendAssets> {
    Ok(FrontendAssets::embedded()?)
}

async fn json_body(response: axum::response::Response) -> TestResult<Value> {
    Ok(serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX).await?,
    )?)
}

#[tokio::test]
async fn operational_routes_require_authentication() -> TestResult {
    let fixture = Fixture::new().await?;
    for uri in [
        "/api/workers",
        "/api/settings",
        "/api/status-counts",
        "/api/events",
    ] {
        let mut request = Request::builder().uri(uri).body(Body::empty())?;
        request.extensions_mut().insert(ConnectInfo(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            40_000,
        )));
        let response = fixture.router.clone().oneshot(request).await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{uri}");
    }
    Ok(())
}

#[tokio::test]
async fn status_counts_materialize_every_status_for_empty_database() -> TestResult {
    // Given: a fresh Controller database with no tasks.
    let fixture = Fixture::new().await?;

    // When: the aggregate status endpoint is requested.
    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/status-counts", None)?)
        .await?;

    // Then: every status is present in lifecycle order with a zero count.
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await?,
        json!({
            "items": [
                {"status": "queued", "count": 0},
                {"status": "reserved", "count": 0},
                {"status": "uploading", "count": 0},
                {"status": "staged", "count": 0},
                {"status": "submitting", "count": 0},
                {"status": "processing", "count": 0},
                {"status": "remote_completed", "count": 0},
                {"status": "downloading", "count": 0},
                {"status": "verifying", "count": 0},
                {"status": "publishing", "count": 0},
                {"status": "remote_cleanup", "count": 0},
                {"status": "completed", "count": 0},
                {"status": "failed", "count": 0},
                {"status": "cancelled", "count": 0}
            ],
            "total": 0
        })
    );
    Ok(())
}

#[tokio::test]
async fn status_counts_zero_fill_partially_populated_database() -> TestResult {
    // Given: one queued task and one task represented as processing.
    let fixture = Fixture::new().await?;
    for key in ["task-14-count-queued", "task-14-count-processing"] {
        let task = json!({
            "input_path": fixture.input, "output_path": fixture.output,
            "workflow": "anime-upscale", "priority": 0, "source": "api", "source_reference": null
        });
        let mut request = Fixture::request("POST", "/api/tasks", Some(&task))?;
        request
            .headers_mut()
            .insert("idempotency-key", key.parse()?);
        assert_eq!(
            fixture.router.clone().oneshot(request).await?.status(),
            StatusCode::CREATED
        );
    }
    sqlx::query("UPDATE tasks SET status = 'processing' WHERE id = (SELECT id FROM tasks ORDER BY id LIMIT 1)")
        .execute(fixture.store.database().pool())
        .await?;

    // When: the aggregate status endpoint is requested.
    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/status-counts", None)?)
        .await?;

    // Then: sparse persisted rows are projected onto every deterministic category.
    assert_eq!(response.status(), StatusCode::OK);
    let counts = json_body(response).await?;
    assert_eq!(counts["items"].as_array().map(Vec::len), Some(14));
    assert_eq!(counts["total"], 2);
    assert_eq!(counts["items"][0], json!({"status": "queued", "count": 1}));
    assert_eq!(
        counts["items"][5],
        json!({"status": "processing", "count": 1})
    );
    assert!(counts["items"].as_array().is_some_and(|items| items
        .iter()
        .enumerate()
        .all(|(index, item)| index == 0 || index == 5 || item["count"] == 0)));
    Ok(())
}

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
    let delete_uri = format!("/api/workers/{id}?version=1");
    let deleted = fixture
        .router
        .clone()
        .oneshot(Fixture::request("DELETE", &delete_uri, None)?)
        .await?;
    assert_eq!(deleted.status(), StatusCode::OK);
    assert_eq!(json_body(deleted).await?["deleted"], true);
    Ok(())
}

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
    let task = json!({
        "input_path": fixture.input, "output_path": fixture.output,
        "workflow": "anime-upscale", "priority": 0, "source": "api", "source_reference": null
    });
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
    let worker_id = create_online_retry_worker(
        &fixture,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9),
    )
    .await?;
    let events = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/events", None)?)
        .await?;
    let mut stream = events.into_body().into_data_stream();
    let _ = stream.next().await.ok_or("missing initial refetch")??;
    let task = json!({
        "input_path": fixture.input, "output_path": fixture.output,
        "workflow": "anime-upscale", "priority": 0, "source": "api", "source_reference": null
    });
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
async fn worker_health_refresh_publishes_background_delta() -> TestResult {
    let fixture = Fixture::new().await?;
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9);
    let worker_id = create_online_retry_worker(&fixture, address).await?;
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

    let updated = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "PUT",
            "/api/settings",
            Some(&json!({
                "version": 0,
                "scheduler": settings["scheduler"],
                "timeouts": {
                    "health_seconds": 11,
                    "poll_seconds": 7,
                    "transfer_seconds": 301
                },
                "retry": {
                    "initial_seconds": 2,
                    "maximum_seconds": 30,
                    "max_attempts": 4
                }
            })),
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

async fn create_and_cancel_task(fixture: &Fixture) -> TestResult {
    let task = json!({
        "input_path": fixture.input, "output_path": fixture.output,
        "workflow": "anime-upscale", "priority": 0, "source": "api", "source_reference": null
    });
    let mut request = Fixture::request("POST", "/api/tasks", Some(&task))?;
    request
        .headers_mut()
        .insert("idempotency-key", "task-14-cancel".parse()?);
    let created = fixture.router.clone().oneshot(request).await?;
    let created = json_body(created).await?;
    let task_id = created["id"].as_str().ok_or("task id missing")?;
    let cancel_uri = format!("/api/tasks/{task_id}/cancel");
    let cancelled = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            &cancel_uri,
            Some(&json!({"version": 0})),
        )?)
        .await?;
    assert_eq!(cancelled.status(), StatusCode::OK);
    assert_eq!(json_body(cancelled).await?["status"], "cancelled");
    Ok(())
}

#[tokio::test]
async fn cookie_mutations_require_same_origin_csrf_proof() -> TestResult {
    let fixture = Fixture::new().await?;
    let mut login = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(
            &json!({"password": PASSWORD}),
        )?))?;
    login.extensions_mut().insert(ConnectInfo(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        40123,
    )));
    let response = fixture.router.clone().oneshot(login).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .ok_or("session cookie missing")?
        .to_str()?
        .split(';')
        .next()
        .ok_or("session cookie value missing")?
        .to_owned();
    let csrf = response
        .headers()
        .get("x-csrf-token")
        .ok_or("csrf proof missing")?
        .to_str()?
        .to_owned();
    let worker = json!({
        "name": "session-worker", "api_url": "https://session-worker.example/api/",
        "enabled": true, "compute_slots": 1
    });

    let mut forbidden = Request::builder()
        .method("POST")
        .uri("/api/workers")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, &cookie)
        .body(Body::from(serde_json::to_vec(&worker)?))?;
    forbidden
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            40_000,
        )));
    let forbidden = fixture.router.clone().oneshot(forbidden).await?;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    let mut accepted = Request::builder()
        .method("POST")
        .uri("/api/workers")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, cookie)
        .header(header::HOST, "controller.test")
        .header(header::ORIGIN, "http://controller.test")
        .header("x-csrf-token", csrf)
        .body(Body::from(serde_json::to_vec(&worker)?))?;
    accepted
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            40_000,
        )));
    let accepted = fixture.router.clone().oneshot(accepted).await?;
    assert_eq!(accepted.status(), StatusCode::CREATED);
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
    let invalid_update = json!({
        "version": settings["version"],
        "scheduler": settings["scheduler"],
        "timeouts": {
            "health_seconds": 0,
            "poll_seconds": settings["timeouts"]["poll_seconds"],
            "transfer_seconds": settings["timeouts"]["transfer_seconds"]
        },
        "retry": settings["retry"]
    });
    let invalid = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "PUT",
            "/api/settings",
            Some(&invalid_update),
        )?)
        .await?;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(invalid).await?["error"]["field_errors"][0]["field"],
        "health_seconds"
    );
    let excessive_update = json!({
        "version": settings["version"],
        "scheduler": settings["scheduler"],
        "timeouts": {
            "health_seconds": settings["timeouts"]["health_seconds"],
            "poll_seconds": settings["timeouts"]["poll_seconds"],
            "transfer_seconds": 604_801
        },
        "retry": settings["retry"]
    });
    let excessive = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "PUT",
            "/api/settings",
            Some(&excessive_update),
        )?)
        .await?;
    assert_eq!(excessive.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(excessive).await?["error"]["field_errors"][0]["field"],
        "transfer_seconds"
    );

    let task = json!({
        "input_path": fixture.input, "output_path": fixture.output,
        "workflow": "anime-upscale", "priority": 0, "source": "api", "source_reference": null
    });
    let mut request = Fixture::request("POST", "/api/tasks", Some(&task))?;
    request
        .headers_mut()
        .insert("idempotency-key", "task-14-retry".parse()?);
    let created = json_body(fixture.router.clone().oneshot(request).await?).await?;
    let task_id = created["id"].as_str().ok_or("task id missing")?;
    let cancel_uri = format!("/api/tasks/{task_id}/cancel");
    let cancelled = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            &cancel_uri,
            Some(&json!({"version": 0})),
        )?)
        .await?;
    assert_eq!(cancelled.status(), StatusCode::OK);
    let retry_uri = format!("/api/tasks/{task_id}/retry");
    let retry = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            &retry_uri,
            Some(&json!({"version": 1})),
        )?)
        .await?;
    assert_eq!(retry.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(retry).await?["error"]["code"], "conflict");
    Ok(())
}

#[tokio::test]
async fn readiness_fails_when_a_retained_root_is_replaced() -> TestResult {
    let fixture = Fixture::new().await?;
    let input_root = fixture
        .input
        .parent()
        .ok_or("input root missing")?
        .to_path_buf();
    let moved = input_root.with_extension("replaced");
    fs::rename(&input_root, &moved)?;
    fs::create_dir(&input_root)?;

    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/readiness", None)?)
        .await?;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let readiness = json_body(response).await?;
    assert_eq!(readiness["status"], "not_ready");
    assert_eq!(readiness["checks"][2]["name"], "root_handles");
    assert_eq!(readiness["checks"][2]["ready"], false);
    Ok(())
}

#[tokio::test]
async fn readiness_reports_invalid_authentication_material() -> TestResult {
    let fixture = Fixture::new().await?;
    fs::write(&fixture.hash_file, "invalid-password-hash")?;

    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/readiness", None)?)
        .await?;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let readiness = json_body(response).await?;
    assert_eq!(readiness["status"], "not_ready");
    assert_eq!(readiness["checks"][1]["name"], "authentication");
    assert_eq!(readiness["checks"][1]["ready"], false);
    Ok(())
}

#[tokio::test]
async fn processing_retry_verifies_terminal_remote_cleanup() -> TestResult {
    let fixture = Fixture::new().await?;
    let remote_job_id = RemoteJobId::random();
    let remote = retry_remote(Ok(retry_job(remote_job_id))).await?;
    let worker_id = create_online_retry_worker(&fixture, remote.address).await?;
    let task_id = create_processing_failure(&fixture, worker_id, remote_job_id).await?;
    let failed = fixture.store.task(task_id).await?.ok_or("task missing")?;
    let old_attempt = fixture
        .store
        .current_attempt(task_id)
        .await?
        .ok_or("attempt missing")?;
    let retry_uri = format!("/api/tasks/{task_id}/retry");
    let retried = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            &retry_uri,
            Some(&json!({"version": failed.version})),
        )?)
        .await?;
    assert_eq!(retried.status(), StatusCode::OK);
    let response = json_body(retried).await?;
    assert_eq!(response["status"], "reserved");
    let new_attempt = fixture
        .store
        .current_attempt(task_id)
        .await?
        .ok_or("new attempt missing")?;
    assert_ne!(new_attempt.attempt.id, old_attempt.attempt.id);
    assert_eq!(response["attempt_id"], new_attempt.attempt.id.to_string());
    assert_eq!(fixture.store.task_attempts(task_id, 10).await?.len(), 2);
    assert_eq!(remote.workspace_deletes.load(Ordering::SeqCst), 1);
    assert_eq!(remote.job_deletes.load(Ordering::SeqCst), 0);
    remote.server.abort();
    Ok(())
}

#[tokio::test]
async fn cancellation_publishes_exactly_one_task_delta() -> TestResult {
    // Given: a queued task and an SSE subscriber caught up to current state.
    let fixture = Fixture::new().await?;
    let task_id = create_api_task(&fixture, "task-14-cancel-event").await?;
    let events = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/events", None)?)
        .await?;
    let mut stream = events.into_body().into_data_stream();
    let _initial = stream.next().await.ok_or("missing initial refetch")??;

    // When: cancellation is committed through the HTTP API.
    let cancelled = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            &format!("/api/tasks/{task_id}/cancel"),
            Some(&json!({"version": 0})),
        )?)
        .await?;

    // Then: the subscriber receives one task delta and no duplicate publication.
    assert_eq!(cancelled.status(), StatusCode::OK);
    let event = stream.next().await.ok_or("missing cancellation delta")??;
    assert!(String::from_utf8_lossy(&event).contains("event: task_updated"));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), stream.next())
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn processing_retry_publishes_exactly_one_task_delta() -> TestResult {
    // Given: a retryable processing failure and an SSE subscriber caught up to current state.
    let fixture = Fixture::new().await?;
    let remote_job_id = RemoteJobId::random();
    let remote = retry_remote(Ok(retry_job(remote_job_id))).await?;
    let worker_id = create_online_retry_worker(&fixture, remote.address).await?;
    let task_id = create_processing_failure(&fixture, worker_id, remote_job_id).await?;
    let failed = fixture.store.task(task_id).await?.ok_or("task missing")?;
    let events = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/events", None)?)
        .await?;
    let mut stream = events.into_body().into_data_stream();
    let _initial = stream.next().await.ok_or("missing initial refetch")??;

    // When: a replacement processing attempt is committed through the HTTP API.
    let retried = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            &format!("/api/tasks/{task_id}/retry"),
            Some(&json!({"version": failed.version})),
        )?)
        .await?;

    // Then: the subscriber receives one task delta and no duplicate publication.
    assert_eq!(retried.status(), StatusCode::OK);
    let event = stream.next().await.ok_or("missing retry delta")??;
    assert!(String::from_utf8_lossy(&event).contains("event: task_updated"));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), stream.next())
            .await
            .is_err()
    );
    remote.server.abort();
    Ok(())
}

#[tokio::test]
async fn cancellation_response_does_not_depend_on_post_commit_reload() -> TestResult {
    // Given: a queued task whose row becomes unreadable only after an update commits.
    let fixture = Fixture::new().await?;
    let task_id = create_api_task(&fixture, "task-14-cancel-post-commit").await?;
    install_post_update_corruption(&fixture).await?;

    // When: cancellation commits successfully.
    let cancelled = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            &format!("/api/tasks/{task_id}/cancel"),
            Some(&json!({"version": 0})),
        )?)
        .await?;

    // Then: the response reports committed success without a fallible reload.
    assert_eq!(cancelled.status(), StatusCode::OK);
    assert_eq!(json_body(cancelled).await?["status"], "cancelled");
    Ok(())
}

#[tokio::test]
async fn processing_retry_response_does_not_depend_on_post_commit_reload() -> TestResult {
    // Given: a retryable processing failure whose row becomes unreadable after retry commits.
    let fixture = Fixture::new().await?;
    let remote_job_id = RemoteJobId::random();
    let remote = retry_remote(Ok(retry_job(remote_job_id))).await?;
    let worker_id = create_online_retry_worker(&fixture, remote.address).await?;
    let task_id = create_processing_failure(&fixture, worker_id, remote_job_id).await?;
    let failed = fixture.store.task(task_id).await?.ok_or("task missing")?;
    install_post_update_corruption(&fixture).await?;

    // When: the replacement attempt commits successfully.
    let retried = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            &format!("/api/tasks/{task_id}/retry"),
            Some(&json!({"version": failed.version})),
        )?)
        .await?;

    // Then: the response reports committed success without a fallible reload.
    assert_eq!(retried.status(), StatusCode::OK);
    assert_eq!(json_body(retried).await?["status"], "reserved");
    remote.server.abort();
    Ok(())
}

#[tokio::test]
async fn processing_retry_rejects_wrong_remote_job_id() -> TestResult {
    assert_processing_retry_rejected(RetryEvidenceFault::WrongJobId).await
}

#[tokio::test]
async fn processing_retry_rejects_wrong_remote_workflow() -> TestResult {
    assert_processing_retry_rejected(RetryEvidenceFault::WrongWorkflow).await
}

#[tokio::test]
async fn processing_retry_rejects_wrong_remote_input() -> TestResult {
    assert_processing_retry_rejected(RetryEvidenceFault::WrongInput).await
}

#[tokio::test]
async fn processing_retry_rejects_wrong_remote_output() -> TestResult {
    assert_processing_retry_rejected(RetryEvidenceFault::WrongOutput).await
}

#[tokio::test]
async fn processing_retry_rejects_missing_remote_params() -> TestResult {
    assert_processing_retry_rejected(RetryEvidenceFault::MissingParams).await
}

#[tokio::test]
async fn processing_retry_rejects_null_remote_params() -> TestResult {
    assert_processing_retry_rejected(RetryEvidenceFault::NullParams).await
}

#[tokio::test]
async fn processing_retry_rejects_nonterminal_remote_job() -> TestResult {
    assert_processing_retry_rejected(RetryEvidenceFault::Nonterminal).await
}

#[tokio::test]
async fn processing_retry_rejects_missing_remote_job() -> TestResult {
    assert_processing_retry_rejected(RetryEvidenceFault::NotFound).await
}

#[tokio::test]
async fn processing_retry_reports_unavailable_remote_worker() -> TestResult {
    assert_processing_retry_rejected(RetryEvidenceFault::Unavailable).await
}

fn retry_job(remote_job_id: RemoteJobId) -> Value {
    json!({
        "id": remote_job_id,
        "status": "cancelled",
        "created_at": "2026-09-03T00:00:00Z",
        "started_at": "2026-09-03T00:00:01Z",
        "completed_at": "2026-09-03T00:00:02Z",
        "progress": null,
        "error": null,
        "workflow_name": "anime-upscale",
        "workflow_source": "test",
        "params": {"input": "task/input.mkv", "output": "task/output.mp4"},
        "rerun_of_job_id": null,
        "duration_ms": 1000
    })
}

async fn assert_processing_retry_rejected(fault: RetryEvidenceFault) -> TestResult {
    // Given: durable processing evidence and a contradictory, incomplete, or unavailable remote.
    let fixture = Fixture::new().await?;
    let remote_job_id = RemoteJobId::random();
    let mut job = retry_job(remote_job_id);
    let (response, expected_status, expected_code) = match fault {
        RetryEvidenceFault::WrongJobId => {
            job["id"] = json!(RemoteJobId::random());
            (Ok(job), StatusCode::CONFLICT, "remote_state_ambiguous")
        }
        RetryEvidenceFault::WrongWorkflow => {
            job["workflow_name"] = json!("other-workflow");
            (Ok(job), StatusCode::CONFLICT, "remote_state_ambiguous")
        }
        RetryEvidenceFault::WrongInput => {
            job["params"]["input"] = json!("other/input.mkv");
            (Ok(job), StatusCode::CONFLICT, "remote_state_ambiguous")
        }
        RetryEvidenceFault::WrongOutput => {
            job["params"]["output"] = json!("other/output.mp4");
            (Ok(job), StatusCode::CONFLICT, "remote_state_ambiguous")
        }
        RetryEvidenceFault::MissingParams => {
            job.as_object_mut()
                .ok_or("job object missing")?
                .remove("params");
            (Ok(job), StatusCode::CONFLICT, "remote_state_ambiguous")
        }
        RetryEvidenceFault::NullParams => {
            job["params"] = Value::Null;
            (Ok(job), StatusCode::CONFLICT, "remote_state_ambiguous")
        }
        RetryEvidenceFault::Nonterminal => {
            job["status"] = json!("running");
            (Ok(job), StatusCode::CONFLICT, "conflict")
        }
        RetryEvidenceFault::NotFound => (
            Err(StatusCode::NOT_FOUND),
            StatusCode::CONFLICT,
            "remote_state_ambiguous",
        ),
        RetryEvidenceFault::Unavailable => (
            Err(StatusCode::SERVICE_UNAVAILABLE),
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
        ),
    };
    let remote = retry_remote(response).await?;
    let worker_id = create_online_retry_worker(&fixture, remote.address).await?;
    let task_id = create_processing_failure(&fixture, worker_id, remote_job_id).await?;
    let failed = fixture.store.task(task_id).await?.ok_or("task missing")?;

    // When: retry is requested for the failed processing attempt.
    let retry = fixture
        .router
        .clone()
        .oneshot(Fixture::request(
            "POST",
            &format!("/api/tasks/{task_id}/retry"),
            Some(&json!({"version": failed.version})),
        )?)
        .await?;

    // Then: cleanup and replacement attempt creation do not occur.
    assert_eq!(retry.status(), expected_status);
    assert_eq!(json_body(retry).await?["error"]["code"], expected_code);
    assert_eq!(remote.workspace_deletes.load(Ordering::SeqCst), 0);
    assert_eq!(remote.job_deletes.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.store.task_attempts(task_id, 10).await?.len(), 1);
    remote.server.abort();
    Ok(())
}

async fn retry_remote(response: Result<Value, StatusCode>) -> TestResult<RetryRemote> {
    let job_deletes = Arc::new(AtomicUsize::new(0));
    let job_delete_count = Arc::clone(&job_deletes);
    let workspace_deletes = Arc::new(AtomicUsize::new(0));
    let workspace_delete_count = Arc::clone(&workspace_deletes);
    let app = Router::new()
        .route(
            "/api/jobs/{id}",
            get(move || {
                let response = response.clone();
                async move {
                    match response {
                        Ok(job) => Json(job).into_response(),
                        Err(status) => status.into_response(),
                    }
                }
            })
            .delete(move || async move {
                job_delete_count.fetch_add(1, Ordering::SeqCst);
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/api/files/{task_id}",
            axum::routing::delete(move || async move {
                workspace_delete_count.fetch_add(1, Ordering::SeqCst);
                StatusCode::NO_CONTENT
            }),
        );
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    Ok(RetryRemote {
        address,
        job_deletes,
        workspace_deletes,
        server,
    })
}

async fn create_api_task(fixture: &Fixture, idempotency_key: &str) -> TestResult<TaskId> {
    let task = json!({
        "input_path": fixture.input, "output_path": fixture.output,
        "workflow": "anime-upscale", "priority": 0, "source": "api", "source_reference": null
    });
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

async fn install_post_update_corruption(fixture: &Fixture) -> TestResult {
    sqlx::query(
        "CREATE TRIGGER task14_corrupt_progress AFTER UPDATE ON tasks
         WHEN NEW.progress_json != '{\"percent\":0,\"unexpected\":true}'
         BEGIN UPDATE tasks SET progress_json = '{\"percent\":0,\"unexpected\":true}' WHERE id = NEW.id; END",
    )
    .execute(fixture.store.database().pool())
    .await?;
    Ok(())
}

async fn create_online_retry_worker(
    fixture: &Fixture,
    address: SocketAddr,
) -> TestResult<videnoa_controller::domain::WorkerId> {
    let worker = json!({
        "name": "retry-worker",
        "api_url": format!("http://{address}/"),
        "enabled": true,
        "compute_slots": 1
    });
    let created_worker = fixture
        .router
        .clone()
        .oneshot(Fixture::request("POST", "/api/workers", Some(&worker))?)
        .await?;
    let worker_id: videnoa_controller::domain::WorkerId = json_body(created_worker).await?["id"]
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

async fn create_processing_failure(
    fixture: &Fixture,
    worker_id: videnoa_controller::domain::WorkerId,
    remote_job_id: RemoteJobId,
) -> TestResult<TaskId> {
    let task = json!({
        "input_path": fixture.input, "output_path": fixture.output,
        "workflow": "anime-upscale", "priority": 0, "source": "api", "source_reference": null
    });
    let mut request = Fixture::request("POST", "/api/tasks", Some(&task))?;
    request
        .headers_mut()
        .insert("idempotency-key", "task-14-processing-retry".parse()?);
    let task_id: TaskId = json_body(fixture.router.clone().oneshot(request).await?).await?["id"]
        .as_str()
        .ok_or("task id missing")?
        .parse()?;
    let service = LifecycleService::new(fixture.store.clone());
    let attempt_id = AttemptId::random();
    service
        .reserve(&ReserveCommand {
            task_id,
            expected_task_version: 0,
            worker_id,
            attempt_id,
            submission_key: SubmissionKey::random(),
            reserved_at: chrono::Utc::now(),
        })
        .await
        .map_err(|error| std::io::Error::other(format!("reserve failed: {error}")))?;
    for command in [
        AdvanceCommand::StartUpload,
        AdvanceCommand::FinishUpload(UploadEvidence {
            remote_input_path: RemotePath::new("task/input.mkv"),
            remote_output_path: RemotePath::new("task/output.mp4"),
        }),
        AdvanceCommand::StartSubmission,
        AdvanceCommand::PersistSubmission(SubmissionEvidence {
            remote_job_id,
            remote_input_path: RemotePath::new("task/input.mkv"),
            remote_output_path: RemotePath::new("task/output.mp4"),
        }),
    ] {
        advance(&fixture.store, &service, task_id, attempt_id, command).await?;
    }
    let task = fixture.store.task(task_id).await?.ok_or("task missing")?;
    let attempt = fixture
        .store
        .attempt(attempt_id)
        .await?
        .ok_or("attempt missing")?;
    service
        .fail(
            &task,
            Some(&attempt),
            LifecycleFailure::restart_cancelled("worker restarted"),
            chrono::Utc::now(),
        )
        .await
        .map_err(|error| std::io::Error::other(format!("processing failure failed: {error}")))?;
    Ok(task_id)
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
    let settings = fixture.store.settings().await?;
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

async fn advance(
    store: &Store,
    service: &LifecycleService,
    task_id: TaskId,
    attempt_id: AttemptId,
    command: AdvanceCommand,
) -> TestResult {
    let task = store.task(task_id).await?.ok_or("task missing")?;
    let attempt = store.attempt(attempt_id).await?.ok_or("attempt missing")?;
    service
        .advance(&task, &attempt, command.clone(), chrono::Utc::now())
        .await
        .map_err(|error| std::io::Error::other(format!("advance {command:?} failed: {error}")))?;
    Ok(())
}
