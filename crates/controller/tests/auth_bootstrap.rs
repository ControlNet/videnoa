use axum::body::{to_bytes, Body};
use axum::http::{header, StatusCode};
use tower::ServiceExt;
use videnoa_controller::auth::{AuthService, SESSION_COOKIE};
use videnoa_controller::authenticated_app_router;
use videnoa_controller::config::ControllerConfig;
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};

#[path = "auth_bootstrap/support.rs"]
mod support;
use support::{request, setup, setup_request, Fixture, TestResult, PASSWORD};

#[tokio::test]
async fn setup_status_transitions_once_and_never_overwrites() -> TestResult {
    // Given: a Controller database without an administrator credential.
    let fixture = Fixture::new().await?;
    let initial = fixture
        .router()
        .oneshot(request("GET", "/api/auth/setup", Body::empty())?)
        .await?;
    assert_eq!(initial.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(initial.into_body(), 4096).await?.as_ref(),
        br#"{"initialized":false}"#
    );

    // When: the password is set and a second setup attempts to replace it.
    let _session = setup(&fixture).await?;
    let conflict = fixture
        .router()
        .oneshot(setup_request(
            "different-safe-password",
            "different-safe-password",
        )?)
        .await?;

    // Then: setup reports initialized, rejects replacement, and the original password logs in.
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    assert_eq!(
        to_bytes(conflict.into_body(), 4096).await?.as_ref(),
        br#"{"error":"conflict"}"#
    );
    let initialized = fixture
        .router()
        .oneshot(request("GET", "/api/auth/setup", Body::empty())?)
        .await?;
    assert_eq!(
        to_bytes(initialized.into_body(), 4096).await?.as_ref(),
        br#"{"initialized":true}"#
    );
    let original_login = serde_json::to_vec(&serde_json::json!({"password": PASSWORD}))?;
    let original_login = fixture
        .router()
        .oneshot(request(
            "POST",
            "/api/auth/login",
            Body::from(original_login),
        )?)
        .await?;
    assert_eq!(original_login.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_setup_has_exactly_one_winner() -> TestResult {
    // Given: two clients racing to initialize the same empty credential row.
    let fixture = Fixture::new().await?;
    let first = fixture.router().oneshot(setup_request(PASSWORD, PASSWORD)?);
    let contender = ["different", "concurrent", "password"].join("-");
    let second = fixture
        .router()
        .oneshot(setup_request(&contender, &contender)?);

    // When: both valid setup requests execute concurrently.
    let (first, second) = tokio::join!(first, second);
    let statuses = [first?.status(), second?.status()];

    // Then: SQLite singleton insertion permits one success and one conflict.
    assert!(statuses.contains(&StatusCode::OK));
    assert!(statuses.contains(&StatusCode::CONFLICT));
    Ok(())
}

#[tokio::test]
async fn setup_rejects_invalid_or_cross_origin_requests_before_mutation() -> TestResult {
    // Given: an uninitialized Controller credential.
    let fixture = Fixture::new().await?;
    let mut cross_origin = setup_request(PASSWORD, PASSWORD)?;
    cross_origin
        .headers_mut()
        .insert(header::ORIGIN, "http://attacker.test".parse()?);
    let missing_origin_body = serde_json::to_vec(&serde_json::json!({
        "password": PASSWORD,
        "password_confirmation": PASSWORD,
    }))?;
    let missing_origin = request("POST", "/api/auth/setup", Body::from(missing_origin_body))?;
    let unknown_field_body = serde_json::to_vec(&serde_json::json!({
        "password": PASSWORD,
        "password_confirmation": PASSWORD,
        "unexpected": true,
    }))?;
    let mut unknown_field = request("POST", "/api/auth/setup", Body::from(unknown_field_body))?;
    unknown_field
        .headers_mut()
        .insert(header::ORIGIN, "http://controller.test".parse()?);

    // When: origin-invalid, malformed, mismatched, short, and oversized requests are submitted.
    let cross_origin = fixture.router().oneshot(cross_origin).await?;
    let missing_origin = fixture.router().oneshot(missing_origin).await?;
    let unknown_field = fixture.router().oneshot(unknown_field).await?;
    let mismatch = fixture
        .router()
        .oneshot(setup_request(PASSWORD, "different-confirmation")?)
        .await?;
    let short = fixture
        .router()
        .oneshot(setup_request("short", "short")?)
        .await?;
    let oversized = "x".repeat(1025);
    let oversized = fixture
        .router()
        .oneshot(setup_request(&oversized, &oversized)?)
        .await?;

    // Then: no invalid request initializes the singleton credential.
    assert_eq!(cross_origin.status(), StatusCode::FORBIDDEN);
    assert_eq!(missing_origin.status(), StatusCode::FORBIDDEN);
    assert_eq!(unknown_field.status(), StatusCode::BAD_REQUEST);
    assert_eq!(mismatch.status(), StatusCode::BAD_REQUEST);
    assert_eq!(short.status(), StatusCode::BAD_REQUEST);
    assert_eq!(oversized.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        to_bytes(mismatch.into_body(), 4096).await?.as_ref(),
        br#"{"error":"invalid_request"}"#
    );
    let status = fixture
        .router()
        .oneshot(request("GET", "/api/auth/setup", Body::empty())?)
        .await?;
    assert_eq!(
        to_bytes(status.into_body(), 4096).await?.as_ref(),
        br#"{"initialized":false}"#
    );
    Ok(())
}

#[tokio::test]
async fn setup_session_and_login_survive_service_restart() -> TestResult {
    // Given: a setup-created cookie session persisted beside the singleton credential.
    let fixture = Fixture::new().await?;
    let (cookie, _) = setup(&fixture).await?;
    let mut config = ControllerConfig::default().auth;
    config.secure_cookie = false;
    let reopened = Database::open(DatabaseOptions::new(&fixture.database_path)).await?;
    let restarted = AuthService::new(config, Store::new(reopened))?;
    let router = authenticated_app_router(&fixture.assets, restarted);

    // When: the existing cookie and original password authenticate through the restarted service.
    let mut session = request("GET", "/api/auth/session", Body::empty())?;
    session
        .headers_mut()
        .insert(header::COOKIE, cookie.parse()?);
    let session = router.clone().oneshot(session).await?;
    let login_body = serde_json::to_vec(&serde_json::json!({"password": PASSWORD}))?;
    let login = router
        .oneshot(request("POST", "/api/auth/login", Body::from(login_body))?)
        .await?;

    // Then: both durable authentication paths remain valid after restart.
    assert_eq!(session.status(), StatusCode::OK);
    assert_eq!(login.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn operational_auth_routes_remain_closed_before_setup() -> TestResult {
    // Given: a Controller that has not been initialized.
    let fixture = Fixture::new().await?;

    // When: anonymous and arbitrary bearer requests access operational auth routes.
    for (method, uri) in [
        ("GET", "/api/auth/session"),
        ("GET", "/api/readiness"),
        ("POST", "/api/auth/logout"),
    ] {
        let anonymous = fixture
            .router()
            .oneshot(request(method, uri, Body::empty())?)
            .await?;
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    }
    let mut bearer = request("GET", "/api/readiness", Body::empty())?;
    bearer
        .headers_mut()
        .insert(header::AUTHORIZATION, "Bearer arbitrary-password".parse()?);
    let bearer = fixture.router().oneshot(bearer).await?;

    // Then: lack of a configured credential never opens an authentication bypass.
    assert_eq!(bearer.status(), StatusCode::UNAUTHORIZED);
    assert!(!bearer.headers().contains_key(SESSION_COOKIE));
    Ok(())
}

#[tokio::test]
async fn default_first_access_setup_accepts_https_reverse_proxy_origin() -> TestResult {
    let fixture = Fixture::new().await?;
    assert!(!fixture.auth.secure_cookie());
    let mut status = request("GET", "/api/auth/setup", Body::empty())?;
    status
        .headers_mut()
        .insert(header::ORIGIN, "https://controller.test".parse()?);
    assert_eq!(
        fixture.router().oneshot(status).await?.status(),
        StatusCode::OK
    );
    let mut setup = setup_request(PASSWORD, PASSWORD)?;
    setup
        .headers_mut()
        .insert(header::ORIGIN, "https://controller.test".parse()?);
    let response = fixture.router().oneshot(setup).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().contains_key(header::SET_COOKIE));
    Ok(())
}
