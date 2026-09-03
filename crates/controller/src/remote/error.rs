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
    #[error("remote request was rejected with HTTP {status}")]
    ClientStatus { status: u16 },
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
}

impl VidenoaClientError {
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
            | Self::EndpointUrl => false,
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
            VidenoaClientError::ClientStatus { status: 400 },
            VidenoaClientError::UnexpectedStatus { status: 300 },
            VidenoaClientError::MalformedPayload,
            VidenoaClientError::OversizedPayload { limit: 1 },
            VidenoaClientError::LocalIo,
            VidenoaClientError::InvalidFilePath,
            VidenoaClientError::EndpointUrl,
        ];

        // When: the runtime classifies each error for automatic retry.
        // Then: only transport and server availability failures are transient.
        assert!(transient.iter().all(VidenoaClientError::is_transient));
        assert!(fatal.iter().all(|error| !error.is_transient()));
    }
}
