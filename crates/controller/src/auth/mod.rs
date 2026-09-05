#[path = "service.rs"]
mod authentication_service;
mod boundary;
#[path = "password.rs"]
mod credentials;
mod dto;
mod http;
#[path = "limiter.rs"]
mod login_attempts;
mod session;
mod setup_http;

pub use authentication_service::{AuthError, AuthService};
pub use credentials::hash_password;
pub use dto::{SetupRequest, SetupResponse};
pub use http::{
    authenticated_app_router, controller_app_router, serve_authenticated, serve_controller,
    serve_controller_until, CSRF_HEADER, SESSION_COOKIE,
};

pub(crate) use boundary::{
    authenticate, authenticate_passive, authorize_mutation, peer_ip, MissingPeerMetadata,
    RequestAuth,
};
