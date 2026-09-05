use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{ComputeSlots, ConcurrencyLimit};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsPaths {
    pub workspace: PathBuf,
    pub data_root: PathBuf,
    pub config_file: PathBuf,
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
    pub server: ServerSettingsDto,
    pub secure_cookie: bool,
    pub session_absolute_seconds: u64,
    pub session_idle_seconds: u64,
    pub scheduler: SchedulerStatus,
    pub timeouts: TimeoutSettingsDto,
    pub retry: RetrySettingsDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsUpdateRequest {
    pub version: u64,
    pub server: ServerSettingsDto,
    pub auth: AuthSettingsDto,
    pub scheduler: SchedulerStatus,
    pub timeouts: TimeoutSettingsDto,
    pub retry: RetrySettingsDto,
}
