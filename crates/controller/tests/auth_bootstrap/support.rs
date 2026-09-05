use std::error::Error;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::extract::connect_info::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use tempfile::TempDir;
use tower::ServiceExt;
use videnoa_controller::auth::{AuthService, CSRF_HEADER};
use videnoa_controller::config::ControllerConfig;
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};
use videnoa_controller::{authenticated_app_router, FrontendAssets};

pub type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
pub const PASSWORD: &str = "test-only-bootstrap-password";

pub struct Fixture {
    _directory: TempDir,
    pub database_path: PathBuf,
    pub assets: FrontendAssets,
    pub auth: AuthService,
}

impl Fixture {
    pub async fn new() -> TestResult<Self> {
        let directory = TempDir::new()?;
        let database_path = directory.path().join("controller.sqlite3");
        let database = Database::open(DatabaseOptions::new(&database_path)).await?;
        let mut config = ControllerConfig::default().auth;
        config.secure_cookie = false;
        config.session_absolute = Duration::from_secs(86_400);
        config.session_idle = Duration::from_secs(3_600);
        let auth = AuthService::new(config, Store::new(database))?;
        let assets = test_frontend_assets(directory.path())?;
        Ok(Self {
            _directory: directory,
            database_path,
            assets,
            auth,
        })
    }

    pub fn router(&self) -> axum::Router {
        authenticated_app_router(&self.assets, self.auth.clone())
    }
}

#[cfg(debug_assertions)]
fn test_frontend_assets(directory: &Path) -> TestResult<FrontendAssets> {
    let assets = directory.join("assets");
    fs::create_dir(&assets)?;
    fs::write(assets.join("index.html"), "<main>controller</main>")?;
    Ok(FrontendAssets::from_dist(assets)?)
}

#[cfg(not(debug_assertions))]
fn test_frontend_assets(_: &Path) -> TestResult<FrontendAssets> {
    Ok(FrontendAssets::embedded()?)
}

pub fn request(method: &str, uri: &str, body: Body) -> TestResult<Request<Body>> {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "controller.test")
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)?;
    request.extensions_mut().insert(ConnectInfo(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        40_000,
    )));
    Ok(request)
}

pub fn setup_request(password: &str, confirmation: &str) -> TestResult<Request<Body>> {
    let body = serde_json::to_vec(&serde_json::json!({
        "password": password,
        "password_confirmation": confirmation,
    }))?;
    let mut request = request("POST", "/api/auth/setup", Body::from(body))?;
    request
        .headers_mut()
        .insert(header::ORIGIN, "http://controller.test".parse()?);
    Ok(request)
}

pub async fn setup(fixture: &Fixture) -> TestResult<(String, String)> {
    let response = fixture
        .router()
        .oneshot(setup_request(PASSWORD, PASSWORD)?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .ok_or_else(|| std::io::Error::other("setup omitted session cookie"))?
        .to_str()?
        .split(';')
        .next()
        .ok_or_else(|| std::io::Error::other("session cookie is empty"))?
        .to_owned();
    let csrf = response
        .headers()
        .get(CSRF_HEADER)
        .ok_or_else(|| std::io::Error::other("setup omitted CSRF proof"))?
        .to_str()?
        .to_owned();
    let body = to_bytes(response.into_body(), 64 * 1024).await?;
    assert!(!body
        .windows(PASSWORD.len())
        .any(|part| part == PASSWORD.as_bytes()));
    Ok((cookie, csrf))
}
