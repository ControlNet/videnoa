use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;

use crate::domain::{RetrySettingsDto, TimeoutSettingsDto};
use crate::lifecycle::RetryPolicy;
use crate::remote::{ClientConfigError, RemoteTimeouts};

#[derive(Clone, Debug)]
struct RuntimeSnapshot {
    timeouts: RemoteTimeouts,
    retry_policy: RetryPolicy,
    timeout_settings: TimeoutSettingsDto,
    retry_settings: RetrySettingsDto,
}

#[derive(Clone, Debug)]
pub struct RuntimeSettings {
    snapshot: Arc<RwLock<RuntimeSnapshot>>,
}

impl RuntimeSettings {
    /// Creates live timeout and retry policy state from persisted settings.
    ///
    /// # Errors
    /// Returns a client configuration error when a persisted timeout is zero.
    pub fn new(
        timeouts: &TimeoutSettingsDto,
        retry: &RetrySettingsDto,
    ) -> Result<Self, ClientConfigError> {
        Ok(Self {
            snapshot: Arc::new(RwLock::new(snapshot(timeouts, retry)?)),
        })
    }

    /// Replaces live timeout and retry policy state after a durable settings commit.
    ///
    /// # Errors
    /// Returns a client configuration error when a timeout is zero.
    pub fn reconfigure(
        &self,
        timeouts: &TimeoutSettingsDto,
        retry: &RetrySettingsDto,
    ) -> Result<(), ClientConfigError> {
        *write(&self.snapshot) = snapshot(timeouts, retry)?;
        Ok(())
    }

    #[must_use]
    pub fn remote_timeouts(&self) -> RemoteTimeouts {
        read(&self.snapshot).timeouts
    }

    #[must_use]
    pub fn retry_policy(&self) -> RetryPolicy {
        read(&self.snapshot).retry_policy
    }

    #[must_use]
    pub fn timeout_settings(&self) -> TimeoutSettingsDto {
        read(&self.snapshot).timeout_settings.clone()
    }

    #[must_use]
    pub fn retry_settings(&self) -> RetrySettingsDto {
        read(&self.snapshot).retry_settings.clone()
    }
}

fn snapshot(
    timeouts: &TimeoutSettingsDto,
    retry: &RetrySettingsDto,
) -> Result<RuntimeSnapshot, ClientConfigError> {
    Ok(RuntimeSnapshot {
        timeouts: RemoteTimeouts::new(
            Duration::from_secs(timeouts.health_seconds),
            Duration::from_secs(timeouts.poll_seconds),
            Duration::from_secs(timeouts.transfer_seconds),
        )?,
        retry_policy: RetryPolicy::from_settings(retry),
        timeout_settings: timeouts.clone(),
        retry_settings: retry.clone(),
    })
}

fn read(lock: &RwLock<RuntimeSnapshot>) -> RwLockReadGuard<'_, RuntimeSnapshot> {
    match lock.read() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn write(lock: &RwLock<RuntimeSnapshot>) -> RwLockWriteGuard<'_, RuntimeSnapshot> {
    match lock.write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
