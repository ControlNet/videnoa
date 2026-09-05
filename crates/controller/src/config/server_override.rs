use chrono::Utc;

use super::{persistence, ConfigBootstrap, ConfigError, ControllerConfig, ServerOverride};
use crate::persistence::{CasOutcome, Store};

impl ConfigBootstrap {
    /// Reconciles configuration and durably applies optional listener overrides.
    ///
    /// # Errors
    /// Returns an error when reconciliation, persistence, or projection fails.
    pub async fn reconcile_with_server_override(
        &self,
        store: &Store,
        server_override: &ServerOverride,
    ) -> Result<ControllerConfig, ConfigError> {
        let mut config = self.reconcile(store).await?;
        let original = config.server.clone();
        if let Some(host) = server_override.host {
            config.server.host = host;
        }
        if let Some(port) = server_override.port {
            config.server.port = port.get();
        }
        if config.server == original {
            return Ok(config);
        }
        let record = store
            .settings()
            .await
            .map_err(|error| persistence(&error))?;
        let document = config.to_toml()?;
        let update = config.settings_update(record.version, &document, Utc::now())?;
        let CasOutcome::Applied { new_version } = store
            .update_configuration(&update)
            .await
            .map_err(|error| persistence(&error))?
        else {
            return Err(ConfigError::Schema {
                detail: "startup listener override conflicted with durable settings".to_owned(),
            });
        };
        self.project(&document)?;
        if !store
            .complete_config_projection(new_version)
            .await
            .map_err(|error| persistence(&error))?
        {
            return Err(ConfigError::Schema {
                detail: "startup listener override projection journal changed".to_owned(),
            });
        }
        Ok(config)
    }
}
