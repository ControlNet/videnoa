use crate::domain::WorkerApiUrl;

use super::{ClientConfigError, Health, PayloadLimits, RemoteTimeouts, VidenoaClientError};

#[derive(Clone)]
pub struct VidenoaClient {
    pub(super) base_url: WorkerApiUrl,
    pub(super) http: reqwest::Client,
    pub(super) authorization: Option<reqwest::header::HeaderValue>,
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
        Self::new_with_password(base_url, timeouts, limits, None)
    }

    /// Creates a client with a private, optional worker credential for protected requests.
    ///
    /// # Errors
    /// Returns an error when the credential or HTTP client is invalid.
    pub fn new_with_password(
        base_url: WorkerApiUrl,
        timeouts: RemoteTimeouts,
        limits: PayloadLimits,
        password: Option<&crate::domain::SecretString>,
    ) -> Result<Self, ClientConfigError> {
        let authorization = if let Some(password) = password {
            let mut value = reqwest::header::HeaderValue::from_bytes(
                format!("Bearer {}", password.expose()).as_bytes(),
            )
            .map_err(|_| ClientConfigError::HttpClient)?;
            value.set_sensitive(true);
            Some(value)
        } else {
            None
        };
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
            authorization,
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
            .send_public(self.http.get(self.endpoint(&["api", "health"])?))
            .await?;
        self.json(response).await
    }
}
