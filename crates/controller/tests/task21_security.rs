#![expect(
    dead_code,
    reason = "the shared Task 21 router fixture exposes load fields unused by security scenarios"
)]

#[path = "auth_http.rs"]
mod auth_http;
#[path = "path_capabilities.rs"]
mod path_capabilities;
#[path = "task21/support.rs"]
mod support;
#[path = "task14.rs"]
mod task14;
#[path = "task21/security.rs"]
mod task21_security;
