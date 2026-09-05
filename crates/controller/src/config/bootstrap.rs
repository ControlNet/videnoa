use std::fs;
use std::path::{Path, PathBuf};

use super::{atomic_file, private, ConfigError, ControllerConfig};
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
    /// configuration file cannot be written, or the configuration document is invalid.
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
                atomic_file::replace(&config_file, &source)?;
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

    /// Atomically replaces the workspace-local configuration file.
    ///
    /// # Errors
    /// Returns an error when the configuration cannot be written or synchronized.
    pub fn persist(&self, source: &str) -> Result<(), ConfigError> {
        atomic_file::replace(&self.config_file, source)
    }

    /// Repairs the workspace-local configuration file from durable content.
    ///
    /// # Errors
    /// Returns an error when the data directory or configuration cannot be written or synchronized.
    pub fn persist_document(workspace: &Path, source: &str) -> Result<(), ConfigError> {
        let workspace = fs::canonicalize(workspace).map_err(|source| ConfigError::Io {
            path: workspace.to_path_buf(),
            source,
        })?;
        let data_root = private::prepare_data_root(&workspace)?;
        atomic_file::replace(&data_root.join("controller.toml"), source)
    }

    /// Installs the startup TOML snapshot into the shared runtime manager.
    /// `SQLite` is deliberately not read or written.
    ///
    /// # Errors
    /// Returns an error when the runtime configuration cannot be initialized.
    pub fn initialize(&self, store: &Store) -> Result<ControllerConfig, ConfigError> {
        store
            .config_manager()
            .initialize(self.config.clone(), Some(self.workspace.clone()));
        Ok(self.config.clone())
    }
}
