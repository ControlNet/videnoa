use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawControllerConfig {
    pub server: RawServerConfig,
    pub paths: RawPathConfig,
    pub auth: RawAuthConfig,
    pub scheduler: RawSchedulerConfig,
    pub timeouts: RawTimeoutConfig,
    pub retry: RawRetryConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawServerConfig {
    pub host: IpAddr,
    pub port: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawPathConfig {
    pub input_roots: Vec<PathBuf>,
    pub output_roots: Vec<PathBuf>,
    pub data_root: PathBuf,
    pub temp_root: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawAuthConfig {
    pub password_hash_file: PathBuf,
    pub secure_cookie: bool,
    pub session_absolute_seconds: u64,
    pub session_idle_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawSchedulerConfig {
    pub paused: bool,
    pub default_compute_slots: u64,
    pub prefetch_per_worker: u64,
    pub max_concurrent_uploads: u64,
    pub max_concurrent_downloads: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawTimeoutConfig {
    #[serde(rename = "health_seconds")]
    pub health: u64,
    #[serde(rename = "poll_seconds")]
    pub poll: u64,
    #[serde(rename = "transfer_seconds")]
    pub transfer: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawRetryConfig {
    pub initial_seconds: u64,
    pub maximum_seconds: u64,
    pub max_attempts: u64,
}

impl Default for RawControllerConfig {
    fn default() -> Self {
        Self {
            server: RawServerConfig {
                host: IpAddr::V4(Ipv4Addr::LOCALHOST),
                port: 3001,
            },
            paths: RawPathConfig {
                input_roots: vec![PathBuf::from("input")],
                output_roots: vec![PathBuf::from("output")],
                data_root: PathBuf::from("data"),
                temp_root: PathBuf::from("data/temp"),
            },
            auth: RawAuthConfig {
                password_hash_file: PathBuf::from("data/admin-password.phc"),
                secure_cookie: true,
                session_absolute_seconds: 86_400,
                session_idle_seconds: 3_600,
            },
            scheduler: RawSchedulerConfig {
                paused: false,
                default_compute_slots: 1,
                prefetch_per_worker: 1,
                max_concurrent_uploads: 1,
                max_concurrent_downloads: 1,
            },
            timeouts: RawTimeoutConfig {
                health: 10,
                poll: 5,
                transfer: 300,
            },
            retry: RawRetryConfig {
                initial_seconds: 1,
                maximum_seconds: 60,
                max_attempts: 5,
            },
        }
    }
}
