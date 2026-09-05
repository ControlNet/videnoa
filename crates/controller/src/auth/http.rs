use std::net::SocketAddr;

use axum::extract::connect_info::ConnectInfo;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::domain::{
    AuthMethod, LoginRequest, LogoutResponse, ReadinessCheck, ReadinessResponse, ReadinessStatus,
};
use crate::{app_router, FrontendAssets, StartupError};

use super::authentication_service::{session_response, IssuedSession};
use super::{authenticate, authorize_mutation, AuthError, AuthService, RequestAuth};

pub const SESSION_COOKIE: &str = "videnoa_session";
pub const CSRF_HEADER: &str = "x-csrf-token";

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
}

pub fn authenticated_app_router(assets: &FrontendAssets, auth: AuthService) -> Router {
    app_router(assets).merge(auth_routes(auth, true))
}

fn auth_routes(auth: AuthService, include_readiness: bool) -> Router {
    let routes = Router::new()
        .route("/api/auth/login", post(login))
        .route("/api/auth/session", get(session))
        .route("/api/auth/logout", post(logout));
    let routes = if include_readiness {
        routes.route("/api/readiness", get(readiness))
    } else {
        routes
    };
    routes.with_state(auth)
}

pub fn controller_app_router(
    assets: &FrontendAssets,
    auth: AuthService,
    tasks: crate::tasks::TaskService,
    operations: crate::operations::OperationsState,
) -> Router {
    let tasks = tasks.with_event_hub(operations.event_hub());
    app_router(assets)
        .merge(auth_routes(auth.clone(), false))
        .merge(crate::tasks::router(auth, tasks))
        .merge(crate::operations::router(operations))
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

/// Serves the complete authenticated Controller API and frontend.
///
/// # Errors
/// Returns a typed startup error when the listener cannot bind or the server fails.
pub async fn serve_controller(
    address: SocketAddr,
    assets: &FrontendAssets,
    auth: AuthService,
    tasks: crate::tasks::TaskService,
    operations: crate::operations::OperationsState,
) -> Result<(), StartupError> {
    serve_controller_until(
        address,
        assets,
        auth,
        tasks,
        operations,
        CancellationToken::new(),
    )
    .await
}

/// Serves the complete Controller until coordinated shutdown closes HTTP intake.
///
/// # Errors
/// Returns a typed startup error when the listener cannot bind or the server fails.
pub async fn serve_controller_until(
    address: SocketAddr,
    assets: &FrontendAssets,
    auth: AuthService,
    tasks: crate::tasks::TaskService,
    operations: crate::operations::OperationsState,
    shutdown: CancellationToken,
) -> Result<(), StartupError> {
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|source| StartupError::Bind { address, source })?;
    let operations = operations.with_shutdown(shutdown.child_token());
    axum::serve(
        listener,
        controller_app_router(assets, auth, tasks, operations)
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown.cancelled_owned())
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

async fn session(
    State(auth): State<AuthService>,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let now = Utc::now();
    match authenticate(&auth, address.ip(), &headers, now).await {
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
            Err(error) => session_error_response(&auth, &error),
        },
        Err(error) => session_error_response(&auth, &error),
    }
}

async fn logout(
    State(auth): State<AuthService>,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    match authorize_mutation(&auth, address.ip(), &headers, Utc::now()).await {
        Ok(RequestAuth::Bearer) => logout_response(&auth),
        Ok(RequestAuth::Session(record)) => match auth.logout(record.id, Utc::now()).await {
            Ok(()) => logout_response(&auth),
            Err(error) => error_response(&error),
        },
        Err(error) => error_response(&error),
    }
}

async fn readiness(
    State(auth): State<AuthService>,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    match authenticate(&auth, address.ip(), &headers, Utc::now()).await {
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

fn session_error_response(auth: &AuthService, error: &AuthError) -> Response {
    let mut response = error_response(error);
    if matches!(error, AuthError::Unauthorized) {
        response.headers_mut().insert(
            header::SET_COOKIE,
            header_value(&session_cookie(auth, "", true)),
        );
    }
    response
}

fn error_response(error: &AuthError) -> Response {
    let (status, code) = match error {
        AuthError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
        AuthError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
        AuthError::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
        AuthError::InvalidPasswordHash
        | AuthError::PasswordHashing
        | AuthError::PasswordVerification
        | AuthError::PasswordFile { .. }
        | AuthError::InvalidLifetime
        | AuthError::Persistence(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
    };
    (status, Json(ErrorBody { error: code })).into_response()
}

fn header_value(value: &str) -> HeaderValue {
    HeaderValue::from_str(value).unwrap_or_else(|_| HeaderValue::from_static("invalid"))
}
