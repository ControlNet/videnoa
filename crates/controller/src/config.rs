use std::net::{IpAddr, Ipv4Addr};
use std::num::{NonZeroU16, NonZeroU32};
use std::path::PathBuf;
use std::time::Duration;

#[path = "config/atomic_file.rs"]
mod atomic_file;
#[path = "config/document.rs"]
mod document;
#[path = "config/bootstrap.rs"]
mod local;
#[path = "config/private.rs"]
mod private;
#[path = "config/raw.rs"]
mod raw;
#[path = "config/server_override.rs"]
mod server_override;
#[path = "config/listener.rs"]
mod serving;
#[path = "config/validate.rs"]
mod validate;

#[path = "config/settings.rs"]
mod policy_dto;
#[path = "config/manager.rs"]
mod runtime_owner;
pub use local::ConfigBootstrap;
pub use policy_dto::{PolicyUpdate, SettingsRecord, SettingsUpdate};
pub use runtime_owner::ConfigManager;
pub use serving::{
    listener_channel, serve_reconfigurable, ListenerHandle, ListenerReceiver, PreparedListener,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ServerOverride {
    pub host: Option<IpAddr>,
    pub port: Option<NonZeroU16>,
}

const DEFAULT_MAX_ATTEMPTS: NonZeroU32 = NonZeroU32::MIN.saturating_add(4);
const SESSION_ABSOLUTE_SECONDS: u64 = 24 * 60 * 60;
const SESSION_IDLE_SECONDS: u64 = 60 * 60;
const TRANSFER_TIMEOUT_SECONDS: u64 = 5 * 60;
const RETRY_MAXIMUM_SECONDS: u64 = 60;

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
    #[error("configuration file is missing or invalid: {path}")]
    MissingConfigFile { path: PathBuf },
    #[error("configuration I/O failed at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("configuration schema is invalid: {detail}")]
    Schema { detail: String },
    #[error("configuration root `{field}` is invalid at {path}: {reason}")]
    InvalidRoot {
        field: &'static str,
        path: PathBuf,
        reason: &'static str,
    },
    #[error("temporary root {temp_root} overlaps output root {output_root}")]
    OverlappingPublicationRoots {
        temp_root: PathBuf,
        output_root: PathBuf,
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

impl Default for ControllerConfig {
    fn default() -> Self {
        let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            server: ServerConfig {
                host: IpAddr::V4(Ipv4Addr::LOCALHOST),
                port: 3001,
            },
            paths: PathConfig {
                input_roots: vec![workspace.clone()],
                output_roots: vec![workspace.clone()],
                data_root: workspace.join("data"),
                temp_root: workspace.join("data"),
            },
            auth: AuthConfig {
                secure_cookie: false,
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
