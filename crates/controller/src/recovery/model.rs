use std::time::Duration;

use crate::domain::TaskId;
use crate::remote::{PayloadLimits, RemoteTimeouts};
use crate::scheduler::RuntimeSettings;

use super::RecoveryCommandKind;

#[derive(Clone)]
pub struct RecoveryConfig {
    pub(crate) paths: crate::paths::PathCapabilities,
    pub(crate) limits: PayloadLimits,
    timeouts: RemoteTimeouts,
    health_initial: Duration,
    health_maximum: Duration,
    health_max_attempts: u32,
    runtime_settings: Option<RuntimeSettings>,
}

impl RecoveryConfig {
    #[must_use]
    pub fn new(
        paths: crate::paths::PathCapabilities,
        timeouts: RemoteTimeouts,
        limits: PayloadLimits,
        health_initial: Duration,
        health_maximum: Duration,
        health_max_attempts: u32,
    ) -> Self {
        Self {
            paths,
            timeouts,
            limits,
            health_initial,
            health_maximum,
            health_max_attempts,
            runtime_settings: None,
        }
    }

    #[must_use]
    pub fn with_runtime_settings(mut self, runtime_settings: RuntimeSettings) -> Self {
        self.runtime_settings = Some(runtime_settings);
        self
    }

    pub(crate) fn remote_timeouts(&self) -> RemoteTimeouts {
        self.runtime_settings
            .as_ref()
            .map_or(self.timeouts, RuntimeSettings::remote_timeouts)
    }

    pub(crate) fn health_retry(&self) -> (Duration, Duration, u32) {
        self.runtime_settings.as_ref().map_or(
            (
                self.health_initial,
                self.health_maximum,
                self.health_max_attempts,
            ),
            |settings| {
                let retry = settings.retry_settings();
                (
                    Duration::from_secs(retry.initial_seconds),
                    Duration::from_secs(retry.maximum_seconds),
                    retry.max_attempts,
                )
            },
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryTrace {
    pub task_id: TaskId,
    pub command: RecoveryCommandKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeferredRecovery {
    pub task_id: TaskId,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RecoveryReport {
    traces: Vec<RecoveryTrace>,
    deferred: Vec<DeferredRecovery>,
}

impl RecoveryReport {
    pub(crate) fn push(&mut self, task_id: TaskId, command: RecoveryCommandKind) {
        self.traces.push(RecoveryTrace { task_id, command });
    }

    pub(crate) fn defer(&mut self, task_id: TaskId) {
        self.deferred.push(DeferredRecovery { task_id });
    }

    #[must_use]
    pub fn traces(&self) -> &[RecoveryTrace] {
        &self.traces
    }

    #[must_use]
    pub fn deferred(&self) -> &[DeferredRecovery] {
        &self.deferred
    }

    #[must_use]
    pub fn command_kind(&self, task_id: TaskId) -> Option<RecoveryCommandKind> {
        self.traces
            .iter()
            .find(|trace| trace.task_id == task_id)
            .map(|trace| trace.command)
    }
}

#[cfg(test)]
mod tests {
    use std::{error::Error, fs, time::Duration};

    use tempfile::TempDir;

    use super::RecoveryConfig;
    use crate::config::PathConfig;
    use crate::domain::{RetrySettingsDto, TimeoutSettingsDto};
    use crate::paths::PathCapabilities;
    use crate::remote::{PayloadLimits, RemoteTimeouts};
    use crate::scheduler::RuntimeSettings;

    type TestResult<T = ()> = Result<T, Box<dyn Error>>;

    #[test]
    fn recovery_timeouts_use_hot_runtime_values_after_reconfiguration() -> TestResult {
        // Given: recovery was constructed with startup fallbacks and shared runtime settings.
        let runtime_settings = RuntimeSettings::new(&timeouts(2, 3, 4), &retry(5, 6, 7))?;
        let (_directory, config) = recovery_config(runtime_settings.clone())?;

        // When: settings are hot-reconfigured before the next recovery operation.
        runtime_settings.reconfigure(&timeouts(11, 13, 17), &retry(5, 6, 7))?;

        // Then: the next recovery client receives the current timeout snapshot.
        assert_eq!(
            config.remote_timeouts(),
            RemoteTimeouts::new(
                Duration::from_secs(11),
                Duration::from_secs(13),
                Duration::from_secs(17),
            )?
        );
        Ok(())
    }

    #[test]
    fn recovery_retry_uses_hot_runtime_values_after_reconfiguration() -> TestResult {
        // Given: recovery was constructed with startup fallbacks and shared runtime settings.
        let runtime_settings = RuntimeSettings::new(&timeouts(2, 3, 4), &retry(5, 6, 7))?;
        let (_directory, config) = recovery_config(runtime_settings.clone())?;

        // When: settings are hot-reconfigured before the next recovery operation.
        runtime_settings.reconfigure(&timeouts(2, 3, 4), &retry(19, 23, 29))?;

        // Then: the next worker deferral receives the current retry snapshot.
        assert_eq!(
            config.health_retry(),
            (Duration::from_secs(19), Duration::from_secs(23), 29)
        );
        Ok(())
    }

    fn recovery_config(runtime_settings: RuntimeSettings) -> TestResult<(TempDir, RecoveryConfig)> {
        let directory = TempDir::new()?;
        let input_root = directory.path().join("input");
        let output_root = directory.path().join("output");
        let data_root = directory.path().join("data");
        let temp_root = directory.path().join("temp");
        for path in [&input_root, &output_root, &data_root, &temp_root] {
            fs::create_dir_all(path)?;
        }
        let paths = PathCapabilities::open(&PathConfig {
            input_roots: vec![input_root],
            output_roots: vec![output_root],
            data_root,
            temp_root,
        })?;
        let config = RecoveryConfig::new(
            paths,
            RemoteTimeouts::new(
                Duration::from_secs(31),
                Duration::from_secs(37),
                Duration::from_secs(41),
            )?,
            PayloadLimits::new(1024, 512)?,
            Duration::from_secs(43),
            Duration::from_secs(47),
            53,
        )
        .with_runtime_settings(runtime_settings);
        Ok((directory, config))
    }

    const fn timeouts(
        health_seconds: u64,
        poll_seconds: u64,
        transfer_seconds: u64,
    ) -> TimeoutSettingsDto {
        TimeoutSettingsDto {
            health_seconds,
            poll_seconds,
            transfer_seconds,
        }
    }

    const fn retry(
        initial_seconds: u64,
        maximum_seconds: u64,
        max_attempts: u32,
    ) -> RetrySettingsDto {
        RetrySettingsDto {
            initial_seconds,
            maximum_seconds,
            max_attempts,
        }
    }
}
