use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

use crate::domain::SecretString;

use super::AuthError;

const MIN_PASSWORD_BYTES: usize = 12;
const MAX_PASSWORD_BYTES: usize = 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SetupRequest {
    pub password: SecretString,
    pub password_confirmation: SecretString,
}

impl SetupRequest {
    /// Consumes a confirmed password within the accepted byte bounds.
    ///
    /// # Errors
    /// Returns [`AuthError::InvalidRequest`] when bounds or confirmation do not match.
    pub fn into_password(self) -> Result<SecretString, AuthError> {
        let password = self.password.expose().as_bytes();
        let confirmation = self.password_confirmation.expose().as_bytes();
        let valid_length = (MIN_PASSWORD_BYTES..=MAX_PASSWORD_BYTES).contains(&password.len());
        let confirmed =
            password.len() == confirmation.len() && bool::from(password.ct_eq(confirmation));
        if valid_length && confirmed {
            Ok(self.password)
        } else {
            Err(AuthError::InvalidRequest)
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SetupResponse {
    pub initialized: bool,
}
