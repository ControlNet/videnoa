use super::{ConfigBootstrap, ConfigError, ControllerConfig, ServerOverride};
use crate::persistence::Store;

impl ConfigBootstrap {
    /// Applies CLI listener overrides directly to TOML before initializing runtime config.
    ///
    /// # Errors
    /// Returns an error when the configuration cannot be persisted.
    pub fn initialize_with_server_override(
        &self,
        store: &Store,
        server_override: &ServerOverride,
    ) -> Result<ControllerConfig, ConfigError> {
        let mut config = self.config().clone();
        if let Some(host) = server_override.host {
            config.server.host = host;
        }
        if let Some(port) = server_override.port {
            config.server.port = port.get();
        }
        if config.server != self.config().server {
            self.persist(&config.to_toml()?)?;
        }
        store
            .config_manager()
            .initialize(config.clone(), Some(self.workspace().to_path_buf()));
        Ok(config)
    }
}
