use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::auth::{AuthError, MissingPeerMetadata};
use crate::domain::{ApiError, ApiErrorCode, ApiErrorEnvelope, FieldError, FieldErrorCode};

#[derive(Debug)]
pub(crate) enum TaskApiError {
    Unauthorized,
    RateLimited,
    Forbidden,
    InvalidField {
        field: &'static str,
        code: FieldErrorCode,
        message: &'static str,
    },
    InvalidRequest,
    Conflict,
    NotFound,
    Internal,
}

impl TaskApiError {
    pub(crate) fn from_auth(error: &AuthError) -> Self {
        match error {
            AuthError::Unauthorized => Self::Unauthorized,
            AuthError::RateLimited => Self::RateLimited,
            AuthError::Forbidden => Self::Forbidden,
            AuthError::InvalidPasswordHash
            | AuthError::InvalidRequest
            | AuthError::Conflict
            | AuthError::PasswordHashing
            | AuthError::PasswordVerification
            | AuthError::InvalidLifetime
            | AuthError::Persistence(_) => Self::Internal,
        }
    }

    pub(crate) const fn invalid(
        field: &'static str,
        code: FieldErrorCode,
        message: &'static str,
    ) -> Self {
        Self::InvalidField {
            field,
            code,
            message,
        }
    }
}

impl From<MissingPeerMetadata> for TaskApiError {
    fn from(_: MissingPeerMetadata) -> Self {
        Self::Internal
    }
}

impl TaskApiError {
    pub(super) fn into_parts(self) -> (StatusCode, ApiError) {
        let (status, code, message, field_errors) = match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                ApiErrorCode::Unauthorized,
                "authentication required",
                Vec::new(),
            ),
            Self::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                ApiErrorCode::RateLimited,
                "too many authentication attempts",
                Vec::new(),
            ),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                ApiErrorCode::Forbidden,
                "request proof is invalid",
                Vec::new(),
            ),
            Self::InvalidField {
                field,
                code,
                message,
            } => (
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidRequest,
                "request validation failed",
                vec![FieldError {
                    field: field.to_owned(),
                    code,
                    message: message.to_owned(),
                }],
            ),
            Self::InvalidRequest => (
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidRequest,
                "request is invalid",
                Vec::new(),
            ),
            Self::Conflict => (
                StatusCode::CONFLICT,
                ApiErrorCode::Conflict,
                "idempotency key is already used for a different request",
                Vec::new(),
            ),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                ApiErrorCode::NotFound,
                "task was not found",
                Vec::new(),
            ),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiErrorCode::InternalError,
                "internal error",
                Vec::new(),
            ),
        };
        (
            status,
            ApiError {
                code,
                message: message.to_owned(),
                retryable: false,
                field_errors,
            },
        )
    }
}

impl IntoResponse for TaskApiError {
    fn into_response(self) -> Response {
        let (status, error) = self.into_parts();
        // Messages and field names originate from static Controller validation rules.
        let diagnostics = crate::logging::TaskRequestDiagnostics(
            serde_json::json!({ "error": &error }).to_string(),
        );
        let mut response = (status, Json(ApiErrorEnvelope { error })).into_response();
        response.extensions_mut().insert(diagnostics);
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn task_error_diagnostics_match_public_response_for_every_error_kind() {
        let errors = [
            TaskApiError::Unauthorized,
            TaskApiError::RateLimited,
            TaskApiError::Forbidden,
            TaskApiError::invalid(
                "input_path",
                FieldErrorCode::InvalidValue,
                "input is unavailable",
            ),
            TaskApiError::InvalidRequest,
            TaskApiError::Conflict,
            TaskApiError::NotFound,
            TaskApiError::Internal,
        ];
        for error in errors {
            let response = error.into_response();
            let diagnostics = response
                .extensions()
                .get::<crate::logging::TaskRequestDiagnostics>()
                .unwrap()
                .0
                .clone();
            let body = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&diagnostics).unwrap(),
                serde_json::from_slice::<serde_json::Value>(&body).unwrap()
            );
        }
    }
}
