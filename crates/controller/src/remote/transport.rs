use std::time::Duration;

use reqwest::{Response, StatusCode, Url};
use serde::de::DeserializeOwned;

use super::{VidenoaClient, VidenoaClientError};

impl VidenoaClient {
    pub(super) async fn ensure_transport(&self) -> Result<(), VidenoaClientError> {
        if let Some(tunnel) = &self.tunnel {
            tokio::time::timeout(self.timeouts.connect, Box::pin(tunnel.ready()))
                .await
                .map_err(|_| VidenoaClientError::Timeout)??;
        }
        Ok(())
    }

    pub(super) fn transport_failure(&self, fallback: VidenoaClientError) -> VidenoaClientError {
        self.tunnel
            .as_ref()
            .and_then(|lease| lease.failure())
            .unwrap_or(fallback)
    }

    pub(super) fn endpoint(&self, segments: &[&str]) -> Result<Url, VidenoaClientError> {
        let mut url = self.base_url.as_url().clone();
        let mut path = url
            .path_segments_mut()
            .map_err(|()| VidenoaClientError::EndpointUrl)?;
        path.pop_if_empty();
        path.extend(segments);
        drop(path);
        Ok(url)
    }

    pub(super) fn authenticated(
        &self,
        request: reqwest::RequestBuilder,
    ) -> reqwest::RequestBuilder {
        match &self.authorization {
            Some(value) => request.header(reqwest::header::AUTHORIZATION, value.clone()),
            None => request,
        }
    }

    pub(super) async fn send_authenticated(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<Response, VidenoaClientError> {
        self.send_public(self.authenticated(request)).await
    }

    pub(super) async fn send_public(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<Response, VidenoaClientError> {
        self.ensure_transport().await?;
        request
            .timeout(self.timeouts.request)
            .send()
            .await
            .map_err(|error| self.transport_failure(classify_reqwest(&error)))
    }

    pub(super) async fn json<T: DeserializeOwned>(
        &self,
        response: Response,
    ) -> Result<T, VidenoaClientError> {
        ensure_success(response.status())?;
        let body = self.bounded_body(response).await?;
        serde_json::from_slice(&body).map_err(|_| VidenoaClientError::MalformedPayload)
    }

    /// The `error` text of a Worker's JSON error envelope, cleaned of control
    /// characters and cut to [`REJECTION_REASON_CHARS`]. Any other body yields
    /// `None`, so raw response bytes are never reflected.
    pub(super) async fn rejection_reason(&self, response: Response) -> Option<String> {
        #[derive(serde::Deserialize)]
        struct ErrorEnvelope {
            error: String,
        }
        let body = self.bounded_body(response).await.ok()?;
        let envelope: ErrorEnvelope = serde_json::from_slice(&body).ok()?;
        bounded_reason(&envelope.error)
    }

    async fn bounded_body(&self, mut response: Response) -> Result<Vec<u8>, VidenoaClientError> {
        if response
            .content_length()
            .is_some_and(|length| length > self.limits.json_bytes as u64)
        {
            return Err(VidenoaClientError::OversizedPayload {
                limit: self.limits.json_bytes,
            });
        }
        let mut body = Vec::new();
        loop {
            let chunk = stalled(self.timeouts.stall, response.chunk())
                .await?
                .map_err(|error| classify_response_body(&error))?;
            let Some(chunk) = chunk else {
                break;
            };
            if body.len().saturating_add(chunk.len()) > self.limits.json_bytes {
                return Err(VidenoaClientError::OversizedPayload {
                    limit: self.limits.json_bytes,
                });
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}

/// Longest Worker rejection reason kept, in characters.
const REJECTION_REASON_CHARS: usize = 1024;

fn bounded_reason(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return None;
    }
    let mut chars = cleaned.chars();
    let kept: String = chars.by_ref().take(REJECTION_REASON_CHARS).collect();
    Some(if chars.next().is_some() {
        format!("{kept}…")
    } else {
        kept
    })
}

pub(super) fn ensure_success(status: StatusCode) -> Result<(), VidenoaClientError> {
    match status.as_u16() {
        200..=299 => Ok(()),
        404 => Err(VidenoaClientError::NotFound),
        409 => Err(VidenoaClientError::Conflict),
        429 => Err(VidenoaClientError::RateLimited),
        code @ 400..=499 => Err(VidenoaClientError::ClientStatus {
            status: code,
            reason: None,
        }),
        code @ 500..=599 => Err(VidenoaClientError::ServerStatus { status: code }),
        code => Err(VidenoaClientError::UnexpectedStatus { status: code }),
    }
}

pub(super) fn classify_reqwest(error: &reqwest::Error) -> VidenoaClientError {
    if error.is_timeout() {
        VidenoaClientError::Timeout
    } else if error.is_body() || error.is_decode() {
        VidenoaClientError::MalformedPayload
    } else {
        VidenoaClientError::Network
    }
}

pub(super) fn classify_response_body(error: &reqwest::Error) -> VidenoaClientError {
    if error.is_timeout() {
        VidenoaClientError::Timeout
    } else {
        VidenoaClientError::MalformedPayload
    }
}

pub(super) async fn stalled<T>(
    duration: Duration,
    future: impl std::future::Future<Output = T>,
) -> Result<T, VidenoaClientError> {
    tokio::time::timeout(duration, future)
        .await
        .map_err(|_| VidenoaClientError::Stall)
}
