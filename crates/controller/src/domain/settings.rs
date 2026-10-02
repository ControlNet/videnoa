use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{ComputeSlots, ConcurrencyLimit};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsPaths {
    pub data_root: PathBuf,
    pub cache_root: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerSettingsDto {
    pub host: std::net::IpAddr,
    pub port: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthSettingsDto {
    pub secure_cookie: bool,
    pub session_absolute_seconds: u64,
    pub session_idle_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TimeoutSettingsDto {
    pub health_seconds: u64,
    pub poll_seconds: u64,
    pub transfer_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetrySettingsDto {
    pub initial_seconds: u64,
    pub maximum_seconds: u64,
    pub max_attempts: u32,
}

/// Self-hosted iroh relays; empty uses the public N0 relays and discovery.
/// Saved values take effect when the Controller restarts.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IrohSettingsDto {
    pub relay_urls: Vec<String>,
}

/// Saved iroh relays and whether the running endpoint still uses others.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IrohSettingsResponse {
    pub relay_urls: Vec<String>,
    /// Kept apart from the path restart, which also locks the scheduler.
    pub restart_required: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulerStatus {
    pub paused: bool,
    pub default_compute_slots: ComputeSlots,
    pub prefetch_per_worker: u16,
    pub max_concurrent_uploads: ConcurrencyLimit,
    pub max_concurrent_downloads: ConcurrencyLimit,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsResponse {
    pub version: u64,
    pub paths: SettingsPaths,
    pub restart_required: bool,
    pub server: ServerSettingsDto,
    pub secure_cookie: bool,
    pub session_absolute_seconds: u64,
    pub session_idle_seconds: u64,
    pub scheduler: SchedulerStatus,
    pub timeouts: TimeoutSettingsDto,
    pub retry: RetrySettingsDto,
    pub iroh: IrohSettingsResponse,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsUpdateRequest {
    pub version: u64,
    pub paths: SettingsPaths,
    pub server: ServerSettingsDto,
    pub auth: AuthSettingsDto,
    pub scheduler: SchedulerStatus,
    pub timeouts: TimeoutSettingsDto,
    pub retry: RetrySettingsDto,
    /// Omitted keeps the saved relays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iroh: Option<IrohSettingsDto>,
}
