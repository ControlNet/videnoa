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
            | AuthError::PasswordHashing
            | AuthError::PasswordFile { .. }
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

impl IntoResponse for TaskApiError {
    fn into_response(self) -> Response {
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
            Json(ApiErrorEnvelope {
                error: ApiError {
                    code,
                    message: message.to_owned(),
                    retryable: false,
                    field_errors,
                },
            }),
        )
            .into_response()
    }
}
