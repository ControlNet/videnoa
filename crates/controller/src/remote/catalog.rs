use crate::domain::{WorkflowKind, WorkflowName};

use super::{
    CompatibilityCatalog, Preset, RunValidation, VidenoaClient, VidenoaClientError, Workflow,
    WorkflowInterface,
};

impl VidenoaClient {
    /// Lists saved remote workflows.
    ///
    /// # Errors
    /// Returns [`VidenoaClientError`] for transport, status, bounds, or payload failures.
    pub async fn workflows(&self) -> Result<Vec<Workflow>, VidenoaClientError> {
        let response = self
            .send_authenticated(self.http.get(self.endpoint(&["api", "workflows"])?))
            .await?;
        self.json(response).await
    }

    /// Lists bundled and user-created remote presets.
    ///
    /// # Errors
    /// Returns [`VidenoaClientError`] for transport, status, bounds, or payload failures.
    pub async fn presets(&self) -> Result<Vec<Preset>, VidenoaClientError> {
        let response = self
            .send_authenticated(self.http.get(self.endpoint(&["api", "presets"])?))
            .await?;
        self.json(response).await
    }

    /// Fetches one workflow or preset interface by exact remote name.
    ///
    /// # Errors
    /// Returns [`VidenoaClientError`] for transport, status, bounds, or payload failures.
    pub async fn workflow_interface(
        &self,
        name: &WorkflowName,
    ) -> Result<WorkflowInterface, VidenoaClientError> {
        let response = self
            .send_authenticated(self.http.get(self.endpoint(&[
                "api",
                "workflows",
                name.as_str(),
                "interface",
            ])?))
            .await?;
        self.json(response).await
    }

    /// Fetches and merges saved workflows and presets with interface
    /// compatibility, then excludes eligible entries the Worker's own run
    /// validation rejects. Missing validation evidence keeps an entry eligible.
    ///
    /// # Errors
    /// Returns [`VidenoaClientError`] for transport, status, bounds, or payload
    /// failures of the workflow and preset listings.
    pub async fn capabilities(&self) -> Result<CompatibilityCatalog, VidenoaClientError> {
        let workflows = self.workflows().await?;
        let presets = self.presets().await?;
        let mut catalog = CompatibilityCatalog::default();
        for workflow in workflows {
            let interface = if workflow.has_interface {
                self.optional_interface(&workflow.filename).await?
            } else {
                None
            };
            catalog.insert(
                workflow.filename,
                WorkflowKind::Workflow,
                interface.as_ref(),
            );
        }
        for preset in presets {
            catalog.insert(
                preset.id,
                WorkflowKind::Preset,
                preset.workflow.interface.as_ref(),
            );
        }
        for name in catalog.eligible_names() {
            match self.validate_run(&name).await {
                Ok(RunValidation::Invalid { reason }) => catalog.mark_invalid(&name, reason),
                Ok(RunValidation::Valid | RunValidation::Unknown) => {}
                Err(error) => tracing::warn!(
                    workflow = %name.as_str(),
                    %error,
                    "worker workflow validation unavailable; keeping it eligible"
                ),
            }
        }
        Ok(catalog)
    }

    async fn optional_interface(
        &self,
        name: &WorkflowName,
    ) -> Result<Option<WorkflowInterface>, VidenoaClientError> {
        match self.workflow_interface(name).await {
            Ok(interface) => Ok(Some(interface)),
            Err(VidenoaClientError::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    }
}
