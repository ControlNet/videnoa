use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::auth::{AuthError, MissingPeerMetadata};
use crate::domain::{ApiError, ApiErrorCode, ApiErrorEnvelope, FieldError, FieldErrorCode};
use crate::lifecycle::{LifecycleError, LifecycleErrorCode};
use crate::remote::VidenoaClientError;
use crate::scheduler::{SchedulerError, SchedulerErrorCode};
use crate::workers::{WorkerRegistryError, WorkerRegistryErrorCode};

const COMMITTED_DEGRADED_MESSAGE: &str =
    "settings persisted; HTTP listener stopped during handoff; restart loads the saved configuration";
type ErrorResponseParts = (
    StatusCode,
    ApiErrorCode,
    &'static str,
    bool,
    Vec<FieldError>,
);

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
    CommittedDegraded,
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
            | AuthError::InvalidRequest
            | AuthError::Conflict
            | AuthError::PasswordHashing
            | AuthError::PasswordVerification
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

impl From<MissingPeerMetadata> for OperationsError {
    fn from(_: MissingPeerMetadata) -> Self {
        Self::Internal
    }
}

impl IntoResponse for OperationsError {
    fn into_response(self) -> Response {
        api_error_response(match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                ApiErrorCode::Unauthorized,
                "authentication required",
                false,
                Vec::new(),
            ),
            Self::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                ApiErrorCode::RateLimited,
                "too many authentication attempts",
                false,
                Vec::new(),
            ),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                ApiErrorCode::Forbidden,
                "request proof is invalid",
                false,
                Vec::new(),
            ),
            Self::InvalidRequest => (
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidRequest,
                "request is invalid",
                false,
                Vec::new(),
            ),
            Self::InvalidField(field, message) => (
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidRequest,
                "request validation failed",
                false,
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
                false,
                Vec::new(),
            ),
            Self::Conflict(message) => (
                StatusCode::CONFLICT,
                ApiErrorCode::Conflict,
                message,
                false,
                Vec::new(),
            ),
            Self::RemoteStateAmbiguous => (
                StatusCode::CONFLICT,
                ApiErrorCode::RemoteStateAmbiguous,
                "remote state is ambiguous",
                false,
                Vec::new(),
            ),
            Self::PublicationAmbiguous => (
                StatusCode::CONFLICT,
                ApiErrorCode::PublicationAmbiguous,
                "publication state is ambiguous",
                false,
                Vec::new(),
            ),
            Self::CommittedDegraded => (
                StatusCode::SERVICE_UNAVAILABLE,
                ApiErrorCode::Unavailable,
                COMMITTED_DEGRADED_MESSAGE,
                true,
                Vec::new(),
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                ApiErrorCode::Unavailable,
                "remote worker is unavailable",
                false,
                Vec::new(),
            ),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiErrorCode::InternalError,
                "internal error",
                false,
                Vec::new(),
            ),
        })
    }
}

fn api_error_response((status, code, message, retryable, fields): ErrorResponseParts) -> Response {
    (
        status,
        Json(ApiErrorEnvelope {
            error: ApiError {
                code,
                message: message.to_owned(),
                retryable,
                field_errors: fields,
            },
        }),
    )
        .into_response()
}
