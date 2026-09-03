use std::error::Error;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::extract::connect_info::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use chrono::{Duration as ChronoDuration, Utc};
use tempfile::TempDir;
use tower::ServiceExt;
use videnoa_controller::auth::{hash_password, AuthService, CSRF_HEADER, SESSION_COOKIE};
use videnoa_controller::config::AuthConfig;
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};
use videnoa_controller::{authenticated_app_router, FrontendAssets};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
const PASSWORD: &str = "test-only-admin-password";

struct Fixture {
    _directory: TempDir,
    hash_file: std::path::PathBuf,
    assets: FrontendAssets,
    auth: AuthService,
}

impl Fixture {
    async fn new() -> TestResult<Self> {
        let directory = TempDir::new()?;
        let hash_file = directory.path().join("admin-password.phc");
        fs::write(&hash_file, hash_password(PASSWORD)?)?;
        let database = Database::open(DatabaseOptions::new(
            directory.path().join("controller.sqlite3"),
        ))
        .await?;
        let auth = AuthService::new(
            AuthConfig {
                password_hash_file: hash_file.clone(),
                secure_cookie: false,
                session_absolute: Duration::from_secs(86_400),
                session_idle: Duration::from_secs(3_600),
            },
            Store::new(database),
        )?;
        let assets = test_frontend_assets(directory.path())?;
        Ok(Self {
            _directory: directory,
            hash_file,
            assets,
            auth,
        })
    }

    fn router(&self) -> axum::Router {
        authenticated_app_router(&self.assets, self.auth.clone())
    }
}

#[cfg(debug_assertions)]
fn test_frontend_assets(directory: &std::path::Path) -> TestResult<FrontendAssets> {
    let assets = directory.join("assets");
    fs::create_dir(&assets)?;
    fs::write(assets.join("index.html"), "<main>controller</main>")?;
    Ok(FrontendAssets::from_dist(assets)?)
}

#[cfg(not(debug_assertions))]
fn test_frontend_assets(_: &std::path::Path) -> TestResult<FrontendAssets> {
    Ok(FrontendAssets::embedded()?)
}

