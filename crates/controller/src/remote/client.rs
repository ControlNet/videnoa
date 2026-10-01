use crate::domain::WorkerApiUrl;

use super::{ClientConfigError, Health, PayloadLimits, RemoteTimeouts, VidenoaClientError};

/// How long an idle pooled connection to a Worker is kept for reuse.
///
/// Workers close a keep-alive connection whose next request head has not
/// arrived within 30 seconds (`HEADER_READ_TIMEOUT`, the same value as this
/// crate's [`crate::http_server::HEADER_READ_TIMEOUT`]). A request sent just as
/// the Worker closes fails with an incomplete message, so the Controller must
/// drop idle connections well before the Worker does.
const WORKER_POOL_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

#[derive(Clone)]
pub struct VidenoaClient {
    pub(super) tunnel: Option<std::sync::Arc<super::iroh::Lease>>,
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
        let (tunnel, base_url) = if let Some(peer) = base_url.iroh_id() {
            let password = password.ok_or(ClientConfigError::HttpClient)?;
            let (lease, local_url) = super::iroh::Lease::bind(peer, password)?;
            (Some(lease), local_url)
        } else {
            (None, base_url)
        };
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
        let builder = reqwest::Client::builder();
        let builder = if tunnel.is_some() {
            builder.no_proxy()
        } else {
            builder
        };
        let http = builder
            .connect_timeout(timeouts.connect)
            .pool_max_idle_per_host(8)
            .pool_idle_timeout(WORKER_POOL_IDLE_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .map_err(|_| ClientConfigError::HttpClient)?;
        Ok(Self {
            tunnel,
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

#[cfg(test)]
mod tests {
    use super::WORKER_POOL_IDLE_TIMEOUT;

    #[test]
    fn idle_connections_are_dropped_before_the_worker_closes_them() {
        let worker_header_read_timeout = crate::http_server::HEADER_READ_TIMEOUT;
        assert!(
            WORKER_POOL_IDLE_TIMEOUT + std::time::Duration::from_secs(5)
                <= worker_header_read_timeout,
            "pool idle timeout {WORKER_POOL_IDLE_TIMEOUT:?} must stay clearly below \
             the Worker's {worker_header_read_timeout:?} request-head deadline"
        );
    }
}
