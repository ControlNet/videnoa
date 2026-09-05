use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;

use super::boundary::same_origin;
use super::http::{error_response, login_response};
use super::{AuthError, AuthService, SetupRequest, SetupResponse};

pub async fn status(State(auth): State<AuthService>) -> Response {
    match auth.initialized().await {
        Ok(initialized) => Json(SetupResponse { initialized }).into_response(),
        Err(error) => error_response(&error),
    }
}

pub async fn create(
    State(auth): State<AuthService>,
    headers: HeaderMap,
    request: Result<Json<SetupRequest>, JsonRejection>,
) -> Response {
    if !same_origin(&auth, &headers) {
        return error_response(&AuthError::Forbidden);
    }
    let Ok(Json(request)) = request else {
        return error_response(&AuthError::InvalidRequest);
    };
    let password = match request.into_password() {
        Ok(password) => password,
        Err(error) => return error_response(&error),
    };
    match auth.setup(password, Utc::now()).await {
        Ok(issued) => login_response(&auth, issued),
        Err(error) => error_response(&error),
    }
}
