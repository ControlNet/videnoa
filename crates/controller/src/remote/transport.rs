use std::time::Duration;

use reqwest::{Response, StatusCode, Url};
use serde::de::DeserializeOwned;

use videnoa_protocol::{ErrorCode, ErrorResponse};

use super::{VidenoaClient, VidenoaClientError};

/// What a Worker's JSON error envelope says, when it is one.
#[derive(Debug, Default)]
pub(super) struct Rejection {
    pub(super) reason: Option<String>,
    pub(super) code: Option<ErrorCode>,
}

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
        if response.status() == StatusCode::FORBIDDEN {
            return Err(self.forbidden(response).await);
        }
        ensure_success(response.status())?;
        let body = self.bounded_body(response).await?;
        serde_json::from_slice(&body).map_err(|_| VidenoaClientError::MalformedPayload)
    }

    /// The `error` text of a Worker's JSON error envelope, cleaned of control
    /// characters and cut to [`REJECTION_REASON_CHARS`], with its code. Any
    /// other body yields neither, so raw response bytes are never reflected.
    /// A 403. The Worker's reason is kept only when it refused the Host this
    /// Controller addressed it by, which the user fixes on the Worker; any
    /// other 403 stays redacted.
    pub(super) async fn forbidden(&self, response: Response) -> VidenoaClientError {
        forbidden(self.rejection(response).await)
    }

    pub(super) async fn rejection(&self, response: Response) -> Rejection {
        let Ok(body) = self.bounded_body(response).await else {
            return Rejection::default();
        };
        let Ok(envelope) = serde_json::from_slice::<ErrorResponse>(&body) else {
            return Rejection::default();
        };
        Rejection {
            reason: bounded_reason(&envelope.error),
            // A code this Controller does not know leaves the status to decide.
            code: envelope.code.filter(|code| *code != ErrorCode::Unknown),
        }
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

/// A 403 read from its [`Rejection`]; see [`VidenoaClient::forbidden`].
pub(super) fn forbidden(Rejection { reason, code }: Rejection) -> VidenoaClientError {
    let refused_host = code == Some(ErrorCode::HostNotAllowed);
    VidenoaClientError::ClientStatus {
        status: StatusCode::FORBIDDEN.as_u16(),
        reason: reason.filter(|_| refused_host),
        code: code.filter(|_| refused_host),
    }
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
            code: None,
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