fn request(method: &str, uri: &str, body: Body) -> TestResult<Request<Body>> {
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

async fn login(fixture: &Fixture, password: &str) -> TestResult<(String, String)> {
    let body = serde_json::to_vec(&serde_json::json!({"password": password}))?;
    let response = fixture
        .router()
        .oneshot(request("POST", "/api/auth/login", Body::from(body))?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .ok_or_else(|| std::io::Error::other("login omitted session cookie"))?
        .to_str()?
        .split(';')
        .next()
        .ok_or_else(|| std::io::Error::other("session cookie is empty"))?
        .to_owned();
    let set_cookie = response.headers()[header::SET_COOKIE].to_str()?;
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("SameSite=Strict"));
    assert!(set_cookie.contains("Path=/"));
    assert!(set_cookie.contains("Max-Age=86400"));
    assert!(!set_cookie.contains("Secure"));
    let csrf = response
        .headers()
        .get(CSRF_HEADER)
        .ok_or_else(|| std::io::Error::other("login omitted CSRF proof"))?
        .to_str()?
        .to_owned();
    let bytes = to_bytes(response.into_body(), 64 * 1024).await?;
    assert!(!bytes
        .windows(PASSWORD.len())
        .any(|part| part == PASSWORD.as_bytes()));
    Ok((cookie, csrf))
}

#[tokio::test]
async fn protected_routes_reject_missing_auth_and_bearer_logout_is_csrf_exempt() -> TestResult {
    // Given: protected session, readiness, and logout routes.
    let fixture = Fixture::new().await?;

    // When: anonymous requests and a bearer-authenticated logout are sent.
    for (method, uri) in [
        ("GET", "/api/auth/session"),
        ("GET", "/api/readiness"),
        ("POST", "/api/auth/logout"),
    ] {
        let response = fixture
            .router()
            .oneshot(request(method, uri, Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let mut bearer = request("POST", "/api/auth/logout", Body::empty())?;
    bearer
        .headers_mut()
        .insert(header::AUTHORIZATION, format!("Bearer {PASSWORD}").parse()?);
    let bearer = fixture.router().oneshot(bearer).await?;

    // Then: all protected routes reject anonymous access while bearer auth needs no CSRF proof.
    assert_eq!(bearer.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn unauthorized_session_check_expires_the_existing_cookie() -> TestResult {
    // Given: a cookie session invalidated by administrator credential rotation.
    let fixture = Fixture::new().await?;
    let (cookie, _) = login(&fixture, PASSWORD).await?;
    fs::write(&fixture.hash_file, hash_password("rotated-test-password")?)?;
    let mut session = request("GET", "/api/auth/session", Body::empty())?;
    session
        .headers_mut()
        .insert(header::COOKIE, cookie.parse()?);

    // When: the browser checks the invalid session through the real HTTP handler.
    let response = fixture.router().oneshot(session).await?;

    // Then: the typed 401 response also expires the original session cookie contract.
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let set_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .ok_or_else(|| {
            std::io::Error::other("unauthorized session response omitted expired cookie")
        })?
        .to_str()?;
    assert!(set_cookie.starts_with(&format!("{SESSION_COOKIE}=;")));
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("SameSite=Strict"));
    assert!(set_cookie.contains("Path=/"));
    assert!(set_cookie.contains("Max-Age=0"));
    assert!(!set_cookie.contains("Secure"));
    let body = to_bytes(response.into_body(), 4096).await?;
    assert_eq!(body.as_ref(), br#"{"error":"unauthorized"}"#);
    Ok(())
}

#[tokio::test]
async fn session_login_requires_csrf_and_same_origin_for_logout() -> TestResult {
    // Given: a valid administrator login.
    let fixture = Fixture::new().await?;
    let (cookie, csrf) = login(&fixture, PASSWORD).await?;

    // When: logout is attempted without proof, cross-origin, and correctly.
    let missing = fixture
        .router()
        .oneshot(request("POST", "/api/auth/logout", Body::empty())?)
        .await?;
    let mut cross_origin = request("POST", "/api/auth/logout", Body::empty())?;
    cross_origin
        .headers_mut()
        .insert(header::COOKIE, cookie.parse()?);
    cross_origin
        .headers_mut()
        .insert(CSRF_HEADER, csrf.parse()?);
    cross_origin
        .headers_mut()
        .insert(header::ORIGIN, "http://attacker.test".parse()?);
    let cross_origin = fixture.router().oneshot(cross_origin).await?;
    let mut valid = request("POST", "/api/auth/logout", Body::empty())?;
    valid.headers_mut().insert(header::COOKIE, cookie.parse()?);
    valid.headers_mut().insert(CSRF_HEADER, csrf.parse()?);
    valid
        .headers_mut()
        .insert(header::ORIGIN, "http://controller.test".parse()?);
    let valid = fixture.router().oneshot(valid).await?;

    // Then: only the same-origin request with the matching CSRF proof mutates the cookie session.
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(cross_origin.status(), StatusCode::FORBIDDEN);
    assert_eq!(valid.status(), StatusCode::OK);
    assert!(valid.headers()[header::SET_COOKIE]
        .to_str()?
        .starts_with(&format!("{SESSION_COOKIE}=;")));
    Ok(())
}

#[tokio::test]
async fn sixth_failed_login_is_throttled_without_reflecting_credentials() -> TestResult {
    // Given: one direct client address and an invalid credential.
    let fixture = Fixture::new().await?;

    // When: that client submits six failed logins inside the five-minute window.
    let mut statuses = Vec::new();
    for _ in 0..6 {
        let body = serde_json::to_vec(&serde_json::json!({"password": "wrong-secret"}))?;
        let response = fixture
            .router()
            .oneshot(request("POST", "/api/auth/login", Body::from(body))?)
            .await?;
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 4096).await?;
        assert!(!bytes.windows(12).any(|part| part == b"wrong-secret"));
        statuses.push(status);
    }

    // Then: five attempts are indistinguishable unauthorized responses and the sixth is limited.
    assert_eq!(&statuses[..5], &[StatusCode::UNAUTHORIZED; 5]);
    assert_eq!(statuses[5], StatusCode::TOO_MANY_REQUESTS);
    Ok(())
}

#[tokio::test]
async fn hash_rotation_invalidates_sessions_and_bearer_uses_current_password() -> TestResult {
    // Given: a session issued under the original password hash.
    let fixture = Fixture::new().await?;
    let (cookie, _) = login(&fixture, PASSWORD).await?;
    let rotated_credential = "rotated-test-password";
    fs::write(&fixture.hash_file, hash_password(rotated_credential)?)?;

    // When: the old session, old bearer, and rotated bearer request readiness.
    let mut session = request("GET", "/api/readiness", Body::empty())?;
    session
        .headers_mut()
        .insert(header::COOKIE, cookie.parse()?);
    let session = fixture.router().oneshot(session).await?;
    let mut old_bearer = request("GET", "/api/readiness", Body::empty())?;
    old_bearer
        .headers_mut()
        .insert(header::AUTHORIZATION, format!("Bearer {PASSWORD}").parse()?);
    let old_bearer = fixture.router().oneshot(old_bearer).await?;
    let mut current_bearer = request("GET", "/api/readiness", Body::empty())?;
    current_bearer.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {rotated_credential}").parse()?,
    );
    let current_bearer = fixture.router().oneshot(current_bearer).await?;

    // Then: rotation closes both old authentication paths without restarting the service.
    assert_eq!(session.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(old_bearer.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(current_bearer.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn expired_session_is_rejected_without_waiting_for_wall_clock() -> TestResult {
    // Given: a valid session token and a future instant beyond its absolute lifetime.
    let fixture = Fixture::new().await?;
    let (cookie, _) = login(&fixture, PASSWORD).await?;
    let token = cookie
        .strip_prefix(&format!("{SESSION_COOKIE}="))
        .ok_or_else(|| std::io::Error::other("unexpected cookie name"))?;

    // When: session authentication is evaluated after the configured absolute expiry.
    let result = fixture
        .auth
        .authenticate_session_at(token, Utc::now() + ChronoDuration::days(2))
        .await;

    // Then: the durable session no longer authenticates.
    assert!(result.is_err());
    Ok(())
}

#[tokio::test]
async fn passive_session_validation_does_not_extend_idle_expiry() -> TestResult {
    let fixture = Fixture::new().await?;
    let (cookie, _) = login(&fixture, PASSWORD).await?;
    let token = cookie
        .strip_prefix(&format!("{SESSION_COOKIE}="))
        .ok_or_else(|| std::io::Error::other("unexpected cookie name"))?;
    let touched = fixture
        .auth
        .authenticate_session_at(token, Utc::now())
        .await?;
    let baseline = fixture
        .auth
        .validate_session_at(token, touched.last_used_at)
        .await?;

    let validated = fixture
        .auth
        .validate_session_at(token, baseline.last_used_at + ChronoDuration::minutes(10))
        .await?;

    assert_eq!(validated.last_used_at, baseline.last_used_at);
    assert_eq!(validated.idle_expires_at, baseline.idle_expires_at);
    Ok(())
}
