use std::net::{IpAddr, SocketAddr};

use axum::extract::connect_info::ConnectInfo;
use axum::extract::Request;
use axum::http::{header, HeaderMap};
use chrono::{DateTime, Utc};

use crate::persistence::SessionRecord;

use super::{AuthError, AuthService};

pub(crate) enum RequestAuth {
    Bearer,
    Session(SessionRecord),
}

#[derive(Debug)]
pub(crate) struct MissingPeerMetadata;

pub(crate) fn peer_ip(request: &Request) -> Result<IpAddr, MissingPeerMetadata> {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|connect_info| connect_info.0.ip())
        .ok_or(MissingPeerMetadata)
}

pub(crate) async fn authenticate(
    auth: &AuthService,
    address: IpAddr,
    headers: &HeaderMap,
    now: DateTime<Utc>,
) -> Result<RequestAuth, AuthError> {
    if let Some(password) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    {
        auth.authenticate_bearer(address, password, now).await?;
        return Ok(RequestAuth::Bearer);
    }
    let token = cookie(headers, super::SESSION_COOKIE).ok_or(AuthError::Unauthorized)?;
    auth.authenticate_session_at(token, now)
        .await
        .map(RequestAuth::Session)
}

pub(crate) async fn authenticate_passive(
    auth: &AuthService,
    address: IpAddr,
    headers: &HeaderMap,
    now: DateTime<Utc>,
) -> Result<RequestAuth, AuthError> {
    if let Some(password) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    {
        auth.authenticate_bearer(address, password, now).await?;
        return Ok(RequestAuth::Bearer);
    }
    let token = cookie(headers, super::SESSION_COOKIE).ok_or(AuthError::Unauthorized)?;
    auth.validate_session_at(token, now)
        .await
        .map(RequestAuth::Session)
}

pub(crate) async fn authorize_mutation(
    auth: &AuthService,
    address: IpAddr,
    headers: &HeaderMap,
    now: DateTime<Utc>,
) -> Result<RequestAuth, AuthError> {
    let authenticated = authenticate(auth, address, headers, now).await?;
    match &authenticated {
        RequestAuth::Bearer => Ok(authenticated),
        RequestAuth::Session(session) => {
            let csrf_matches = headers
                .get(super::CSRF_HEADER)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|csrf| AuthService::csrf_matches(session, csrf));
            if same_origin(auth, headers) && csrf_matches {
                Ok(authenticated)
            } else {
                Err(AuthError::Forbidden)
            }
        }
    }
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix(name)?.strip_prefix('='))
}

pub(crate) fn same_origin(auth: &AuthService, headers: &HeaderMap) -> bool {
    if headers.get_all(header::HOST).iter().count() != 1
        || headers.get_all(header::ORIGIN).iter().count() != 1
    {
        return false;
    }
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    valid_origin(host, origin, auth.secure_cookie())
}

fn valid_origin(host: &str, origin: &str, require_https: bool) -> bool {
    // A browser Origin is one serialized origin, never credentials, a URL path,
    // multiple origins, or proxy forwarding metadata.
    let Ok(parsed) = url::Url::parse(origin) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https")
        || (require_https && parsed.scheme() != "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
        || origin.contains(['\\', ' ', '\t', '\r', '\n'])
    {
        return false;
    }
    let Some(authority) = origin
        .strip_prefix(parsed.scheme())
        .and_then(|s| s.strip_prefix("://"))
    else {
        return false;
    };
    if authority.is_empty() || authority.contains(['/', '@']) || authority.ends_with(':') {
        return false;
    }
    let Ok(host_authority) = host.parse::<axum::http::uri::Authority>() else {
        return false;
    };
    if host_authority.as_str().contains('@') {
        return false;
    }
    let Ok(expected) = url::Url::parse(&format!("{}://{}", parsed.scheme(), host_authority)) else {
        return false;
    };
    parsed.host().is_some()
        && parsed.host() == expected.host()
        && parsed.port_or_known_default() == expected.port_or_known_default()
}

#[cfg(test)]
mod tests {
    use super::valid_origin;

    #[test]
    fn same_origin_transport_matrix() {
        for (secure, origin, accepted) in [
            (false, "http://controller.example.com", true),
            (false, "https://controller.example.com", true),
            (false, "https://different.example.com", false),
            (true, "https://controller.example.com", true),
            (true, "http://controller.example.com", false),
        ] {
            assert_eq!(
                valid_origin("controller.example.com", origin, secure),
                accepted
            );
        }
    }

    #[test]
    fn authorities_ports_and_malformed_origins() {
        for (host, origin) in [
            ("example.com:443", "https://EXAMPLE.com"),
            ("example.com", "http://example.com:80"),
            ("127.0.0.1:3001", "https://127.0.0.1:3001"),
            ("[::1]:3001", "https://[::1]:3001"),
        ] {
            assert!(valid_origin(host, origin, false), "{host} {origin}");
        }
        for origin in [
            "null",
            "https://example.com:444",
            "https://example.com/",
            "https://example.com/path",
            "https://user@example.com",
            "https://@example.com",
            "https://example.com:",
            "https://example.com?x",
            "https://example.com#x",
            "https://example.com https://other.com",
            "https:example.com",
            "https://example.com\\evil",
            "file://example.com",
        ] {
            assert!(!valid_origin("example.com", origin, false), "{origin}");
        }
        assert!(!valid_origin(
            "user@example.com",
            "https://example.com",
            false
        ));
        assert!(!valid_origin("", "https://example.com", false));
    }
}
