use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;

use super::{persistence, private, projection, ConfigError, ControllerConfig};
use crate::persistence::Store;

#[derive(Clone, Debug)]
pub struct ConfigBootstrap {
    config: ControllerConfig,
    workspace: PathBuf,
    config_file: PathBuf,
    source: String,
}

impl ConfigBootstrap {
    /// Opens the workspace-local configuration, creating defaults when needed.
    ///
    /// # Errors
    /// Returns an error when the workspace cannot be canonicalized, the data directory or
    /// configuration projection cannot be written, or the configuration document is invalid.
    pub fn open(workspace: &Path) -> Result<Self, ConfigError> {
        let workspace = fs::canonicalize(workspace).map_err(|source| ConfigError::Io {
            path: workspace.to_path_buf(),
            source,
        })?;
        let data_root = private::prepare_data_root(&workspace)?;
        let config_file = data_root.join("controller.toml");
        private::prepare_config_file(&config_file)?;
        let existing = match fs::read_to_string(&config_file) {
            Ok(source) => Some(source),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(source) => {
                return Err(ConfigError::Io {
                    path: config_file,
                    source,
                });
            }
        };
        let source = match existing {
            Some(source) if !source.trim().is_empty() => source,
            Some(_) | None => {
                let config = ControllerConfig::for_workspace(&workspace)?;
                let source = config.to_toml()?;
                projection::replace(&config_file, &source)?;
                source
            }
        };
        let config = ControllerConfig::from_toml_in(&source, &workspace)?;
        Ok(Self {
            config,
            workspace,
            config_file,
            source,
        })
    }

    /// Prepares the private workspace-local data boundary without reading configuration.
    ///
    /// # Errors
    /// Returns an error when the workspace or data directory is redirected or inaccessible.
    pub fn prepare_data_root(workspace: &Path) -> Result<PathBuf, ConfigError> {
        let workspace = fs::canonicalize(workspace).map_err(|source| ConfigError::Io {
            path: workspace.to_path_buf(),
            source,
        })?;
        private::prepare_data_root(&workspace)
    }

    #[must_use]
    pub const fn config(&self) -> &ControllerConfig {
        &self.config
    }

    #[must_use]
    pub fn into_config(self) -> ControllerConfig {
        self.config
    }

    #[must_use]
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    #[must_use]
    pub fn config_file(&self) -> &Path {
        &self.config_file
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Atomically replaces the workspace-local configuration projection.
    ///
    /// # Errors
    /// Returns an error when the projection cannot be written or synchronized.
    pub fn project(&self, source: &str) -> Result<(), ConfigError> {
        projection::replace(&self.config_file, source)
    }

    /// Repairs the workspace-local configuration projection from durable content.
    ///
    /// # Errors
    /// Returns an error when the data directory or projection cannot be written or synchronized.
    pub fn repair_projection(workspace: &Path, source: &str) -> Result<(), ConfigError> {
        let workspace = fs::canonicalize(workspace).map_err(|source| ConfigError::Io {
            path: workspace.to_path_buf(),
            source,
        })?;
        let data_root = private::prepare_data_root(&workspace)?;
        projection::replace(&data_root.join("controller.toml"), source)
    }

    /// Reconciles the local projection with the authoritative durable configuration.
    ///
    /// # Errors
    /// Returns an error when persistence, projection, or configuration parsing fails.
    pub async fn reconcile(&self, store: &Store) -> Result<ControllerConfig, ConfigError> {
        let record = store
            .settings()
            .await
            .map_err(|error| persistence(&error))?;
        if let Some(pending) = &record.pending_config_document {
            self.project(pending)?;
            store
                .complete_config_projection(record.version)
                .await
                .map_err(|error| persistence(&error))?;
            return ControllerConfig::from_toml_in(pending, &self.workspace);
        }
        if !record.configuration_initialized {
            let update = self
                .config
                .settings_update(record.version, &self.source, Utc::now())?;
            let initialized = store
                .initialize_settings(&update)
                .await
                .map_err(|error| persistence(&error))?;
            return ControllerConfig::from_record(&initialized, &self.workspace);
        }
        if record.config_document != self.source {
            let update = self
                .config
                .settings_update(record.version, &self.source, Utc::now())?;
            let imported = store
                .import_settings(&update)
                .await
                .map_err(|error| persistence(&error))?;
            return ControllerConfig::from_record(&imported, &self.workspace);
        }
        ControllerConfig::from_record(&record, &self.workspace)
    }

}
