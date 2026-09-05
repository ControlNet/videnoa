use std::error::Error;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use axum::body::{to_bytes, Body};
use axum::extract::connect_info::ConnectInfo;
use axum::http::{header, HeaderValue, Method, Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use tempfile::TempDir;
use tower::ServiceExt;
use videnoa_controller::auth::{AuthService, CSRF_HEADER, SESSION_COOKIE};
use videnoa_controller::config::{AuthConfig, ControllerConfig, PathConfig};
use videnoa_controller::operations::{EventHub, OperationsDependencies, OperationsState};
use videnoa_controller::paths::PathCapabilities;
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};
use videnoa_controller::remote::PayloadLimits;
use videnoa_controller::scheduler::Scheduler;
use videnoa_controller::tasks::TaskService;
use videnoa_controller::{controller_app_router, FrontendAssets};

pub(super) type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
const PASSWORD: &str = "test-only-password";
// TEST ONLY: Argon2id v19, 64 KiB, one iteration/lane, salt "videnoa-test-salt".
// This precomputed PHC authenticates PASSWORD through the real production verifier.
// These deliberately weak parameters MUST NEVER be used in production.
const FAST_TEST_PASSWORD_HASH: &str = "$argon2id$v=19$m=64,t=1,p=1$dmlkZW5vYS10ZXN0LXNhbHQ$dGqvNdXw/rRAt0s9Uc4UrMmfKjwpqrGJqfwKo0aP1To";

pub(super) struct Fixture {
    _directory: TempDir,
    pub router: Router,
    pub input: PathBuf,
    pub output: PathBuf,
    pub session: SessionClient,
}

#[derive(Clone)]
pub(super) struct SessionClient {
    cookie: HeaderValue,
    csrf: HeaderValue,
}

impl SessionClient {
    async fn login(router: &Router) -> TestResult<Self> {
        let response = router
            .clone()
            .oneshot(request(
                "POST",
                "/api/auth/login",
                Some(&json!({ "password": PASSWORD })),
            )?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .ok_or("login omitted session cookie")?
            .to_str()?
            .split(';')
            .next()
            .ok_or("empty session cookie")?;
        assert!(cookie.starts_with(&format!("{SESSION_COOKIE}=")));
        let csrf = response
            .headers()
            .get(CSRF_HEADER)
            .ok_or("login omitted CSRF token")?
            .clone();
        Ok(Self {
            cookie: cookie.parse()?,
            csrf,
        })
    }

    pub fn request(
        &self,
        method: &str,
        uri: &str,
        body: Option<&Value>,
    ) -> TestResult<Request<Body>> {
        let mut request = request(method, uri, body)?;
        request
            .headers_mut()
            .insert(header::COOKIE, self.cookie.clone());
        if !matches!(
            *request.method(),
            Method::GET | Method::HEAD | Method::OPTIONS
        ) {
            request.headers_mut().insert(CSRF_HEADER, self.csrf.clone());
            request.headers_mut().insert(
                header::ORIGIN,
                HeaderValue::from_static("http://controller.test"),
            );
        }
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

pub(super) async fn fixture() -> TestResult<Fixture> {
    fixture_with_busy_timeout_option(None).await
}

pub(super) async fn fixture_with_busy_timeout(busy_timeout: Duration) -> TestResult<Fixture> {
    fixture_with_busy_timeout_option(Some(busy_timeout)).await
}

async fn fixture_with_busy_timeout_option(busy_timeout: Option<Duration>) -> TestResult<Fixture> {
    let directory = TempDir::new()?;
    let input_root = directory.path().join("input");
    let output_root = directory.path().join("output");
    let data_root = directory.path().join("data");
    let temp_root = directory.path().join("temp");
    for root in [&input_root, &output_root, &data_root, &temp_root] {
        fs::create_dir(root)?;
    }
    let input = input_root.join("source.MKV");
    let output = output_root.join("result.mp4");
    fs::write(&input, b"video")?;

    let mut database_options = DatabaseOptions::new(directory.path().join("controller.sqlite3"));
    if let Some(busy_timeout) = busy_timeout {
        // Schema creation is fixture setup, outside the request-contention regression.
        // Finish it before opening the one-connection pool with its strict 100 ms bound.
        Database::open(database_options.clone())
            .await
            .context("migrating task API fixture database")?
            .close()
            .await;
        database_options = database_options
            .with_busy_timeout(busy_timeout)
            .with_max_connections(1);
    }
    let database = Database::open(database_options)
        .await
        .context("opening task API fixture database")?;
    let store = Store::new(database);
    store
        .insert_administrator_credential(FAST_TEST_PASSWORD_HASH, chrono::Utc::now())
        .await
        .context("seeding task API fixture credential")?;
    let auth_config = AuthConfig {
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
    let auth = AuthService::new(auth_config.clone(), store.clone())?;
    let paths = PathCapabilities::open(&path_config)?;
    let scheduler = Scheduler::load(store.clone())
        .await
        .context("loading task API fixture scheduler")?;
    let config = ControllerConfig {
        auth: auth_config,
        paths: path_config,
        ..ControllerConfig::default()
    };
    let operations = OperationsState::new(OperationsDependencies {
        auth: auth.clone(),
        store: store.clone(),
        scheduler,
        paths: paths.clone(),
        config,
        events: EventHub::new(),
        payload_limits: PayloadLimits::new(1024 * 1024, 4096)?,
    });
    let tasks = TaskService::new(store, paths);
    let router = controller_app_router(&assets(directory.path())?, auth, tasks, operations);
    let session = SessionClient::login(&router)
        .await
        .map_err(|error| format!("logging in task API fixture session: {error}"))?;
    Ok(Fixture {
        _directory: directory,
        router,
        input,
        output,
        session,
    })
}

#[must_use]
pub(super) fn task_request(input: &Path, output: &Path, priority: i32) -> Value {
    json!({
        "input_path": input,
        "output_path": output,
        "workflow": "anime-upscale",
        "priority": priority,
        "source": "api",
        "source_reference": "request-42"
    })
}

pub(super) fn request(method: &str, uri: &str, body: Option<&Value>) -> TestResult<Request<Body>> {
    let mut request = request_without_peer(method, uri, body)?;
    request.extensions_mut().insert(ConnectInfo(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        40_000,
    )));
    Ok(request)
}

pub(super) fn bearer_request(
    method: &str,
    uri: &str,
    body: Option<&Value>,
) -> TestResult<Request<Body>> {
    let mut request = request(method, uri, body)?;
    request
        .headers_mut()
        .insert(header::AUTHORIZATION, format!("Bearer {PASSWORD}").parse()?);
    Ok(request)
}

fn request_without_peer(
    method: &str,
    uri: &str,
    body: Option<&Value>,
) -> TestResult<Request<Body>> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "controller.test");
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(serde_json::to_vec(value)?)
        }
        None => Body::empty(),
    };
    Ok(builder.body(body)?)
}

pub(super) async fn json_body(response: axum::response::Response) -> TestResult<Value> {
    Ok(serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX).await?,
    )?)
}
