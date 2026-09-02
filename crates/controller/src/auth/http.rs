use std::net::SocketAddr;

use axum::extract::connect_info::ConnectInfo;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::Serialize;

use crate::domain::{
    AuthMethod, LoginRequest, LogoutResponse, ReadinessCheck, ReadinessResponse, ReadinessStatus,
};
use crate::{app_router, FrontendAssets, StartupError};

use super::service::{session_response, IssuedSession};
use super::{AuthError, AuthService};

pub const SESSION_COOKIE: &str = "videnoa_session";
pub const CSRF_HEADER: &str = "x-csrf-token";

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
}

enum RequestAuth {
    Bearer,
    Session(crate::persistence::SessionRecord),
}

pub fn authenticated_app_router(assets: &FrontendAssets, auth: AuthService) -> Router {
    let routes = Router::new()
        .route("/api/auth/login", post(login))
        .route("/api/auth/session", get(session))
        .route("/api/auth/logout", post(logout))
        .route("/api/readiness", get(readiness))
        .with_state(auth);
    app_router(assets).merge(routes)
}

/// Serves the authenticated Controller API and frontend until the HTTP server exits.
///
/// # Errors
/// Returns a typed startup error when the listener cannot bind or the server fails.
pub async fn serve_authenticated(
    address: SocketAddr,
    assets: &FrontendAssets,
    auth: AuthService,
) -> Result<(), StartupError> {
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|source| StartupError::Bind { address, source })?;
    axum::serve(
        listener,
        authenticated_app_router(assets, auth).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .map_err(StartupError::Serve)
}

async fn login(
    State(auth): State<AuthService>,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    Json(request): Json<LoginRequest>,
) -> Response {
    match auth
        .login(address.ip(), &request.password, Utc::now())
        .await
    {
        Ok(issued) => login_response(&auth, issued),
        Err(error) => error_response(&error),
    }
}

async fn session(State(auth): State<AuthService>, headers: HeaderMap) -> Response {
    let now = Utc::now();
    match authenticate(&auth, &headers, now).await {
        Ok(RequestAuth::Bearer) => Json(session_response(
            crate::domain::SessionId::random(),
            AuthMethod::Bearer,
            now,
            now,
        ))
        .into_response(),
        Ok(RequestAuth::Session(record)) => match auth.rotate_csrf(record.id).await {
            Ok(csrf) => {
                let response = session_response(
                    record.id,
                    AuthMethod::Session,
                    record.absolute_expires_at,
                    record.idle_expires_at,
                );
                response_with_csrf(Json(response).into_response(), &csrf)
            }
            Err(error) => error_response(&error),
        },
        Err(error) => error_response(&error),
    }
}

async fn logout(State(auth): State<AuthService>, headers: HeaderMap) -> Response {
    match authenticate(&auth, &headers, Utc::now()).await {
        Ok(RequestAuth::Bearer) => logout_response(&auth),
        Ok(RequestAuth::Session(record)) => {
            if !same_origin(&auth, &headers)
                || headers
                    .get(CSRF_HEADER)
                    .and_then(|value| value.to_str().ok())
                    .is_none_or(|csrf| !AuthService::csrf_matches(&record, csrf))
            {
                return error_response(&AuthError::Forbidden);
            }
            match auth.logout(record.id, Utc::now()).await {
                Ok(()) => logout_response(&auth),
                Err(error) => error_response(&error),
            }
        }
        Err(error) => error_response(&error),
    }
}

async fn readiness(State(auth): State<AuthService>, headers: HeaderMap) -> Response {
    match authenticate(&auth, &headers, Utc::now()).await {
        Ok(RequestAuth::Bearer | RequestAuth::Session(_)) => Json(ReadinessResponse {
            status: ReadinessStatus::Ready,
            checks: vec![ReadinessCheck {
                name: "authentication".to_owned(),
                ready: true,
                message: None,
            }],
        })
        .into_response(),
        Err(error) => error_response(&error),
    }
}

async fn authenticate(
    auth: &AuthService,
    headers: &HeaderMap,
    now: chrono::DateTime<Utc>,
) -> Result<RequestAuth, AuthError> {
    if let Some(password) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    {
        auth.authenticate_bearer(password)?;
        return Ok(RequestAuth::Bearer);
    }
    let token = cookie(headers, SESSION_COOKIE).ok_or(AuthError::Unauthorized)?;
    auth.authenticate_session_at(token, now)
        .await
        .map(RequestAuth::Session)
}

fn login_response(auth: &AuthService, issued: IssuedSession) -> Response {
    let cookie = session_cookie(auth, &issued.token, false);
    let mut response = Json(issued.response).into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, header_value(&cookie));
    response_with_csrf(response, &issued.csrf)
}

fn logout_response(auth: &AuthService) -> Response {
    let mut response = Json(LogoutResponse { logged_out: true }).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        header_value(&session_cookie(auth, "", true)),
    );
    response
}

fn response_with_csrf(mut response: Response, csrf: &str) -> Response {
    response
        .headers_mut()
        .insert(CSRF_HEADER, header_value(csrf));
    response
}

fn session_cookie(auth: &AuthService, token: &str, expired: bool) -> String {
    let max_age = if expired {
        0
    } else {
        auth.session_absolute_seconds()
    };
    let secure = if auth.secure_cookie() { "; Secure" } else { "" };
    format!(
        "{SESSION_COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age}{secure}"
    )
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix(name)?.strip_prefix('='))
}

fn same_origin(auth: &AuthService, headers: &HeaderMap) -> bool {
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let scheme = if auth.secure_cookie() {
        "https"
    } else {
        "http"
    };
    origin == format!("{scheme}://{host}")
}

fn error_response(error: &AuthError) -> Response {
    let (status, code) = match error {
        AuthError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
        AuthError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
        AuthError::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
        AuthError::InvalidPasswordHash
        | AuthError::PasswordHashing
        | AuthError::PasswordFile { .. }
        | AuthError::InvalidLifetime
        | AuthError::Persistence(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
    };
    (status, Json(ErrorBody { error: code })).into_response()
}

fn header_value(value: &str) -> HeaderValue {
    HeaderValue::from_str(value).unwrap_or_else(|_| HeaderValue::from_static("invalid"))
}
