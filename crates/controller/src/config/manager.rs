use std::path::PathBuf;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;

use chrono::Utc;

use super::{ConfigBootstrap, ConfigError, ControllerConfig, SettingsRecord, SettingsUpdate};
use crate::persistence::CasOutcome;

#[derive(Debug)]
struct State {
    config: ControllerConfig,
    generation: u64,
    updated_at: chrono::DateTime<Utc>,
    workspace: Option<PathBuf>,
}

/// Shared runtime configuration. TOML is its only durable backing store.
/// The admission lock serializes configuration commits with new work admission.
#[derive(Clone, Debug)]
pub struct ConfigManager {
    state: Arc<RwLock<State>>,
    admission: Arc<tokio::sync::RwLock<()>>,
}

impl Default for ConfigManager {
    fn default() -> Self {
        Self {
            state: Arc::new(RwLock::new(State {
                config: ControllerConfig::default(),
                generation: 0,
                updated_at: Utc::now(),
                workspace: None,
            })),
            admission: Arc::new(tokio::sync::RwLock::new(())),
        }
    }
}

impl ConfigManager {
    /// Installs a startup snapshot before runtime components start.
    /// `None` supplies ephemeral configuration for embedded callers and tests.
    pub fn initialize(&self, config: ControllerConfig, workspace: Option<PathBuf>) {
        *self.write() = State {
            config,
            workspace,
            generation: 0,
            updated_at: Utc::now(),
        };
    }

    #[must_use]
    pub fn config(&self) -> ControllerConfig {
        self.read().config.clone()
    }

    #[must_use]
    pub fn scheduler(&self) -> super::SchedulerConfig {
        self.read().config.scheduler.clone()
    }

    pub(crate) fn admission(&self) -> Arc<tokio::sync::RwLock<()>> {
        Arc::clone(&self.admission)
    }

    /// Returns the active in-memory policy and generation, without filesystem or SQL reads.
    ///
    /// # Errors
    /// Returns an error if a policy cannot be represented by its API DTO.
    pub fn settings(&self) -> Result<SettingsRecord, ConfigError> {
        let state = self.read();
        let dto = state
            .config
            .settings_update(state.generation, state.updated_at)?;
        Ok(SettingsRecord {
            version: state.generation,
            server: dto.server,
            auth: dto.auth,
            scheduler: dto.scheduler,
            timeouts: dto.timeouts,
            retry: dto.retry,
            updated_at: dto.updated_at,
        })
    }

    /// Persists scheduler policy using runtime generation CAS and admission synchronization.
    ///
    /// # Errors
    /// Returns an error on invalid policy or failed TOML persistence; runtime stays unchanged.
    pub async fn update_settings(
        &self,
        update: &SettingsUpdate,
    ) -> Result<CasOutcome, ConfigError> {
        let _admission = self.admission.clone().write_owned().await;
        self.update_settings_locked(update)
    }

    pub(crate) fn update_settings_locked(
        &self,
        update: &SettingsUpdate,
    ) -> Result<CasOutcome, ConfigError> {
        let mut config = self.config();
        config.scheduler.paused = update.scheduler.paused;
        config.scheduler.default_compute_slots = std::num::NonZeroU16::new(
            update.scheduler.default_compute_slots.get(),
        )
        .ok_or(ConfigError::ZeroValue {
            field: "default_compute_slots",
        })?;
        config.scheduler.prefetch_per_worker = update.scheduler.prefetch_per_worker;
        config.scheduler.max_concurrent_uploads = std::num::NonZeroU16::new(
            update.scheduler.max_concurrent_uploads.get(),
        )
        .ok_or(ConfigError::ZeroValue {
            field: "max_concurrent_uploads",
        })?;
        config.scheduler.max_concurrent_downloads = std::num::NonZeroU16::new(
            update.scheduler.max_concurrent_downloads.get(),
        )
        .ok_or(ConfigError::ZeroValue {
            field: "max_concurrent_downloads",
        })?;
        config.timeouts.health = Duration::from_secs(update.timeouts.health_seconds);
        config.timeouts.poll = Duration::from_secs(update.timeouts.poll_seconds);
        config.timeouts.transfer = Duration::from_secs(update.timeouts.transfer_seconds);
        config.retry.initial = Duration::from_secs(update.retry.initial_seconds);
        config.retry.maximum = Duration::from_secs(update.retry.maximum_seconds);
        config.retry.max_attempts =
            std::num::NonZeroU32::new(update.retry.max_attempts).ok_or(ConfigError::ZeroValue {
                field: "max_attempts",
            })?;
        self.commit(config, update.expected_version)
    }

    /// Caller retains exclusive admission until dependent runtime components have been applied.
    pub(crate) fn commit(
        &self,
        config: ControllerConfig,
        expected: u64,
    ) -> Result<CasOutcome, ConfigError> {
        let mut state = self.write();
        if state.generation != expected {
            return Ok(CasOutcome::Conflict);
        }
        let document = config.to_toml()?;
        let workspace = config
            .paths
            .data_root
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        ControllerConfig::from_toml_in(&document, workspace)?;
        if let Some(workspace) = &state.workspace {
            ConfigBootstrap::persist_document(workspace, &document)?;
        }
        state.config = config;
        state.generation += 1;
        state.updated_at = Utc::now();
        Ok(CasOutcome::Applied {
            new_version: state.generation,
        })
    }

    pub(crate) fn pause_for_shutdown(&self) {
        let mut state = self.write();
        state.config.scheduler.paused = true;
        state.generation += 1;
    }

    fn read(&self) -> RwLockReadGuard<'_, State> {
        self.state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn write(&self) -> RwLockWriteGuard<'_, State> {
        self.state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
