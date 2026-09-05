use std::time::Duration;

use crate::config::{
    AuthConfig, ControllerConfig, PathConfig, RetryConfig, SchedulerConfig, ServerConfig,
    TimeoutConfig,
};
use crate::domain::SettingsUpdateRequest;

use super::OperationsError;

const MAX_DURATION_SECONDS: u64 = 7 * 24 * 60 * 60;
const MAX_RETRY_ATTEMPTS: u32 = 100;

pub(super) fn build_config(
    paths: &PathConfig,
    request: &SettingsUpdateRequest,
) -> Result<ControllerConfig, OperationsError> {
    Ok(ControllerConfig {
        server: ServerConfig {
            host: request.server.host,
            port: request.server.port,
        },
        paths: paths.clone(),
        auth: AuthConfig {
            secure_cookie: request.auth.secure_cookie,
            session_absolute: Duration::from_secs(request.auth.session_absolute_seconds),
            session_idle: Duration::from_secs(request.auth.session_idle_seconds),
        },
        scheduler: SchedulerConfig {
            paused: request.scheduler.paused,
            default_compute_slots: nonzero_u16(request.scheduler.default_compute_slots.get())?,
            prefetch_per_worker: request.scheduler.prefetch_per_worker,
            max_concurrent_uploads: nonzero_u16(request.scheduler.max_concurrent_uploads.get())?,
            max_concurrent_downloads: nonzero_u16(
                request.scheduler.max_concurrent_downloads.get(),
            )?,
        },
        timeouts: TimeoutConfig {
            health: Duration::from_secs(request.timeouts.health_seconds),
            poll: Duration::from_secs(request.timeouts.poll_seconds),
            transfer: Duration::from_secs(request.timeouts.transfer_seconds),
        },
        retry: RetryConfig {
            initial: Duration::from_secs(request.retry.initial_seconds),
            maximum: Duration::from_secs(request.retry.maximum_seconds),
            max_attempts: std::num::NonZeroU32::new(request.retry.max_attempts)
                .ok_or(OperationsError::InvalidRequest)?,
        },
    })
}

pub(super) fn validate(request: &SettingsUpdateRequest) -> Result<(), OperationsError> {
    if request.server.port == 0 {
        return Err(OperationsError::InvalidField(
            "server.port",
            "value must be greater than zero",
        ));
    }
    validate_duration(
        "session_absolute_seconds",
        request.auth.session_absolute_seconds,
    )?;
    validate_duration("session_idle_seconds", request.auth.session_idle_seconds)?;
    if request.auth.session_idle_seconds > request.auth.session_absolute_seconds {
        return Err(OperationsError::InvalidField(
            "auth",
            "idle lifetime must not exceed absolute lifetime",
        ));
    }
    validate_duration("health_seconds", request.timeouts.health_seconds)?;
    validate_duration("poll_seconds", request.timeouts.poll_seconds)?;
    validate_duration("transfer_seconds", request.timeouts.transfer_seconds)?;
    validate_duration("retry.initial_seconds", request.retry.initial_seconds)?;
    validate_duration("retry.maximum_seconds", request.retry.maximum_seconds)?;
    if request.retry.initial_seconds > request.retry.maximum_seconds {
        return Err(OperationsError::InvalidField(
            "retry",
            "initial delay must not exceed maximum delay",
        ));
    }
    if request.retry.max_attempts == 0 || request.retry.max_attempts > MAX_RETRY_ATTEMPTS {
        return Err(OperationsError::InvalidField(
            "max_attempts",
            "value must be between 1 and 100",
        ));
    }
    Ok(())
}

fn nonzero_u16(value: u16) -> Result<std::num::NonZeroU16, OperationsError> {
    std::num::NonZeroU16::new(value).ok_or(OperationsError::InvalidRequest)
}

fn validate_duration(field: &'static str, value: u64) -> Result<(), OperationsError> {
    if value == 0 || value > MAX_DURATION_SECONDS {
        return Err(OperationsError::InvalidField(
            field,
            "value must be between one second and seven days",
        ));
    }
    Ok(())
}
