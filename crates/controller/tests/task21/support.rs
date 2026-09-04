use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Request};
use axum::Router;
use tempfile::TempDir;
use videnoa_controller::auth::{hash_password, AuthService};
use videnoa_controller::config::{AuthConfig, ControllerConfig, PathConfig};
use videnoa_controller::operations::{EventHub, OperationsDependencies, OperationsState};
use videnoa_controller::paths::PathCapabilities;
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};
use videnoa_controller::remote::PayloadLimits;
use videnoa_controller::scheduler::Scheduler;
use videnoa_controller::tasks::TaskService;
use videnoa_controller::{controller_app_router, FrontendAssets};

pub type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

pub const PASSWORD: &str = "task-21-transient-password";

pub struct Fixture {
    pub _directory: TempDir,
    pub input_root: PathBuf,
    pub output_root: PathBuf,
    pub router: Router,
    pub store: Store,
}

pub async fn fixture() -> TestResult<Fixture> {
    let directory = TempDir::new()?;
    let input_root = directory.path().join("input");
    let output_root = directory.path().join("output");
    let data_root = directory.path().join("data");
    let temp_root = directory.path().join("temp");
    for root in [&input_root, &output_root, &data_root, &temp_root] {
        fs::create_dir(root)?;
    }
    let hash_file = directory.path().join("admin-password.phc");
    fs::write(&hash_file, hash_password(PASSWORD)?)?;
    let database = Database::open(
        DatabaseOptions::new(directory.path().join("controller.sqlite3")).with_max_connections(8),
    )
    .await?;
    let store = Store::new(database);
    let auth_config = AuthConfig {
        password_hash_file: hash_file,
        secure_cookie: false,
        session_absolute: Duration::from_secs(86_400),
        session_idle: Duration::from_secs(3_600),
    };
    let path_config = PathConfig {
        input_roots: vec![input_root.clone()],
        output_roots: vec![output_root.clone()],
        data_root,
        temp_root,
    };
    let auth = AuthService::new(auth_config.clone(), store.clone())?;
    let paths = PathCapabilities::open(&path_config)?;
    let scheduler = Scheduler::load(store.clone()).await?;
    let operations = OperationsState::new(OperationsDependencies {
        auth: auth.clone(),
        store: store.clone(),
        scheduler,
        paths: paths.clone(),
        config: ControllerConfig {
            auth: auth_config,
            paths: path_config,
            ..ControllerConfig::default()
        },
        events: EventHub::new(),
        payload_limits: PayloadLimits::new(1024 * 1024, 4096)?,
    });
    let tasks = TaskService::new(store.clone(), paths);
    let router = controller_app_router(&assets(directory.path())?, auth, tasks, operations);
    Ok(Fixture {
        _directory: directory,
        input_root,
        output_root,
        router,
        store,
    })
}

pub fn request(uri: &str) -> TestResult<Request<Body>> {
    Ok(Request::builder()
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {PASSWORD}"))
        .body(Body::empty())?)
}

pub fn json_request(
    method: &str,
    uri: &str,
    body: &serde_json::Value,
) -> TestResult<Request<Body>> {
    Ok(Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {PASSWORD}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(body)?))?)
}

#[cfg(debug_assertions)]
fn assets(directory: &Path) -> TestResult<FrontendAssets> {
    let root = directory.join("assets");
    fs::create_dir(&root)?;
    fs::write(root.join("index.html"), "<main>task 21</main>")?;
    Ok(FrontendAssets::from_dist(root)?)
}

#[cfg(not(debug_assertions))]
fn assets(_: &Path) -> TestResult<FrontendAssets> {
    Ok(FrontendAssets::embedded()?)
}
