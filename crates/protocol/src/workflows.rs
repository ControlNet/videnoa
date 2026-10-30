use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One entry of `GET /api/workflows`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WorkflowEntry {
    pub filename: String,
    pub name: String,
    pub description: String,
    pub workflow: Value,
    pub has_interface: bool,
}

/// One entry of `GET /api/presets`. `workflow` is the preset's graph as
/// stored, including its `interface` when it has one.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PresetResponse {
    pub id: String,
    pub name: String,
    pub description: String,
    pub workflow: Value,
}

/// `GET /api/workflows/{filename}/interface`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkflowInterface {
    pub inputs: Vec<WorkflowPort>,
    pub outputs: Vec<WorkflowPort>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkflowPort {
    pub name: String,
    pub port_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<Value>,
}
