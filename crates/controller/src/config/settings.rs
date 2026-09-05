use chrono::{DateTime, Utc};

use crate::domain::{
    AuthSettingsDto, RetrySettingsDto, SchedulerStatus, ServerSettingsDto, TimeoutSettingsDto,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsRecord {
    pub version: u64,
    pub server: ServerSettingsDto,
    pub auth: AuthSettingsDto,
    pub scheduler: SchedulerStatus,
    pub timeouts: TimeoutSettingsDto,
    pub retry: RetrySettingsDto,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsUpdate {
    pub expected_version: u64,
    pub scheduler: SchedulerStatus,
    pub timeouts: TimeoutSettingsDto,
    pub retry: RetrySettingsDto,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyUpdate {
    pub expected_version: u64,
    pub server: ServerSettingsDto,
    pub auth: AuthSettingsDto,
    pub scheduler: SchedulerStatus,
    pub timeouts: TimeoutSettingsDto,
    pub retry: RetrySettingsDto,
    pub updated_at: DateTime<Utc>,
}
