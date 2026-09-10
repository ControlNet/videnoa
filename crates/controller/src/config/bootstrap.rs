use std::fs;
use std::path::{Path, PathBuf};

use super::{atomic_file, private, root_locator, ConfigError, ControllerConfig};
use crate::persistence::Store;

#[derive(Clone, Debug)]
pub struct ConfigBootstrap {
    config: ControllerConfig,
    workspace: PathBuf,
    retained_private_roots: Vec<PathBuf>,
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
        let bootstrap_root = private::prepare_data_root(&workspace)?;
        let located_root = root_locator::load(&bootstrap_root)?;
        if let Some(located_root) = &located_root {
            require_existing_data_root(located_root)?;
        }
        let mut active_data_root = located_root.unwrap_or_else(|| bootstrap_root.clone());
        let mut visited = std::collections::HashSet::new();

        loop {
            if !visited.insert(active_data_root.clone()) {
                return Err(ConfigError::InvalidRoot {
                    field: "paths.data_root",
                    path: active_data_root,
                    reason: "data root configuration contains a cycle",
                });
            }
            let prepared_active_root = private::prepare_root(&active_data_root, "data_root")?;
            let config_file = prepared_active_root.join("controller.toml");
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
                    let mut config = ControllerConfig::for_workspace(&workspace)?;
                    config.paths.data_root.clone_from(&prepared_active_root);
                    config.paths.temp_root.clone_from(&prepared_active_root);
                    let source = config.to_toml()?;
                    atomic_file::replace(&config_file, &source)?;
                    source
                }
            };
            let config = ControllerConfig::from_toml_in_with_path_defaults(
                &source,
                &workspace,
                &prepared_active_root,
                &prepared_active_root,
            )?;
            let data_root = config.paths.data_root.clone();
            if data_root == prepared_active_root {
                root_locator::persist(&bootstrap_root, &data_root)?;
                private::prepare_root(&config.paths.temp_root, "paths.cache_root")?;
                let mut retained_private_roots = root_locator::retained_sources(&data_root)?;
                if bootstrap_root != data_root && !retained_private_roots.contains(&bootstrap_root) {
                    retained_private_roots.push(bootstrap_root.clone());
                }
                return Ok(Self {
                    config,
                    workspace,
                    retained_private_roots,
                    config_file,
                    source,
                });
            }
            root_locator::migrate_data_root(&prepared_active_root, &data_root)?;
            root_locator::persist(&bootstrap_root, &data_root)?;
            active_data_root = data_root;
        }
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

    /// Returns prior data roots that still contain private Controller state.
    #[must_use]
    pub fn retained_private_roots(&self) -> &[PathBuf] {
        &self.retained_private_roots
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

    pub(crate) fn persist_document_at(path: &Path, source: &str) -> Result<(), ConfigError> {
        atomic_file::replace(path, source)
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

fn require_existing_data_root(path: &Path) -> Result<(), ConfigError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(ConfigError::InvalidRoot {
                field: "data_root_locator",
                path: path.to_path_buf(),
                reason: "configured data root is unavailable",
            })
        }
        Err(source) => Err(ConfigError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}
