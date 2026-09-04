use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::auth::AuthError;
use crate::domain::{ApiError, ApiErrorCode, ApiErrorEnvelope, FieldError, FieldErrorCode};
use crate::lifecycle::{LifecycleError, LifecycleErrorCode};
use crate::remote::VidenoaClientError;
use crate::scheduler::{SchedulerError, SchedulerErrorCode};
use crate::workers::{WorkerRegistryError, WorkerRegistryErrorCode};

#[derive(Debug)]
pub(super) enum OperationsError {
    Unauthorized,
    RateLimited,
    Forbidden,
    InvalidRequest,
    InvalidField(&'static str, &'static str),
    NotFound(&'static str),
    Conflict(&'static str),
    RemoteStateAmbiguous,
    PublicationAmbiguous,
    Unavailable,
    Internal,
}

impl OperationsError {
    pub(super) fn from_auth(error: &AuthError) -> Self {
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

    pub(super) fn from_worker(error: &WorkerRegistryError) -> Self {
        match error.code() {
            WorkerRegistryErrorCode::NotFound => Self::NotFound("worker was not found"),
            WorkerRegistryErrorCode::Conflict => Self::Conflict("worker changed since it was read"),
            WorkerRegistryErrorCode::DuplicateName => {
                Self::Conflict("worker name is already registered")
            }
            WorkerRegistryErrorCode::DuplicateApiUrl => {
                Self::Conflict("worker API URL is already registered")
            }
            WorkerRegistryErrorCode::Referenced => Self::Conflict("worker is referenced by tasks"),
            WorkerRegistryErrorCode::CapacityBelowUsage => {
                Self::Conflict("worker capacity is below durable usage")
            }
            WorkerRegistryErrorCode::InvalidName => {
                Self::InvalidField("name", "worker name must not be empty")
            }
            WorkerRegistryErrorCode::Internal => Self::Internal,
        }
    }

    pub(super) fn from_scheduler(error: &SchedulerError) -> Self {
        match error.code() {
            SchedulerErrorCode::Conflict => Self::Conflict("settings changed since they were read"),
            SchedulerErrorCode::Internal => Self::Internal,
        }
    }

    pub(super) fn from_lifecycle(error: &LifecycleError) -> Self {
        match error.code() {
            LifecycleErrorCode::IllegalCommand => {
                Self::Conflict("task action is not allowed in its current state")
            }
            LifecycleErrorCode::Conflict => Self::Conflict("task changed since it was read"),
            LifecycleErrorCode::RemoteStateAmbiguous => Self::RemoteStateAmbiguous,
            LifecycleErrorCode::PublicationAmbiguous => Self::PublicationAmbiguous,
            LifecycleErrorCode::Internal => Self::Internal,
        }
    }

    pub(super) fn from_remote(error: &VidenoaClientError) -> Self {
        match error {
            VidenoaClientError::NotFound => Self::RemoteStateAmbiguous,
            VidenoaClientError::Conflict | VidenoaClientError::ClientStatus { .. } => {
                Self::Conflict("remote worker rejected retry cleanup")
            }
            VidenoaClientError::RateLimited
            | VidenoaClientError::ServerStatus { .. }
            | VidenoaClientError::UnexpectedStatus { .. }
            | VidenoaClientError::Network
            | VidenoaClientError::Timeout
            | VidenoaClientError::Stall => Self::Unavailable,
            VidenoaClientError::MalformedPayload
            | VidenoaClientError::OversizedPayload { .. }
            | VidenoaClientError::LocalIo
            | VidenoaClientError::InvalidFilePath
            | VidenoaClientError::EndpointUrl => Self::Internal,
        }
    }
}

impl IntoResponse for OperationsError {
    fn into_response(self) -> Response {
        let (status, code, message, fields) = match self {
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
            Self::InvalidRequest => (
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidRequest,
                "request is invalid",
                Vec::new(),
            ),
            Self::InvalidField(field, message) => (
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidRequest,
                "request validation failed",
                vec![FieldError {
                    field: field.to_owned(),
                    code: FieldErrorCode::InvalidValue,
                    message: message.to_owned(),
                }],
            ),
            Self::NotFound(message) => (
                StatusCode::NOT_FOUND,
                ApiErrorCode::NotFound,
                message,
                Vec::new(),
            ),
            Self::Conflict(message) => (
                StatusCode::CONFLICT,
                ApiErrorCode::Conflict,
                message,
                Vec::new(),
            ),
            Self::RemoteStateAmbiguous => (
                StatusCode::CONFLICT,
                ApiErrorCode::RemoteStateAmbiguous,
                "remote state is ambiguous",
                Vec::new(),
            ),
            Self::PublicationAmbiguous => (
                StatusCode::CONFLICT,
                ApiErrorCode::PublicationAmbiguous,
                "publication state is ambiguous",
                Vec::new(),
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                ApiErrorCode::Unavailable,
                "remote worker is unavailable",
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
                    field_errors: fields,
                },
            }),
        )
            .into_response()
    }
}
