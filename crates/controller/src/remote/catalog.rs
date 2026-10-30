use serde::de::DeserializeOwned;
use serde_json::Value;

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
    /// An unreadable entry or interface is listed invalid on its own.
    ///
    /// # Errors
    /// Returns [`VidenoaClientError`] for transport, status, bounds, or payload
    /// failures of the workflow and preset listings as a whole.
    pub async fn capabilities(&self) -> Result<CompatibilityCatalog, VidenoaClientError> {
        let workflows = self.listing("workflows").await?;
        let presets = self.listing("presets").await?;
        let mut catalog = CompatibilityCatalog::default();
        for entry in workflows {
            let Some(workflow) = listed::<Workflow>(
                entry,
                "filename",
                WorkflowKind::Workflow,
                UNREADABLE_WORKFLOW,
                &mut catalog,
            ) else {
                continue;
            };
            let interface = if workflow.has_interface {
                match self.optional_interface(&workflow.filename).await {
                    Ok(interface) => interface,
                    Err(
                        error @ (VidenoaClientError::MalformedPayload
                        | VidenoaClientError::OversizedPayload { .. }),
                    ) => {
                        tracing::warn!(workflow = %workflow.filename.as_str(), %error,
                            "worker workflow interface is unreadable; listing it invalid");
                        catalog.insert_invalid(
                            workflow.filename,
                            WorkflowKind::Workflow,
                            UNREADABLE_INTERFACE,
                        );
                        continue;
                    }
                    Err(error) => return Err(error),
                }
            } else {
                None
            };
            catalog.insert(
                workflow.filename,
                WorkflowKind::Workflow,
                interface.as_ref(),
            );
        }
        for entry in presets {
            let Some(preset) = listed::<Preset>(
                entry,
                "id",
                WorkflowKind::Preset,
                UNREADABLE_PRESET,
                &mut catalog,
            ) else {
                continue;
            };
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

    /// One catalog listing as separate entries, so one unreadable entry
    /// cannot hide the others.
    async fn listing(&self, route: &str) -> Result<Vec<Value>, VidenoaClientError> {
        let response = self
            .send_authenticated(self.http.get(self.endpoint(&["api", route])?))
            .await?;
        self.json(response).await
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

const UNREADABLE_WORKFLOW: &str =
    "the worker lists this workflow in a form this Controller cannot read";
const UNREADABLE_PRESET: &str = "the worker lists this preset in a form this Controller cannot read";
const UNREADABLE_INTERFACE: &str = "the worker reports an interface this Controller cannot read";

/// Reads one listing entry. An unreadable entry that still carries its name
/// under `name_key` is listed invalid; one without a name is skipped.
fn listed<T: DeserializeOwned>(
    entry: Value,
    name_key: &str,
    kind: WorkflowKind,
    reason: &str,
    catalog: &mut CompatibilityCatalog,
) -> Option<T> {
    let name = entry
        .get(name_key)
        .and_then(Value::as_str)
        .map(WorkflowName::new);
    match serde_json::from_value(entry) {
        Ok(value) => Some(value),
        Err(error) => {
            tracing::warn!(workflow = name.as_ref().map_or("", WorkflowName::as_str),
                ?kind, %error, "worker catalog entry is unreadable; listing it invalid");
            if let Some(name) = name {
                catalog.insert_invalid(name, kind, reason);
            }
            None
        }
    }
}
