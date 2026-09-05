use std::error::Error;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use axum::body::{to_bytes, Body};
use axum::extract::connect_info::ConnectInfo;
use axum::http::{header, Request};
use axum::Router;
use chrono::Utc;
use serde_json::Value;
use tempfile::TempDir;
use videnoa_controller::auth::{hash_password, AuthService};
use videnoa_controller::config::ConfigBootstrap;
use videnoa_controller::operations::{EventHub, OperationsDependencies, OperationsState};
use videnoa_controller::paths::PathCapabilities;
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};
use videnoa_controller::remote::PayloadLimits;
use videnoa_controller::scheduler::Scheduler;
use videnoa_controller::tasks::TaskService;
use videnoa_controller::{controller_app_router, FrontendAssets};

pub type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
pub const PASSWORD: &str = "test-only-operations-password";

pub struct Fixture {
    pub _directory: TempDir,
    pub router: Router,
    pub input: PathBuf,
    pub output: PathBuf,
    pub scheduler: Scheduler,
    pub store: Store,
    pub workspace: PathBuf,
    pub config_file: PathBuf,
}

impl Fixture {
    pub async fn new() -> TestResult<Self> {
        let directory = TempDir::new()?;
        let workspace = directory.path().canonicalize()?;
        let input = workspace.join("input/source.mkv");
        let output = workspace.join("output/result.mp4");
        fs::create_dir(workspace.join("input"))?;
        fs::create_dir(workspace.join("output"))?;
        fs::write(&input, b"synthetic video fixture")?;

        let bootstrap = ConfigBootstrap::open(&workspace)?;
        let database = Database::open(DatabaseOptions::new(
            bootstrap
                .config()
                .paths
                .data_root
                .join("controller.sqlite3"),
        ))
        .await?;
        let store = Store::new(database);
        let config = bootstrap.initialize(&store)?;
        let password_hash = hash_password(PASSWORD)?;
        if !store
            .insert_administrator_credential(&password_hash, Utc::now())
            .await?
        {
            return Err(std::io::Error::other("administrator fixture already exists").into());
        }
        let auth = AuthService::new(config.auth.clone(), store.clone())?;
        let paths = PathCapabilities::open(&config.paths)?;
        let scheduler = Scheduler::load(store.clone())?;
        let events = EventHub::new();
        let operations = OperationsState::new(OperationsDependencies {
            auth: auth.clone(),
            store: store.clone(),
            scheduler: scheduler.clone(),
            paths: paths.clone(),
            config,
            events,
            payload_limits: PayloadLimits::new(1024 * 1024, 4096)?,
        });
        let tasks = TaskService::new(store.clone(), paths);
        let router = controller_app_router(&assets(&workspace)?, auth, tasks, operations);
        Ok(Self {
            config_file: bootstrap.config_file().to_path_buf(),
            _directory: directory,
            router,
            input,
            output,
            scheduler,
            store,
            workspace,
        })
    }

    pub fn request(method: &str, uri: &str, body: Option<&Value>) -> TestResult<Request<Body>> {
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
        Ok(connected_request(builder.body(body)?, 40_000))
    }
}

pub fn connected_request(mut request: Request<Body>, port: u16) -> Request<Body> {
    request.extensions_mut().insert(ConnectInfo(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        port,
    )));
    request
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

pub async fn json_body(response: axum::response::Response) -> TestResult<Value> {
    Ok(serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX).await?,
    )?)
}
