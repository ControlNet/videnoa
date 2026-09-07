use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{AuthMethod, SecretString, SessionId};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LoginRequest {
    pub password: SecretString,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionResponse {
    pub id: SessionId,
    pub authenticated: bool,
    pub method: AuthMethod,
    pub expires_at: DateTime<Utc>,
    pub idle_expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LoginResponse {
    pub session: SessionResponse,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogoutResponse {
    pub logged_out: bool,
}
