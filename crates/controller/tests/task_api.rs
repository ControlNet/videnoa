use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use tempfile::TempDir;
use tower::ServiceExt;
use videnoa_controller::auth::{hash_password, AuthService};
use videnoa_controller::config::{AuthConfig, PathConfig};
use videnoa_controller::paths::PathCapabilities;
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};
use videnoa_controller::tasks::TaskService;
use videnoa_controller::{controller_app_router, FrontendAssets};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
const PASSWORD: &str = "test-only-password";

struct Fixture {
    _directory: TempDir,
    router: Router,
    input: PathBuf,
    output: PathBuf,
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

async fn fixture() -> TestResult<Fixture> {
    let directory = TempDir::new()?;
    let input_root = directory.path().join("input");
    let output_root = directory.path().join("output");
    fs::create_dir(&input_root)?;
    fs::create_dir(&output_root)?;
    let input = input_root.join("source.MKV");
    let output = output_root.join("result.mp4");
    fs::write(&input, b"video")?;

    let hash_file = directory.path().join("admin-password.phc");
    fs::write(&hash_file, hash_password(PASSWORD)?)?;
    let database = Database::open(DatabaseOptions::new(
        directory.path().join("controller.sqlite3"),
    ))
    .await?;
    let store = Store::new(database);
    let auth = AuthService::new(
        AuthConfig {
            password_hash_file: hash_file,
            secure_cookie: false,
            session_absolute: Duration::from_secs(86_400),
            session_idle: Duration::from_secs(3_600),
        },
        store.clone(),
    )?;
    let paths = PathCapabilities::open(&PathConfig {
        input_roots: vec![input_root],
        output_roots: vec![output_root],
        data_root: directory.path().join("data"),
        temp_root: directory.path().join("temp"),
    })?;
    let tasks = TaskService::new(store, paths);
    let router = controller_app_router(&assets(directory.path())?, auth, tasks);
    Ok(Fixture {
        _directory: directory,
        router,
        input,
        output,
    })
}

fn task_request(input: &Path, output: &Path, priority: i32) -> Value {
    json!({
        "input_path": input,
        "output_path": output,
        "workflow": "anime-upscale",
        "priority": priority,
        "source": "api",
        "source_reference": "request-42"
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
    Ok(builder.body(body)?)
}

async fn json_body(response: axum::response::Response) -> TestResult<Value> {
    Ok(serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX).await?,
    )?)
}

#[tokio::test]
async fn task_routes_reject_anonymous_requests_before_api_fallback() -> TestResult {
    let fixture = fixture().await?;
    for (method, uri) in [
        ("POST", "/api/tasks"),
        ("GET", "/api/tasks"),
        ("GET", "/api/tasks/00000000-0000-4000-8000-000000000001"),
    ] {
        let response = fixture
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    Ok(())
}

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

#[tokio::test]
async fn concurrent_duplicate_intake_creates_exactly_one_task() -> TestResult {
    let fixture = fixture().await?;
    let body = task_request(&fixture.input, &fixture.output, 1);
    let mut submissions = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let router = fixture.router.clone();
        let body = body.clone();
        submissions.spawn(async move {
            let mut request = request("POST", "/api/tasks", Some(&body))?;
            request
                .headers_mut()
                .insert("idempotency-key", "concurrent-key".parse()?);
            let response = router.oneshot(request).await?;
            Ok::<_, Box<dyn Error + Send + Sync>>(response.status())
        });
    }

    let mut created = 0;
    let mut replayed = 0;
    while let Some(result) = submissions.join_next().await {
        match result?? {
            StatusCode::CREATED => created += 1,
            StatusCode::OK => replayed += 1,
            status => return Err(format!("unexpected intake status: {status}").into()),
        }
    }
    assert_eq!(created, 1);
    assert_eq!(replayed, 7);

    let response = fixture
        .router
        .oneshot(request("GET", "/api/tasks", None)?)
        .await?;
    assert_eq!(json_body(response).await?["total"], 1);
    Ok(())
}
