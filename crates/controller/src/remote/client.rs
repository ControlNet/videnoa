use crate::domain::WorkerApiUrl;

use super::{ClientConfigError, Health, PayloadLimits, RemoteTimeouts, VidenoaClientError};

#[derive(Clone)]
pub struct VidenoaClient {
    pub(super) base_url: WorkerApiUrl,
    pub(super) http: reqwest::Client,
    pub(super) timeouts: RemoteTimeouts,
    pub(super) limits: PayloadLimits,
}

impl VidenoaClient {
    /// Creates a bounded HTTP client using rustls for HTTPS endpoints.
    ///
    /// # Errors
    /// Returns [`ClientConfigError`] when the HTTP client cannot be constructed.
    pub fn new(
        base_url: WorkerApiUrl,
        timeouts: RemoteTimeouts,
        limits: PayloadLimits,
    ) -> Result<Self, ClientConfigError> {
        let http = reqwest::Client::builder()
            .connect_timeout(timeouts.connect)
            .pool_max_idle_per_host(8)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .map_err(|_| ClientConfigError::HttpClient)?;
        Ok(Self {
            base_url,
            http,
            timeouts,
            limits,
        })
    }

    /// Fetches typed remote health.
    ///
    /// # Errors
    /// Returns [`VidenoaClientError`] for transport, status, bounds, or payload failures.
    pub async fn health(&self) -> Result<Health, VidenoaClientError> {
        let response = self
            .send(self.http.get(self.endpoint(&["api", "health"])?))
            .await?;
        self.json(response).await
    }
}
