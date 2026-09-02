use std::fs;
use std::num::{NonZeroU16, NonZeroU32};
use std::path::{Path, PathBuf};
use std::time::Duration;

use argon2::password_hash::PasswordHash;

use super::raw::RawControllerConfig;
use super::{
    AuthConfig, ConfigError, ControllerConfig, PathConfig, RetryConfig, SchedulerConfig,
    ServerConfig, TimeoutConfig,
};

pub(super) fn build_config(raw: RawControllerConfig) -> Result<ControllerConfig, ConfigError> {
    validate_roots("paths.input_roots", &raw.paths.input_roots)?;
    validate_roots("paths.output_roots", &raw.paths.output_roots)?;
    validate_directory("paths.data_root", &raw.paths.data_root)?;
    validate_directory("paths.temp_root", &raw.paths.temp_root)?;
    validate_hash_file(&raw.auth.password_hash_file)?;
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
            input_roots: raw.paths.input_roots,
            output_roots: raw.paths.output_roots,
            data_root: raw.paths.data_root,
            temp_root: raw.paths.temp_root,
        },
        auth: AuthConfig {
            password_hash_file: raw.auth.password_hash_file,
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

fn validate_roots(field: &'static str, paths: &[PathBuf]) -> Result<(), ConfigError> {
    if paths.is_empty() {
        return Err(ConfigError::InvalidRoot {
            field,
            path: PathBuf::new(),
            reason: "at least one root is required",
        });
    }
    for path in paths {
        validate_directory(field, path)?;
    }
    Ok(())
}

fn validate_directory(field: &'static str, path: &Path) -> Result<(), ConfigError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ConfigError::InvalidRoot {
        field,
        path: path.to_path_buf(),
        reason: "path does not exist",
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ConfigError::InvalidRoot {
            field,
            path: path.to_path_buf(),
            reason: "path must be a non-symlink directory",
        });
    }
    Ok(())
}

fn validate_hash_file(path: &Path) -> Result<(), ConfigError> {
    let valid = fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink());
    if !valid {
        return Err(ConfigError::MissingPasswordHashFile {
            path: path.to_path_buf(),
        });
    }
    let encoded = fs::read_to_string(path).map_err(|_| ConfigError::MissingPasswordHashFile {
        path: path.to_path_buf(),
    })?;
    let hash = PasswordHash::new(encoded.trim()).map_err(|_| ConfigError::InvalidPasswordHash {
        path: path.to_path_buf(),
    })?;
    if hash.algorithm.as_str() != "argon2id" {
        return Err(ConfigError::InvalidPasswordHash {
            path: path.to_path_buf(),
        });
    }
    Ok(())
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
