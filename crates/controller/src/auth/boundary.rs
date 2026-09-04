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

pub(crate) fn peer_ip(request: &Request) -> IpAddr {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .expect("server requests include peer connection information")
        .0
        .ip()
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
        auth.authenticate_bearer(address, password, now)?;
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
        auth.authenticate_bearer(address, password, now)?;
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

fn same_origin(auth: &AuthService, headers: &HeaderMap) -> bool {
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
    let scheme = if auth.secure_cookie() {
        "https"
    } else {
        "http"
    };
    origin == format!("{scheme}://{host}")
}
