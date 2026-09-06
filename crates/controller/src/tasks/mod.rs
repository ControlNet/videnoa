mod error;
mod batch;
mod fingerprint;
mod intake;
pub(crate) mod mapping;
mod routes;

pub use intake::TaskService;

pub(crate) use routes::router;
