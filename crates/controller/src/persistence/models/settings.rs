use chrono::{DateTime, Utc};

use crate::domain::{RetrySettingsDto, SchedulerStatus, TimeoutSettingsDto};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsRecord {
    pub version: u64,
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
