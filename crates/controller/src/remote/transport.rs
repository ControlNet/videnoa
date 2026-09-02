use std::time::Duration;

use reqwest::{Response, StatusCode, Url};
use serde::de::DeserializeOwned;

use super::{VidenoaClient, VidenoaClientError};

impl VidenoaClient {
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

    pub(super) async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<Response, VidenoaClientError> {
        request
            .timeout(self.timeouts.request)
            .send()
            .await
            .map_err(|error| classify_reqwest(&error))
    }

    pub(super) async fn json<T: DeserializeOwned>(
        &self,
        mut response: Response,
    ) -> Result<T, VidenoaClientError> {
        ensure_success(response.status())?;
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
        serde_json::from_slice(&body).map_err(|_| VidenoaClientError::MalformedPayload)
    }
}

pub(super) fn ensure_success(status: StatusCode) -> Result<(), VidenoaClientError> {
    match status.as_u16() {
        200..=299 => Ok(()),
        404 => Err(VidenoaClientError::NotFound),
        409 => Err(VidenoaClientError::Conflict),
        429 => Err(VidenoaClientError::RateLimited),
        code @ 400..=499 => Err(VidenoaClientError::ClientStatus { status: code }),
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
