use std::net::{IpAddr, Ipv4Addr};
use std::num::{NonZeroU16, NonZeroU32};
use std::path::{Path, PathBuf};
use std::time::Duration;

use figment::providers::{Env, Format, Serialized, Toml};
use figment::Figment;

mod raw;
mod validate;
use raw::RawControllerConfig;

const DEFAULT_MAX_ATTEMPTS: NonZeroU32 = match NonZeroU32::new(5) {
    Some(value) => value,
    None => NonZeroU32::MIN,
};
const SESSION_ABSOLUTE_SECONDS: u64 = 24 * 60 * 60;
const SESSION_IDLE_SECONDS: u64 = 60 * 60;
const TRANSFER_TIMEOUT_SECONDS: u64 = 5 * 60;
const RETRY_MAXIMUM_SECONDS: u64 = 60;

pub const ENV_PREFIX: &str = "VIDENOA_CONTROLLER_";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControllerConfig {
    pub server: ServerConfig,
    pub paths: PathConfig,
    pub auth: AuthConfig,
    pub scheduler: SchedulerConfig,
    pub timeouts: TimeoutConfig,
    pub retry: RetryConfig,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerConfig {
    pub host: IpAddr,
    pub port: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathConfig {
    pub input_roots: Vec<PathBuf>,
    pub output_roots: Vec<PathBuf>,
    pub data_root: PathBuf,
    pub temp_root: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthConfig {
    pub password_hash_file: PathBuf,
    pub secure_cookie: bool,
    pub session_absolute: Duration,
    pub session_idle: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchedulerConfig {
    pub paused: bool,
    pub default_compute_slots: NonZeroU16,
    pub prefetch_per_worker: u16,
    pub max_concurrent_uploads: NonZeroU16,
    pub max_concurrent_downloads: NonZeroU16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimeoutConfig {
    pub health: Duration,
    pub poll: Duration,
    pub transfer: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetryConfig {
    pub initial: Duration,
    pub maximum: Duration,
    pub max_attempts: NonZeroU32,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("configuration schema is invalid: {detail}")]
    Schema { detail: String },
    #[error("configuration root `{field}` is invalid at {path}: {reason}")]
    InvalidRoot {
        field: &'static str,
        path: PathBuf,
        reason: &'static str,
    },
    #[error("password hash file is missing or invalid: {path}")]
    MissingPasswordHashFile { path: PathBuf },
    #[error("password hash file does not contain an Argon2id PHC string: {path}")]
    InvalidPasswordHash { path: PathBuf },
    #[error("configuration value `{field}` must be greater than zero")]
    ZeroValue { field: &'static str },
    #[error("configuration value `{field}` ({value}) exceeds maximum {maximum}")]
    NumericOverflow {
        field: &'static str,
        value: u64,
        maximum: u64,
    },
    #[error("session idle lifetime must not exceed absolute lifetime")]
    InvalidSessionBounds,
    #[error("retry initial delay {initial} must not exceed maximum delay {maximum}")]
    InvalidRetryBounds { initial: u64, maximum: u64 },
}

impl ControllerConfig {
    /// Parses and validates a TOML configuration layered over typed defaults.
    ///
    /// # Errors
    /// Returns [`ConfigError`] when the schema or any configured boundary is invalid.
    pub fn from_toml(source: &str) -> Result<Self, ConfigError> {
        let figment = Figment::from(Serialized::defaults(RawControllerConfig::default()))
            .merge(Toml::string(source));
        Self::extract(&figment)
    }

    /// Loads defaults, an optional exact TOML file, and prefixed environment overrides.
    ///
    /// # Errors
    /// Returns [`ConfigError`] when extraction or runtime boundary validation fails.
    pub fn load(path: Option<&Path>) -> Result<Self, ConfigError> {
        let mut figment = Figment::from(Serialized::defaults(RawControllerConfig::default()));
        if let Some(path) = path.filter(|candidate| candidate.exists()) {
            figment = figment.merge(Toml::file_exact(path));
        }
        let figment = figment.merge(Env::prefixed(ENV_PREFIX).split("__"));
        Self::extract(&figment)
    }

    fn extract(figment: &Figment) -> Result<Self, ConfigError> {
        let raw =
            figment
                .extract::<RawControllerConfig>()
                .map_err(|error| ConfigError::Schema {
                    detail: error.to_string(),
                })?;
        validate::build_config(raw)
    }
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: IpAddr::V4(Ipv4Addr::LOCALHOST),
                port: 3001,
            },
            paths: PathConfig {
                input_roots: vec![PathBuf::from("input")],
                output_roots: vec![PathBuf::from("output")],
                data_root: PathBuf::from("data"),
                temp_root: PathBuf::from("data/temp"),
            },
            auth: AuthConfig {
                password_hash_file: PathBuf::from("data/admin-password.phc"),
                secure_cookie: true,
                session_absolute: Duration::from_secs(SESSION_ABSOLUTE_SECONDS),
                session_idle: Duration::from_secs(SESSION_IDLE_SECONDS),
            },
            scheduler: SchedulerConfig {
                paused: false,
                default_compute_slots: NonZeroU16::MIN,
                prefetch_per_worker: 1,
                max_concurrent_uploads: NonZeroU16::MIN,
                max_concurrent_downloads: NonZeroU16::MIN,
            },
            timeouts: TimeoutConfig {
                health: Duration::from_secs(10),
                poll: Duration::from_secs(5),
                transfer: Duration::from_secs(TRANSFER_TIMEOUT_SECONDS),
            },
            retry: RetryConfig {
                initial: Duration::from_secs(1),
                maximum: Duration::from_secs(RETRY_MAXIMUM_SECONDS),
                max_attempts: DEFAULT_MAX_ATTEMPTS,
            },
        }
    }
}
