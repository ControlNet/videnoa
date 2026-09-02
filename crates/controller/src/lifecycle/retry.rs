use std::time::Duration;

use crate::config::RetryConfig;
use crate::domain::RetrySettingsDto;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutomaticRetry {
    Upload,
    Download,
    Health,
    Cleanup,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct JitterSample(u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("jitter sample must be between 0 and 10000")]
pub struct JitterRangeError;

impl TryFrom<u16> for JitterSample {
    type Error = JitterRangeError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if value <= 10_000 {
            Ok(Self(value))
        } else {
            Err(JitterRangeError)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryDecision {
    Schedule {
        operation: AutomaticRetry,
        retry_count: u32,
        delay: Duration,
    },
    Exhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryPolicy {
    initial_seconds: u64,
    maximum_seconds: u64,
    max_attempts: u32,
}

impl RetryPolicy {
    #[must_use]
    pub fn from_config(config: &RetryConfig) -> Self {
        Self {
            initial_seconds: config.initial.as_secs(),
            maximum_seconds: config.maximum.as_secs(),
            max_attempts: config.max_attempts.get(),
        }
    }

    #[must_use]
    pub const fn from_settings(settings: &RetrySettingsDto) -> Self {
        Self {
            initial_seconds: settings.initial_seconds,
            maximum_seconds: settings.maximum_seconds,
            max_attempts: settings.max_attempts,
        }
    }

    #[must_use]
    pub fn decide(
        self,
        operation: AutomaticRetry,
        retry_count: u32,
        jitter: JitterSample,
    ) -> RetryDecision {
        if retry_count >= self.max_attempts {
            return RetryDecision::Exhausted;
        }
        let multiplier = 1_u64.checked_shl(retry_count.min(63)).unwrap_or(u64::MAX);
        let base = self
            .initial_seconds
            .saturating_mul(multiplier)
            .min(self.maximum_seconds);
        let lower = base / 2;
        let span = base - lower;
        let scaled = u128::from(span) * u128::from(jitter.0) / 10_000;
        let jittered = match u64::try_from(scaled) {
            Ok(value) => lower.saturating_add(value),
            Err(_) => base,
        };
        RetryDecision::Schedule {
            operation,
            retry_count: retry_count + 1,
            delay: Duration::from_secs(jittered.min(self.maximum_seconds)),
        }
    }
}
