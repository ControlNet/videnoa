use std::num::{NonZeroU16, NonZeroU32};
use std::path::Path;
use std::time::Duration;

use super::raw::RawControllerConfig;
use super::{
    AuthConfig, ConfigError, ControllerConfig, PathConfig, RetryConfig, SchedulerConfig,
    ServerConfig, TimeoutConfig,
};

pub(super) fn build_config(
    raw: &RawControllerConfig,
    workspace: &Path,
) -> Result<ControllerConfig, ConfigError> {
    let session_absolute = positive_duration(
        "auth.session_absolute_seconds",
        raw.auth.session_absolute_seconds,
    )?;
    let session_idle =
        positive_duration("auth.session_idle_seconds", raw.auth.session_idle_seconds)?;
    if session_idle > session_absolute {
        return Err(ConfigError::InvalidSessionBounds);
    }
    let initial = positive_duration("retry.initial_seconds", raw.retry.initial_seconds)?;
    let maximum = positive_duration("retry.maximum_seconds", raw.retry.maximum_seconds)?;
    if initial > maximum {
        return Err(ConfigError::InvalidRetryBounds {
            initial: raw.retry.initial_seconds,
            maximum: raw.retry.maximum_seconds,
        });
    }
    Ok(ControllerConfig {
        server: ServerConfig {
            host: raw.server.host,
            port: positive_u16("server.port", raw.server.port)?,
        },
        paths: PathConfig {
            input_roots: vec![workspace.to_path_buf()],
            output_roots: vec![workspace.to_path_buf()],
            data_root: workspace.join("data"),
            temp_root: workspace.join("data"),
        },
        auth: AuthConfig {
            secure_cookie: raw.auth.secure_cookie,
            session_absolute,
            session_idle,
        },
        scheduler: SchedulerConfig {
            paused: raw.scheduler.paused,
            default_compute_slots: positive_nonzero_u16(
                "scheduler.default_compute_slots",
                raw.scheduler.default_compute_slots,
            )?,
            prefetch_per_worker: checked_u16(
                "scheduler.prefetch_per_worker",
                raw.scheduler.prefetch_per_worker,
            )?,
            max_concurrent_uploads: positive_nonzero_u16(
                "scheduler.max_concurrent_uploads",
                raw.scheduler.max_concurrent_uploads,
            )?,
            max_concurrent_downloads: positive_nonzero_u16(
                "scheduler.max_concurrent_downloads",
                raw.scheduler.max_concurrent_downloads,
            )?,
        },
        timeouts: TimeoutConfig {
            health: positive_duration("timeouts.health_seconds", raw.timeouts.health)?,
            poll: positive_duration("timeouts.poll_seconds", raw.timeouts.poll)?,
            transfer: positive_duration("timeouts.transfer_seconds", raw.timeouts.transfer)?,
        },
        retry: RetryConfig {
            initial,
            maximum,
            max_attempts: positive_nonzero_u32("retry.max_attempts", raw.retry.max_attempts)?,
        },
    })
}

fn positive_duration(field: &'static str, value: u64) -> Result<Duration, ConfigError> {
    if value == 0 {
        return Err(ConfigError::ZeroValue { field });
    }
    Ok(Duration::from_secs(value))
}

fn checked_u16(field: &'static str, value: u64) -> Result<u16, ConfigError> {
    u16::try_from(value).map_err(|_| ConfigError::NumericOverflow {
        field,
        value,
        maximum: u64::from(u16::MAX),
    })
}

fn positive_u16(field: &'static str, value: u64) -> Result<u16, ConfigError> {
    let value = checked_u16(field, value)?;
    if value == 0 {
        return Err(ConfigError::ZeroValue { field });
    }
    Ok(value)
}

fn positive_nonzero_u16(field: &'static str, value: u64) -> Result<NonZeroU16, ConfigError> {
    NonZeroU16::new(positive_u16(field, value)?).ok_or(ConfigError::ZeroValue { field })
}

fn positive_nonzero_u32(field: &'static str, value: u64) -> Result<NonZeroU32, ConfigError> {
    let value = u32::try_from(value).map_err(|_| ConfigError::NumericOverflow {
        field,
        value,
        maximum: u64::from(u32::MAX),
    })?;
    NonZeroU32::new(value).ok_or(ConfigError::ZeroValue { field })
}
