use videnoa_protocol::ErrorCode;

#[derive(Debug, thiserror::Error)]
pub enum ClientConfigError {
    #[error("remote client timeouts must be greater than zero")]
    ZeroTimeout,
    #[error("remote client payload limits must be greater than zero")]
    ZeroPayloadLimit,
    #[error("failed to construct the remote HTTP client")]
    HttpClient,
}

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub enum VidenoaClientError {
    #[error("remote resource was not found")]
    NotFound,
    #[error("remote request conflicted with existing state")]
    Conflict,
    #[error("remote request was rate limited")]
    RateLimited,
    #[error("remote request was rejected with HTTP {status}{}", reason_suffix(reason.as_deref()))]
    ClientStatus {
        status: u16,
        /// The Worker's bounded rejection reason; set only for a rejected
        /// submission (`POST /api/run` answered with a JSON `error`).
        reason: Option<String>,
        /// The Worker's machine-readable code (protocol 2), when it is one
        /// this Controller knows.
        code: Option<ErrorCode>,
    },
    #[error("remote service failed with HTTP {status}")]
    ServerStatus { status: u16 },
    #[error("remote service returned unexpected HTTP {status}")]
    UnexpectedStatus { status: u16 },
    #[error("remote network request failed")]
    Network,
    #[error("remote request timed out")]
    Timeout,
    #[error("remote response stalled")]
    Stall,
    #[error("remote payload was malformed")]
    MalformedPayload,
    #[error("remote payload exceeded the {limit}-byte bound")]
    OversizedPayload { limit: usize },
    #[error("local transfer I/O failed")]
    LocalIo,
    #[error("remote file path is invalid")]
    InvalidFilePath,
    #[error("failed to construct a remote endpoint URL")]
    EndpointUrl,
    /// The Worker's tunnel or API speaks another protocol version; retrying
    /// cannot help until one side is upgraded.
    #[error("worker is incompatible with this Controller")]
    Incompatible,
}

fn reason_suffix(reason: Option<&str>) -> String {
    reason
        .map(|reason| format!(": {reason}"))
        .unwrap_or_default()
}

impl VidenoaClientError {
    /// Whether the Worker rejected this Controller's credentials, which says
    /// nothing about the request itself.
    #[must_use]
    pub const fn is_authentication(&self) -> bool {
        matches!(
            self,
            Self::ClientStatus {
                status: 401 | 403,
                ..
            }
        )
    }

    /// Whether the failure is about the Worker rather than the request: it is
    /// unreachable or overloaded, rejected the credentials, rate limited this
    /// Controller, sits behind a redirecting proxy, or speaks another tunnel
    /// protocol. Such a failure must never fail the task that sent it.
    #[must_use]
    pub const fn is_worker_level(&self) -> bool {
        self.is_transient()
            || self.is_authentication()
            || matches!(
                self,
                Self::RateLimited | Self::UnexpectedStatus { .. } | Self::Incompatible
            )
    }

    #[must_use]
    pub const fn is_transient(&self) -> bool {
        match self {
            Self::ServerStatus { .. } | Self::Network | Self::Timeout | Self::Stall => true,
            Self::NotFound
            | Self::Conflict
            | Self::RateLimited
            | Self::ClientStatus { .. }
            | Self::UnexpectedStatus { .. }
            | Self::MalformedPayload
            | Self::OversizedPayload { .. }
            | Self::LocalIo
            | Self::InvalidFilePath
            | Self::EndpointUrl
            | Self::Incompatible => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::VidenoaClientError;

    #[test]
    fn transient_classification_covers_every_remote_error_variant() {
        // Given: every remote error that can cross a runtime stage boundary.
        let transient = [
            VidenoaClientError::ServerStatus { status: 503 },
            VidenoaClientError::Network,
            VidenoaClientError::Timeout,
            VidenoaClientError::Stall,
        ];
        let fatal = [
            VidenoaClientError::NotFound,
            VidenoaClientError::Conflict,
            VidenoaClientError::RateLimited,
            VidenoaClientError::ClientStatus {
                status: 400,
                reason: None,
                code: None,
            },
            VidenoaClientError::UnexpectedStatus { status: 300 },
            VidenoaClientError::MalformedPayload,
            VidenoaClientError::OversizedPayload { limit: 1 },
            VidenoaClientError::LocalIo,
            VidenoaClientError::InvalidFilePath,
            VidenoaClientError::EndpointUrl,
            VidenoaClientError::Incompatible,
        ];

        // When: the runtime classifies each error for automatic retry.
        // Then: only transport and server availability failures are transient.
        assert!(transient.iter().all(VidenoaClientError::is_transient));
        assert!(fatal.iter().all(|error| !error.is_transient()));
    }

    #[test]
    fn worker_level_failures_are_the_ones_no_request_can_be_blamed_for() {
        let worker_level = [
            VidenoaClientError::ServerStatus { status: 503 },
            VidenoaClientError::Network,
            VidenoaClientError::Timeout,
            VidenoaClientError::Stall,
            VidenoaClientError::RateLimited,
            VidenoaClientError::ClientStatus {
                status: 401,
                reason: None,
                code: None,
            },
            VidenoaClientError::ClientStatus {
                status: 403,
                reason: None,
                code: None,
            },
            VidenoaClientError::UnexpectedStatus { status: 302 },
            VidenoaClientError::Incompatible,
        ];
        let request_level = [
            VidenoaClientError::NotFound,
            VidenoaClientError::Conflict,
            VidenoaClientError::ClientStatus {
                status: 400,
                reason: Some("invalid workflow".to_owned()),
                code: None,
            },
            VidenoaClientError::MalformedPayload,
            VidenoaClientError::OversizedPayload { limit: 1 },
            VidenoaClientError::LocalIo,
            VidenoaClientError::InvalidFilePath,
            VidenoaClientError::EndpointUrl,
        ];
        assert!(worker_level.iter().all(VidenoaClientError::is_worker_level));
        assert!(request_level.iter().all(|error| !error.is_worker_level()));
    }
}
