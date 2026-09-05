use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use figment::providers::{Format, Serialized, Toml};
use figment::Figment;

use super::raw::RawControllerConfig;
use super::{raw, validate, ConfigError, ControllerConfig};
use crate::domain::{
    AuthSettingsDto, RetrySettingsDto, SchedulerStatus, ServerSettingsDto, TimeoutSettingsDto,
};
use crate::persistence::{ConfigurationUpdate, SettingsRecord};

impl ControllerConfig {
    /// Builds the default controller configuration for a workspace.
    ///
    /// # Errors
    /// Returns an error when the workspace cannot be canonicalized or defaults are invalid.
    pub fn for_workspace(workspace: &Path) -> Result<Self, ConfigError> {
        let workspace = fs::canonicalize(workspace).map_err(|source| ConfigError::Io {
            path: workspace.to_path_buf(),
            source,
        })?;
        validate::build_config(&RawControllerConfig::default(), &workspace)
    }

    /// Parses a controller configuration relative to the current directory.
    ///
    /// # Errors
    /// Returns an error when the current directory is unavailable or the document is invalid.
    pub fn from_toml(source: &str) -> Result<Self, ConfigError> {
        let workspace = std::env::current_dir().map_err(|source| ConfigError::Io {
            path: PathBuf::from("."),
            source,
        })?;
        Self::from_toml_in(source, &workspace)
    }

    /// Loads a controller configuration file, or workspace defaults when no path is supplied.
    ///
    /// # Errors
    /// Returns an error when the file is missing, cannot be read, or contains invalid settings.
    pub fn load(path: Option<&Path>) -> Result<Self, ConfigError> {
        let Some(path) = path else {
            return Self::for_workspace(&std::env::current_dir().map_err(|source| {
                ConfigError::Io {
                    path: PathBuf::from("."),
                    source,
                }
            })?);
        };
        if !path.is_file() {
            return Err(ConfigError::MissingConfigFile {
                path: path.to_path_buf(),
            });
        }
        let source = fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let workspace = path
            .parent()
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new("."));
        Self::from_toml_in(&source, workspace)
    }

    /// Parses a controller configuration relative to an explicit workspace.
    ///
    /// # Errors
    /// Returns an error when the document schema or a typed configuration bound is invalid.
    pub fn from_toml_in(source: &str, workspace: &Path) -> Result<Self, ConfigError> {
        let figment = Figment::from(Serialized::defaults(RawControllerConfig::default()))
            .merge(Toml::string(source));
        let raw =
            figment
                .extract::<RawControllerConfig>()
                .map_err(|error| ConfigError::Schema {
                    detail: error.to_string(),
                })?;
        validate::build_config(&raw, workspace)
    }

    /// Serializes the public policy configuration as TOML.
    ///
    /// # Errors
    /// Returns an error when the typed configuration cannot be serialized.
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        toml::to_string_pretty(&RawControllerConfig::from(self)).map_err(|error| {
            ConfigError::Schema {
                detail: error.to_string(),
            }
        })
    }

    /// Builds the durable configuration update represented by this configuration.
    ///
    /// # Errors
    /// Returns an error when a scheduler limit cannot be represented by the domain DTO.
    pub fn settings_update(
        &self,
        expected_version: u64,
        config_document: &str,
        updated_at: DateTime<Utc>,
    ) -> Result<ConfigurationUpdate, ConfigError> {
        Ok(ConfigurationUpdate {
            expected_version,
            server: ServerSettingsDto {
                host: self.server.host,
                port: self.server.port,
            },
            auth: AuthSettingsDto {
                secure_cookie: self.auth.secure_cookie,
                session_absolute_seconds: self.auth.session_absolute.as_secs(),
                session_idle_seconds: self.auth.session_idle.as_secs(),
            },
            scheduler: SchedulerStatus {
                paused: self.scheduler.paused,
                default_compute_slots: crate::domain::ComputeSlots::try_from(u64::from(
                    self.scheduler.default_compute_slots.get(),
                ))
                .map_err(schema)?,
                prefetch_per_worker: self.scheduler.prefetch_per_worker,
                max_concurrent_uploads: crate::domain::ConcurrencyLimit::try_from(u64::from(
                    self.scheduler.max_concurrent_uploads.get(),
                ))
                .map_err(schema)?,
                max_concurrent_downloads: crate::domain::ConcurrencyLimit::try_from(u64::from(
                    self.scheduler.max_concurrent_downloads.get(),
                ))
                .map_err(schema)?,
            },
            timeouts: TimeoutSettingsDto {
                health_seconds: self.timeouts.health.as_secs(),
                poll_seconds: self.timeouts.poll.as_secs(),
                transfer_seconds: self.timeouts.transfer.as_secs(),
            },
            retry: RetrySettingsDto {
                initial_seconds: self.retry.initial.as_secs(),
                maximum_seconds: self.retry.maximum.as_secs(),
                max_attempts: self.retry.max_attempts.get(),
            },
            updated_at,
            config_document: config_document.to_owned(),
        })
    }

    /// Reconstructs typed configuration from a durable settings record.
    ///
    /// # Errors
    /// Returns an error when the record's configuration document is invalid.
    pub fn from_record(record: &SettingsRecord, workspace: &Path) -> Result<Self, ConfigError> {
        Self::from_toml_in(&record.config_document, workspace)
    }
}

impl From<&ControllerConfig> for RawControllerConfig {
    fn from(config: &ControllerConfig) -> Self {
        Self {
            server: raw::RawServerConfig {
                host: config.server.host,
                port: u64::from(config.server.port),
            },
            auth: raw::RawAuthConfig {
                secure_cookie: config.auth.secure_cookie,
                session_absolute_seconds: config.auth.session_absolute.as_secs(),
                session_idle_seconds: config.auth.session_idle.as_secs(),
            },
            scheduler: raw::RawSchedulerConfig {
                paused: config.scheduler.paused,
                default_compute_slots: u64::from(config.scheduler.default_compute_slots.get()),
                prefetch_per_worker: u64::from(config.scheduler.prefetch_per_worker),
                max_concurrent_uploads: u64::from(config.scheduler.max_concurrent_uploads.get()),
                max_concurrent_downloads: u64::from(
                    config.scheduler.max_concurrent_downloads.get(),
                ),
            },
            timeouts: raw::RawTimeoutConfig {
                health: config.timeouts.health.as_secs(),
                poll: config.timeouts.poll.as_secs(),
                transfer: config.timeouts.transfer.as_secs(),
            },
            retry: raw::RawRetryConfig {
                initial_seconds: config.retry.initial.as_secs(),
                maximum_seconds: config.retry.maximum.as_secs(),
                max_attempts: u64::from(config.retry.max_attempts.get()),
            },
        }
    }
}

fn schema(detail: &'static str) -> ConfigError {
    ConfigError::Schema {
        detail: detail.to_owned(),
    }
}
