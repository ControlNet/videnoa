use serde::{Deserialize, Serialize};

use super::{ApiErrorCode, FieldErrorCode};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FieldError {
    pub field: String,
    pub code: FieldErrorCode,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApiError {
    pub code: ApiErrorCode,
    pub message: String,
    pub retryable: bool,
    pub field_errors: Vec<FieldError>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApiErrorEnvelope {
    pub error: ApiError,
}
