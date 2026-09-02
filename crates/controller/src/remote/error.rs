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
