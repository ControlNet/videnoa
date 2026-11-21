//! Workflow I/O nodes: WorkflowInput, WorkflowOutput, and Workflow (nested execution).
//!
//! These nodes enable "workflow-as-function" — making workflows reusable and parameterizable.
//! WorkflowInput outputs injected params, WorkflowOutput collects results, and Workflow
//! loads and executes a nested workflow with circular reference detection.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};

use crate::executor::{port_data_from_json, SequentialExecutor};
use crate::graph::PipelineGraph;
use crate::node::{ExecutionContext, Node, PortDefinition};
use crate::registry::NodeRegistry;
use crate::types::{PortData, PortType};

// ─── WorkflowInputNode ──────────────────────────────────────────────────────

/// Entry point for parameterized workflows. Outputs injected params to downstream nodes.
/// Ports are dynamically configured from the workflow interface or frontend.
pub struct WorkflowInputNode {
    ports: Vec<PortDefinition>,
    injected: HashMap<String, serde_json::Value>,
}

impl WorkflowInputNode {
    pub fn new() -> Self {
        Self {
            ports: vec![],
            injected: HashMap::new(),
        }
    }

    pub fn with_ports(ports: Vec<PortDefinition>) -> Self {
        Self {
            ports,
            injected: HashMap::new(),
        }
    }

    pub fn from_params(params: &HashMap<String, serde_json::Value>) -> Self {
        let ports = params
            .get("ports")
            .and_then(|v| v.as_array())
            .map(|arr| parse_port_definitions(arr, true))
            .unwrap_or_default();
        let injected: HashMap<String, serde_json::Value> = params
            .iter()
            .filter(|(k, _)| *k != "ports")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Self { ports, injected }
    }
}

impl Default for WorkflowInputNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for WorkflowInputNode {
    fn node_type(&self) -> &str {
        "WorkflowInput"
    }

    /// Besides `ports`, every other param is a value injected for one of the
    /// ports, by the run dialogs, job params, the batch endpoint or preview.
    fn accepts_config_param(&self, _key: &str) -> bool {
        true
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![]
    }

    fn output_ports(&self) -> Vec<PortDefinition> {
        self.ports.clone()
    }

    fn execute(
        &mut self,
        inputs: &HashMap<String, PortData>,
        _ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        let mut outputs = HashMap::new();

        for port in &self.ports {
            if let Some(data) = inputs.get(&port.name) {
                outputs.insert(port.name.clone(), clone_port_data(data));
                continue;
            }

            if let Some(json_val) = self.injected.get(&port.name) {
                let data = port_data_from_json(&port.port_type, json_val).with_context(|| {
                    format!(
                        "WorkflowInput: failed to parse injected value for port '{}'",
                        port.name
                    )
                })?;
                outputs.insert(port.name.clone(), data);
                continue;
            }

            if let Some(ref default_val) = port.default_value {
                let data =
                    port_data_from_json(&port.port_type, default_val).with_context(|| {
                        format!(
                            "WorkflowInput: failed to parse default for port '{}'",
                            port.name
                        )
                    })?;
                outputs.insert(port.name.clone(), data);
                continue;
            }

            bail!(
                "WorkflowInput: missing value for port '{}' with no default",
                port.name
            );
        }

        Ok(outputs)
    }
}

// ─── WorkflowOutputNode ─────────────────────────────────────────────────────

/// Exit point for parameterized workflows. Collects input values as workflow results.
pub struct WorkflowOutputNode {
    ports: Vec<PortDefinition>,
}

impl WorkflowOutputNode {
    pub fn new() -> Self {
        Self { ports: vec![] }
    }

    pub fn with_ports(ports: Vec<PortDefinition>) -> Self {
        Self { ports }
    }

    pub fn from_params(params: &HashMap<String, serde_json::Value>) -> Self {
        let ports = params
            .get("ports")
            .and_then(|v| v.as_array())
            .map(|arr| parse_port_definitions(arr, false))
            .unwrap_or_default();
        Self { ports }
    }
}

impl Default for WorkflowOutputNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for WorkflowOutputNode {
    fn node_type(&self) -> &str {
        "WorkflowOutput"
    }

    fn accepts_config_param(&self, key: &str) -> bool {
        key == "ports"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        self.ports.clone()
    }

    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![]
    }

    fn execute(
        &mut self,
        inputs: &HashMap<String, PortData>,
        _ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        // Collect all input values into outputs so the executor can retrieve them.
        let mut results = HashMap::new();
        for port in &self.ports {
            if let Some(data) = inputs.get(&port.name) {
                results.insert(port.name.clone(), clone_port_data(data));
            }
        }
        Ok(results)
    }
}

// ─── WorkflowNode (nested execution) ────────────────────────────────────────

/// Executes a nested workflow as a single node. Input ports map to the inner
/// workflow's WorkflowInput, output ports map to the inner WorkflowOutput.
pub struct WorkflowNode {
    workflow_path: String,
    interface_inputs: Vec<PortDefinition>,
    interface_outputs: Vec<PortDefinition>,
}

impl WorkflowNode {
    pub fn new() -> Self {
        Self {
            workflow_path: String::new(),
            interface_inputs: vec![],
            interface_outputs: vec![],
        }
    }

    pub fn from_params(params: &HashMap<String, serde_json::Value>) -> Self {
        let workflow_path = params
            .get("workflow_path")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let interface_inputs = params
            .get("interface_inputs")
            .and_then(|v| v.as_array())
            .map(|arr| parse_workflow_interface_inputs(&workflow_path, arr))
            .unwrap_or_default();

        let interface_outputs = params
            .get("interface_outputs")
            .and_then(|v| v.as_array())
            .map(|arr| parse_port_definitions(arr, false))
            .unwrap_or_default();

        Self {
            workflow_path,
            interface_inputs,
            interface_outputs,
        }
    }
}

impl Default for WorkflowNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for WorkflowNode {
    fn node_type(&self) -> &str {
        "Workflow"
    }

    fn accepts_config_param(&self, key: &str) -> bool {
        matches!(
            key,
            "workflow_path" | "interface_inputs" | "interface_outputs"
        )
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        // The workflow_path is a config param, not a connection port.
        self.interface_inputs.clone()
    }

    fn output_ports(&self) -> Vec<PortDefinition> {
        self.interface_outputs.clone()
    }

    fn execute(
        &mut self,
        inputs: &HashMap<String, PortData>,
        ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        if self.workflow_path.is_empty() {
            bail!("Workflow node: workflow_path is empty");
        }

        let path = PathBuf::from(&self.workflow_path);

        if ctx.nesting_depth >= MAX_NESTING_DEPTH {
            bail!(
                "Workflow node: maximum nesting depth ({}) exceeded — workflow '{}' cannot be executed",
                MAX_NESTING_DEPTH,
                self.workflow_path
            );
        }

        // Circular reference detection
        if ctx.executing_workflows.contains(&path) {
            bail!(
                "Workflow node: circular reference detected — '{}' is already executing",
                self.workflow_path
            );
        }

        // A nested workflow is a live reference by design: the file is read
        // here, when the node runs, not when the outer job was submitted. A
        // queued job or a rerun therefore uses the file's current content.
        let graph = load_nested_workflow(&self.workflow_path)?;

        // Build inner registry (same as outer — reuse the standard set)
        let registry = crate::registry::build_default_registry();

        let mut inner_ctx = ExecutionContext {
            executing_workflows: ctx.executing_workflows.clone(),
            nesting_depth: ctx.nesting_depth + 1,
            cancellation: ctx.cancellation.clone(),
            scratch: ctx.scratch.clone(),
            staged_output_hook: ctx.staged_output_hook.clone(),
            ..ExecutionContext::default()
        };
        inner_ctx.executing_workflows.insert(path);

        // Inject our inputs as params for the inner WorkflowInput node
        let mut inner_params = HashMap::new();
        for (key, value) in inputs {
            inner_params.insert(key.clone(), clone_port_data(value));
        }

        let outputs =
            SequentialExecutor::execute_with_params(&graph, &registry, inner_params, &inner_ctx)?;

        // Collect results from the inner WorkflowOutput nodes only: other
        // nodes may have output ports with the same names.
        let mut results = HashMap::new();
        let output_nodes = graph
            .nodes()
            .filter(|node| node.node_type == "WorkflowOutput");
        for output_node in output_nodes {
            let Some(node_outputs) = outputs.get(&output_node.id) else {
                continue;
            };
            for (port_name, port_data) in node_outputs {
                if !self.interface_outputs.iter().any(|p| p.name == *port_name) {
                    continue;
                }
                if results
                    .insert(port_name.clone(), clone_port_data(port_data))
                    .is_some()
                {
                    bail!(
                        "Workflow node: '{}' has more than one WorkflowOutput for '{port_name}'",
                        self.workflow_path
                    );
                }
            }
        }

        Ok(results)
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

const MAX_NESTING_DEPTH: u32 = 10;

/// Load and parse the workflow a `Workflow` node runs. A preset envelope
/// (`{"workflow": {...}}`) is accepted too.
fn load_nested_workflow(workflow_path: &str) -> Result<PipelineGraph> {
    let json_str = std::fs::read_to_string(workflow_path)
        .with_context(|| format!("Workflow node: failed to read '{workflow_path}'"))?;
    let workflow_value: serde_json::Value = serde_json::from_str(&json_str)
        .with_context(|| format!("Workflow node: failed to parse '{workflow_path}'"))?;
    serde_json::from_value(unwrap_workflow(workflow_value))
        .with_context(|| format!("Workflow node: invalid graph in '{workflow_path}'"))
}

/// Check, before a job is queued, that a `Workflow` node with these params can
/// run its target: the target must load, must not nest itself, must pass the
/// same graph checks as a job, must match the caller's declared input/output
/// interface, and neither it nor a workflow it nests may have a VideoFrames
/// chain. Nested workflows run on the scalar executor, which cannot stream
/// frames.
pub(crate) fn validate_nested_workflow(
    params: &HashMap<String, serde_json::Value>,
    registry: &NodeRegistry,
) -> Result<()> {
    check_nested_workflow(params, registry, &mut Vec::new())
}

fn check_nested_workflow(
    caller_params: &HashMap<String, serde_json::Value>,
    registry: &NodeRegistry,
    chain: &mut Vec<PathBuf>,
) -> Result<()> {
    let workflow_path = caller_params
        .get("workflow_path")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if workflow_path.is_empty() {
        bail!("Workflow node: workflow_path is empty");
    }
    let path = PathBuf::from(workflow_path);
    if chain.contains(&path) {
        bail!("Workflow node: circular reference detected — '{workflow_path}' nests itself");
    }
    if chain.len() >= MAX_NESTING_DEPTH as usize {
        bail!("Workflow node: maximum nesting depth ({MAX_NESTING_DEPTH}) exceeded at '{workflow_path}'");
    }

    let graph = load_nested_workflow(workflow_path)?;
    if graph.has_video_frames_edges() {
        bail!(
            "Workflow node: '{workflow_path}' has VideoFrames connections; a Workflow node can only \
             run workflows without a video pipeline"
        );
    }
    // Not `validate`: that would restart the nesting chain for each level.
    graph
        .validate_own_nodes(registry)
        .with_context(|| format!("Workflow node: invalid graph in '{workflow_path}'"))?;

    validate_nested_interface(workflow_path, caller_params, &graph)?;

    chain.push(path);
    for node in graph.nodes().filter(|node| node.node_type == "Workflow") {
        check_nested_workflow(&node.params, registry, chain)
            .with_context(|| format!("in node '{}' of '{workflow_path}'", node.id))?;
    }
    chain.pop();
    Ok(())
}

struct InnerInputSpec {
    name: String,
    port_type: PortType,
    needs_caller_value: bool,
}

/// Verify that a `Workflow` node's declared `interface_inputs`,
/// `interface_outputs`, and node-level parameter values match the inner
/// workflow's `WorkflowInput` and `WorkflowOutput` nodes.
fn validate_nested_interface(
    workflow_path: &str,
    caller_params: &HashMap<String, serde_json::Value>,
    graph: &PipelineGraph,
) -> Result<()> {
    let interface_inputs = parse_port_definitions_strict(caller_params, "interface_inputs", true)?;
    let interface_outputs =
        parse_port_definitions_strict(caller_params, "interface_outputs", false)?;

    for caller_in in &interface_inputs {
        if let Some(param_val) = caller_params.get(&caller_in.name) {
            port_data_from_json(&caller_in.port_type, param_val).with_context(|| {
                format!(
                    "Workflow node: invalid param value for interface input '{}'",
                    caller_in.name
                )
            })?;
        }
    }

    let mut inner_inputs: Vec<InnerInputSpec> = Vec::new();
    let mut inner_input_indices: HashMap<String, usize> = HashMap::new();
    let mut inner_outputs: HashMap<String, Vec<(PortType, bool)>> = HashMap::new();

    for idx in graph.execution_order()? {
        let instance = graph.node(idx);
        if instance.node_type == "WorkflowInput" {
            let ports = parse_port_definitions_strict(&instance.params, "ports", true)
                .with_context(|| {
                    format!(
                        "in WorkflowInput node '{}' of '{workflow_path}'",
                        instance.id
                    )
                })?;
            for port in ports {
                // Match `WorkflowInputNode::execute`: caller input overrides
                // node-level `injected`, which overrides `port.default_value`
                // without falling through if `injected` is present.
                let has_inner_fallback = match instance.params.get(&port.name) {
                    Some(injected_val) => {
                        port_data_from_json(&port.port_type, injected_val).is_ok()
                    }
                    None => port.default_value.is_some(),
                };
                let needs_caller_value = !has_inner_fallback;

                if let Some(&existing_idx) = inner_input_indices.get(&port.name) {
                    let existing = &mut inner_inputs[existing_idx];
                    if !existing.port_type.is_compatible(&port.port_type) {
                        bail!(
                            "Workflow node: '{workflow_path}' defines conflicting types ({:?} vs {:?}) \
                             for WorkflowInput port '{}'",
                            existing.port_type,
                            port.port_type,
                            port.name
                        );
                    }
                    existing.needs_caller_value |= needs_caller_value;
                } else {
                    inner_input_indices.insert(port.name.clone(), inner_inputs.len());
                    inner_inputs.push(InnerInputSpec {
                        name: port.name,
                        port_type: port.port_type,
                        needs_caller_value,
                    });
                }
            }
        } else if instance.node_type == "WorkflowOutput" {
            let ports = parse_port_definitions_strict(&instance.params, "ports", false)
                .with_context(|| {
                    format!(
                        "in WorkflowOutput node '{}' of '{workflow_path}'",
                        instance.id
                    )
                })?;
            let connected: HashSet<&str> = graph
                .connections_to(idx)
                .into_iter()
                .map(|(_, conn)| conn.target_port.as_str())
                .collect();

            for port in ports {
                let is_connected = connected.contains(port.name.as_str());
                if !is_connected {
                    if let Some(param_val) = instance.params.get(&port.name) {
                        port_data_from_json(&port.port_type, param_val).with_context(|| {
                            format!(
                                "in WorkflowOutput node '{}' of '{workflow_path}': invalid param for '{}'",
                                instance.id, port.name
                            )
                        })?;
                    }
                }
                let is_fed = is_connected
                    || instance.params.contains_key(&port.name)
                    || port.default_value.is_some();
                inner_outputs
                    .entry(port.name)
                    .or_default()
                    .push((port.port_type, is_fed));
            }
        }
    }

    for caller_in in &interface_inputs {
        let Some(&spec_idx) = inner_input_indices.get(&caller_in.name) else {
            bail!(
                "Workflow node: interface input '{}' does not exist in '{workflow_path}'",
                caller_in.name
            );
        };
        let inner_in = &inner_inputs[spec_idx];
        if !caller_in.port_type.is_compatible(&inner_in.port_type) {
            bail!(
                "Workflow node: interface input '{}' has type {:?}, expected {:?} from '{workflow_path}'",
                caller_in.name,
                caller_in.port_type,
                inner_in.port_type
            );
        }
    }

    for inner_in in &inner_inputs {
        if !inner_in.needs_caller_value {
            continue;
        }
        let Some(caller_in) = interface_inputs.iter().find(|p| p.name == inner_in.name) else {
            bail!(
                "Workflow node: missing required interface input '{}' ({:?}) for '{workflow_path}'",
                inner_in.name,
                inner_in.port_type
            );
        };
        let has_caller_param = caller_params.contains_key(&inner_in.name);
        if !caller_in.required && caller_in.default_value.is_none() && !has_caller_param {
            bail!(
                "Workflow node: interface input '{}' is optional without a default, \
                 but '{workflow_path}' requires it",
                inner_in.name
            );
        }
    }

    for caller_out in &interface_outputs {
        let Some(specs) = inner_outputs.get(&caller_out.name) else {
            bail!(
                "Workflow node: interface output '{}' does not exist in '{workflow_path}'",
                caller_out.name
            );
        };
        if specs.len() > 1 {
            bail!(
                "Workflow node: '{workflow_path}' has more than one WorkflowOutput for '{}'",
                caller_out.name
            );
        }
        let (ref inner_type, is_fed) = specs[0];
        if !inner_type.is_compatible(&caller_out.port_type) {
            bail!(
                "Workflow node: interface output '{}' has type {:?}, expected {:?} from '{workflow_path}'",
                caller_out.name,
                caller_out.port_type,
                inner_type
            );
        }
        if !is_fed {
            bail!(
                "Workflow node: interface output '{}' is not connected in '{workflow_path}'",
                caller_out.name
            );
        }
    }

    Ok(())
}

fn parse_port_type(s: &str) -> Option<PortType> {
    match s {
        "Int" => Some(PortType::Int),
        "Float" => Some(PortType::Float),
        "Str" => Some(PortType::Str),
        "Bool" => Some(PortType::Bool),
        "Path" => Some(PortType::Path),
        "WorkflowPath" => Some(PortType::WorkflowPath),
        _ => None,
    }
}

fn parse_workflow_interface_inputs(
    workflow_path: &str,
    arr: &[serde_json::Value],
) -> Vec<PortDefinition> {
    let needs_inner_lookup = arr.iter().any(|item| {
        matches!(item.get("required"), None | Some(serde_json::Value::Null))
            && item.get("default_value").filter(|v| !v.is_null()).is_none()
    });
    let optional_inner_ports = if needs_inner_lookup {
        inner_ports_with_fallback(workflow_path)
    } else {
        HashSet::new()
    };

    arr.iter()
        .filter_map(|item| {
            let name = item.get("name")?.as_str()?.to_string();
            if name.is_empty() {
                return None;
            }
            let type_str = item.get("port_type")?.as_str()?;
            let port_type = parse_port_type(type_str)?;
            let default_value = item.get("default_value").filter(|v| !v.is_null()).cloned();
            let required = match item.get("required") {
                None | Some(serde_json::Value::Null) => {
                    default_value.is_none() && !optional_inner_ports.contains(&name)
                }
                Some(v) => v.as_bool()?,
            };
            Some(PortDefinition {
                name,
                port_type,
                required,
                default_value,
            })
        })
        .collect()
}

/// Ports of the target workflow's `WorkflowInput` nodes whose every declaration
/// already has a valid inner fallback (`injected` node param or `default_value`),
/// inspected from the immediate file without recursion.
fn inner_ports_with_fallback(workflow_path: &str) -> HashSet<String> {
    if workflow_path.is_empty() {
        return HashSet::new();
    }
    let Ok(graph) = load_nested_workflow(workflow_path) else {
        return HashSet::new();
    };

    let mut fallback_status: HashMap<String, bool> = HashMap::new();
    for instance in graph
        .nodes()
        .filter(|node| node.node_type == "WorkflowInput")
    {
        let Some(ports_arr) = instance.params.get("ports").and_then(|v| v.as_array()) else {
            continue;
        };
        for item in ports_arr {
            let Some(name) = item
                .get("name")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let Some(port_type) = item
                .get("port_type")
                .and_then(|v| v.as_str())
                .and_then(parse_port_type)
            else {
                fallback_status.insert(name.to_string(), false);
                continue;
            };
            let has_fallback = match instance.params.get(name) {
                Some(injected_val) => port_data_from_json(&port_type, injected_val).is_ok(),
                None => item
                    .get("default_value")
                    .filter(|v| !v.is_null())
                    .is_some_and(|dv| port_data_from_json(&port_type, dv).is_ok()),
            };
            fallback_status
                .entry(name.to_string())
                .and_modify(|all_have_fallback| *all_have_fallback &= has_fallback)
                .or_insert(has_fallback);
        }
    }

    fallback_status
        .into_iter()
        .filter_map(|(name, all_have_fallback)| all_have_fallback.then_some(name))
        .collect()
}

fn parse_port_definitions(
    arr: &[serde_json::Value],
    required_by_default: bool,
) -> Vec<PortDefinition> {
    arr.iter()
        .filter_map(|item| {
            let name = item.get("name")?.as_str()?.to_string();
            if name.is_empty() {
                return None;
            }
            let type_str = item.get("port_type")?.as_str()?;
            let port_type = parse_port_type(type_str)?;
            let default_value = item.get("default_value").filter(|v| !v.is_null()).cloned();
            let required = match item.get("required") {
                None | Some(serde_json::Value::Null) => {
                    required_by_default && default_value.is_none()
                }
                Some(v) => v.as_bool()?,
            };
            Some(PortDefinition {
                name,
                port_type,
                required,
                default_value,
            })
        })
        .collect()
}

fn parse_port_definitions_strict(
    params: &HashMap<String, serde_json::Value>,
    key: &str,
    required_by_default: bool,
) -> Result<Vec<PortDefinition>> {
    let Some(raw) = params.get(key) else {
        return Ok(Vec::new());
    };
    if raw.is_null() {
        return Ok(Vec::new());
    }
    let arr = raw
        .as_array()
        .ok_or_else(|| anyhow!("Workflow node: '{key}' must be an array"))?;

    let mut ports = Vec::with_capacity(arr.len());
    let mut seen_names = HashSet::with_capacity(arr.len());

    for (idx, item) in arr.iter().enumerate() {
        let obj = item
            .as_object()
            .ok_or_else(|| anyhow!("Workflow node: '{key}[{idx}]' must be an object"))?;
        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                anyhow!("Workflow node: '{key}[{idx}]' must have a non-empty string 'name'")
            })?
            .to_string();
        if !seen_names.insert(name.clone()) {
            bail!("Workflow node: duplicate port '{name}' in '{key}'");
        }
        let type_str = obj
            .get("port_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                anyhow!("Workflow node: port '{name}' in '{key}' must have a string 'port_type'")
            })?;
        let port_type = parse_port_type(type_str).ok_or_else(|| {
            anyhow!(
                "Workflow node: unsupported port_type '{type_str}' for port '{name}' in '{key}'"
            )
        })?;
        let default_value = obj.get("default_value").filter(|v| !v.is_null()).cloned();
        if let Some(ref dv) = default_value {
            port_data_from_json(&port_type, dv).with_context(|| {
                format!("Workflow node: invalid default_value for port '{name}' in '{key}'")
            })?;
        }
        let required = match obj.get("required") {
            None | Some(serde_json::Value::Null) => required_by_default && default_value.is_none(),
            Some(serde_json::Value::Bool(b)) => *b,
            Some(_) => {
                bail!("Workflow node: 'required' for port '{name}' in '{key}' must be a boolean")
            }
        };
        ports.push(PortDefinition {
            name,
            port_type,
            required,
            default_value,
        });
    }

    Ok(ports)
}

fn clone_port_data(data: &PortData) -> PortData {
    crate::executor::clone_port_data(data)
}

/// Extract the inner workflow object from a preset envelope.
fn unwrap_workflow(value: serde_json::Value) -> serde_json::Value {
    if value.get("nodes").is_some() {
        return value;
    }
    if let Some(inner) = value.get("workflow").cloned() {
        if inner.get("nodes").is_some() {
            return inner;
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_port(
        name: &str,
        port_type: PortType,
        default: Option<serde_json::Value>,
    ) -> PortDefinition {
        PortDefinition {
            name: name.to_string(),
            port_type,
            required: false,
            default_value: default,
        }
    }

    // ── WorkflowInputNode tests ──

    #[test]
    fn test_workflow_input_default_empty_ports() {
        let node = WorkflowInputNode::new();
        assert_eq!(node.node_type(), "WorkflowInput");
        assert!(node.input_ports().is_empty());
        assert!(node.output_ports().is_empty());
    }

    #[test]
    fn test_workflow_input_with_ports() {
        let ports = vec![
            make_port("name", PortType::Str, None),
            make_port("count", PortType::Int, Some(serde_json::json!(10))),
        ];
        let node = WorkflowInputNode::with_ports(ports);
        assert!(node.input_ports().is_empty());
        assert_eq!(node.output_ports().len(), 2);
        assert_eq!(node.output_ports()[0].name, "name");
        assert_eq!(node.output_ports()[1].name, "count");
    }

    #[test]
    fn test_workflow_input_execute_with_injected_params() {
        let ports = vec![
            make_port("greeting", PortType::Str, None),
            make_port("value", PortType::Int, None),
        ];
        let mut node = WorkflowInputNode::with_ports(ports);
        let ctx = ExecutionContext::default();

        let mut inputs = HashMap::new();
        inputs.insert("greeting".to_string(), PortData::Str("hello".to_string()));
        inputs.insert("value".to_string(), PortData::Int(42));

        let outputs = node.execute(&inputs, &ctx).unwrap();
        match outputs.get("greeting") {
            Some(PortData::Str(s)) => assert_eq!(s, "hello"),
            _ => panic!("expected Str"),
        }
        match outputs.get("value") {
            Some(PortData::Int(v)) => assert_eq!(*v, 42),
            _ => panic!("expected Int"),
        }
    }

    #[test]
    fn test_workflow_input_uses_default_when_no_param() {
        let ports = vec![make_port(
            "count",
            PortType::Int,
            Some(serde_json::json!(5)),
        )];
        let mut node = WorkflowInputNode::with_ports(ports);
        let ctx = ExecutionContext::default();

        let outputs = node.execute(&HashMap::new(), &ctx).unwrap();
        match outputs.get("count") {
            Some(PortData::Int(v)) => assert_eq!(*v, 5),
            _ => panic!("expected Int(5)"),
        }
    }

    #[test]
    fn test_workflow_input_error_on_missing_param_no_default() {
        let ports = vec![make_port("required_val", PortType::Str, None)];
        let mut node = WorkflowInputNode::with_ports(ports);
        let ctx = ExecutionContext::default();

        match node.execute(&HashMap::new(), &ctx) {
            Err(e) => assert!(e.to_string().contains("missing value for port")),
            Ok(_) => panic!("should error on missing param"),
        }
    }

    #[test]
    fn test_workflow_input_from_params() {
        let mut params = HashMap::new();
        params.insert(
            "ports".to_string(),
            serde_json::json!([
                {"name": "x", "port_type": "Int", "default_value": 0},
                {"name": "y", "port_type": "Float"},
                {"name": "path", "port_type": "Path"}
            ]),
        );
        let node = WorkflowInputNode::from_params(&params);
        assert_eq!(node.output_ports().len(), 3);
        assert_eq!(node.output_ports()[0].port_type, PortType::Int);
        assert_eq!(node.output_ports()[1].port_type, PortType::Float);
        assert_eq!(node.output_ports()[2].port_type, PortType::Path);
    }

    // ── WorkflowOutputNode tests ──

    #[test]
    fn test_workflow_output_default_empty() {
        let node = WorkflowOutputNode::new();
        assert_eq!(node.node_type(), "WorkflowOutput");
        assert!(node.input_ports().is_empty());
        assert!(node.output_ports().is_empty());
    }

    #[test]
    fn test_workflow_output_collects_inputs() {
        let ports = vec![
            make_port("result", PortType::Int, None),
            make_port("message", PortType::Str, None),
        ];
        let mut node = WorkflowOutputNode::with_ports(ports);
        let ctx = ExecutionContext::default();

        let mut inputs = HashMap::new();
        inputs.insert("result".to_string(), PortData::Int(100));
        inputs.insert("message".to_string(), PortData::Str("done".to_string()));

        let outputs = node.execute(&inputs, &ctx).unwrap();
        match outputs.get("result") {
            Some(PortData::Int(v)) => assert_eq!(*v, 100),
            _ => panic!("expected Int(100)"),
        }
        match outputs.get("message") {
            Some(PortData::Str(s)) => assert_eq!(s, "done"),
            _ => panic!("expected Str"),
        }
    }

    // ── Dynamic port definition tests ──

    #[test]
    fn test_add_remove_ports_at_runtime() {
        let mut node = WorkflowInputNode::new();
        assert!(node.output_ports().is_empty());

        node.ports.push(make_port("a", PortType::Int, None));
        assert_eq!(node.output_ports().len(), 1);

        node.ports.push(make_port("b", PortType::Str, None));
        assert_eq!(node.output_ports().len(), 2);

        node.ports.retain(|p| p.name != "a");
        assert_eq!(node.output_ports().len(), 1);
        assert_eq!(node.output_ports()[0].name, "b");
    }

    // ── WorkflowNode tests ──

    #[test]
    fn test_workflow_node_empty_path_error() {
        let mut node = WorkflowNode::new();
        let ctx = ExecutionContext::default();
        let result = node.execute(&HashMap::new(), &ctx);
        match result {
            Err(e) => assert!(e.to_string().contains("workflow_path is empty")),
            Ok(_) => panic!("should error on empty path"),
        }
    }

    #[test]
    fn test_workflow_node_circular_reference_detection() {
        let path = test_workflow_path();
        let mut node = WorkflowNode {
            workflow_path: path.to_string_lossy().to_string(),
            interface_inputs: vec![],
            interface_outputs: vec![],
        };

        let mut ctx = ExecutionContext::default();
        ctx.executing_workflows.insert(path);

        match node.execute(&HashMap::new(), &ctx) {
            Err(e) => assert!(e.to_string().contains("circular reference")),
            Ok(_) => panic!("should detect circular reference"),
        }
    }

    #[test]
    fn test_workflow_node_file_not_found() {
        let mut node = WorkflowNode {
            workflow_path: "/nonexistent/path/workflow.json".to_string(),
            interface_inputs: vec![],
            interface_outputs: vec![],
        };
        let ctx = ExecutionContext::default();
        let result = node.execute(&HashMap::new(), &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_workflow_node_nested_execution() {
        // Create a simple inner workflow: WorkflowInput(x: Int) → WorkflowOutput(x: Int)
        let inner_workflow = serde_json::json!({
            "nodes": [
                {
                    "id": "wf_in",
                    "node_type": "WorkflowInput",
                    "params": {
                        "ports": [{"name": "x", "port_type": "Int"}]
                    }
                },
                {
                    "id": "wf_out",
                    "node_type": "WorkflowOutput",
                    "params": {
                        "ports": [{"name": "x", "port_type": "Int"}]
                    }
                }
            ],
            "connections": [
                {
                    "from_node": "wf_in",
                    "from_port": "x",
                    "to_node": "wf_out",
                    "to_port": "x",
                    "port_type": "Int"
                }
            ]
        });

        // Write the inner workflow to a temp file
        let dir = std::env::temp_dir().join(format!(
            "videnoa-wf-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let wf_path = dir.join("inner.json");
        std::fs::write(
            &wf_path,
            serde_json::to_string_pretty(&inner_workflow).unwrap(),
        )
        .unwrap();

        let mut node = WorkflowNode {
            workflow_path: wf_path.to_string_lossy().to_string(),
            interface_inputs: vec![make_port("x", PortType::Int, None)],
            interface_outputs: vec![make_port("x", PortType::Int, None)],
        };

        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("x".to_string(), PortData::Int(42));

        let outputs = node.execute(&inputs, &ctx).unwrap();
        match outputs.get("x") {
            Some(PortData::Int(v)) => assert_eq!(*v, 42),
            other => panic!("expected Int(42), got {:?}", other.map(|_| "something")),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nested_results_come_from_the_inner_workflow_output_only() {
        // Given: an inner node whose output port shares the declared output's name.
        let inner_workflow = serde_json::json!({
            "nodes": [
                {"id": "raw", "node_type": "StringTemplate", "params": {
                    "num_input": 1, "template": "raw", "strict": true, "str0": "unused"
                }},
                {"id": "processed", "node_type": "StringTemplate", "params": {
                    "num_input": 1, "template": "processed-{str0}", "strict": true
                }},
                {"id": "wf_out", "node_type": "WorkflowOutput", "params": {
                    "ports": [{"name": "value", "port_type": "Str"}]
                }}
            ],
            "connections": [
                {"from_node": "raw", "from_port": "value", "to_node": "processed", "to_port": "str0", "port_type": "Str"},
                {"from_node": "processed", "from_port": "value", "to_node": "wf_out", "to_port": "value", "port_type": "Str"}
            ]
        });
        let dir = std::env::temp_dir().join(format!(
            "videnoa-wf-output-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let wf_path = dir.join("inner.json");
        std::fs::write(&wf_path, inner_workflow.to_string()).unwrap();

        // When / Then: every run returns the WorkflowOutput's value. Inner
        // outputs are kept in a HashMap, so one run could pass by luck.
        for _ in 0..50 {
            let mut node = WorkflowNode {
                workflow_path: wf_path.to_string_lossy().to_string(),
                interface_inputs: vec![],
                interface_outputs: vec![make_port("value", PortType::Str, None)],
            };
            let outputs = node.execute(&HashMap::new(), &ExecutionContext::default());
            match outputs.unwrap().get("value") {
                Some(PortData::Str(value)) => assert_eq!(value, "processed-raw"),
                _ => panic!("expected the nested workflow's Str output"),
            }
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Type coercion tests ──

    #[test]
    fn test_string_value_as_int_via_default() {
        let ports = vec![make_port(
            "count",
            PortType::Int,
            Some(serde_json::json!(42)),
        )];
        let mut node = WorkflowInputNode::with_ports(ports);
        let ctx = ExecutionContext::default();

        let outputs = node.execute(&HashMap::new(), &ctx).unwrap();
        match outputs.get("count") {
            Some(PortData::Int(v)) => assert_eq!(*v, 42),
            _ => panic!("expected Int"),
        }
    }

    // ── Port type parsing ──

    #[test]
    fn test_parse_port_type_valid() {
        assert_eq!(parse_port_type("Int"), Some(PortType::Int));
        assert_eq!(parse_port_type("Float"), Some(PortType::Float));
        assert_eq!(parse_port_type("Str"), Some(PortType::Str));
        assert_eq!(parse_port_type("Bool"), Some(PortType::Bool));
        assert_eq!(parse_port_type("Path"), Some(PortType::Path));
    }

    #[test]
    fn test_parse_port_type_rejects_video_frames() {
        assert_eq!(parse_port_type("VideoFrames"), None);
        assert_eq!(parse_port_type("Metadata"), None);
    }

    // ── Bool port support ──

    #[test]
    fn test_workflow_input_bool_port() {
        let ports = vec![make_port(
            "flag",
            PortType::Bool,
            Some(serde_json::json!(true)),
        )];
        let mut node = WorkflowInputNode::with_ports(ports);
        let ctx = ExecutionContext::default();

        let outputs = node.execute(&HashMap::new(), &ctx).unwrap();
        match outputs.get("flag") {
            Some(PortData::Bool(v)) => assert!(*v),
            _ => panic!("expected Bool(true)"),
        }
    }

    // ── Path port support ──

    #[test]
    fn test_workflow_input_path_port() {
        let path = test_txt_path();
        let ports = vec![make_port(
            "file",
            PortType::Path,
            Some(serde_json::json!(path.to_string_lossy())),
        )];
        let mut node = WorkflowInputNode::with_ports(ports);
        let ctx = ExecutionContext::default();

        let outputs = node.execute(&HashMap::new(), &ctx).unwrap();
        match outputs.get("file") {
            Some(PortData::Path(p)) => assert_eq!(p, &path),
            _ => panic!("expected Path"),
        }
    }

    // ── Nesting depth & multi-level cycle tests ──

    fn make_test_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "videnoa-wf-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn test_workflow_path() -> PathBuf {
        std::env::temp_dir().join("test_workflow.json")
    }

    fn test_txt_path() -> PathBuf {
        std::env::temp_dir().join("test.txt")
    }

    fn test_nested_workflow_path() -> PathBuf {
        std::env::temp_dir().join("some_workflow.json")
    }

    fn simple_passthrough_workflow_json() -> serde_json::Value {
        serde_json::json!({
            "nodes": [
                {
                    "id": "wf_in",
                    "node_type": "WorkflowInput",
                    "params": {
                        "ports": [{"name": "x", "port_type": "Int"}]
                    }
                },
                {
                    "id": "wf_out",
                    "node_type": "WorkflowOutput",
                    "params": {
                        "ports": [{"name": "x", "port_type": "Int"}]
                    }
                }
            ],
            "connections": [
                {
                    "from_node": "wf_in",
                    "from_port": "x",
                    "to_node": "wf_out",
                    "to_port": "x",
                    "port_type": "Int"
                }
            ]
        })
    }

    #[test]
    fn test_workflow_node_multi_level_cycle_detection() {
        let dir = make_test_dir();
        let path_a = dir.join("workflow_a.json");
        let path_b = dir.join("workflow_b.json");

        let workflow_a = serde_json::json!({
            "nodes": [
                {
                    "id": "wf_in",
                    "node_type": "WorkflowInput",
                    "params": { "ports": [{"name": "x", "port_type": "Int"}] }
                },
                {
                    "id": "nested_b",
                    "node_type": "Workflow",
                    "params": {
                        "workflow_path": path_b.to_string_lossy(),
                        "interface_inputs": [{"name": "x", "port_type": "Int"}],
                        "interface_outputs": [{"name": "x", "port_type": "Int"}]
                    }
                },
                {
                    "id": "wf_out",
                    "node_type": "WorkflowOutput",
                    "params": { "ports": [{"name": "x", "port_type": "Int"}] }
                }
            ],
            "connections": [
                { "from_node": "wf_in", "from_port": "x", "to_node": "nested_b", "to_port": "x", "port_type": "Int" },
                { "from_node": "nested_b", "from_port": "x", "to_node": "wf_out", "to_port": "x", "port_type": "Int" }
            ]
        });

        let workflow_b = serde_json::json!({
            "nodes": [
                {
                    "id": "wf_in",
                    "node_type": "WorkflowInput",
                    "params": { "ports": [{"name": "x", "port_type": "Int"}] }
                },
                {
                    "id": "nested_a",
                    "node_type": "Workflow",
                    "params": {
                        "workflow_path": path_a.to_string_lossy(),
                        "interface_inputs": [{"name": "x", "port_type": "Int"}],
                        "interface_outputs": [{"name": "x", "port_type": "Int"}]
                    }
                },
                {
                    "id": "wf_out",
                    "node_type": "WorkflowOutput",
                    "params": { "ports": [{"name": "x", "port_type": "Int"}] }
                }
            ],
            "connections": [
                { "from_node": "wf_in", "from_port": "x", "to_node": "nested_a", "to_port": "x", "port_type": "Int" },
                { "from_node": "nested_a", "from_port": "x", "to_node": "wf_out", "to_port": "x", "port_type": "Int" }
            ]
        });

        std::fs::write(&path_a, serde_json::to_string_pretty(&workflow_a).unwrap()).unwrap();
        std::fs::write(&path_b, serde_json::to_string_pretty(&workflow_b).unwrap()).unwrap();

        let mut node = WorkflowNode {
            workflow_path: path_a.to_string_lossy().to_string(),
            interface_inputs: vec![make_port("x", PortType::Int, None)],
            interface_outputs: vec![make_port("x", PortType::Int, None)],
        };

        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("x".to_string(), PortData::Int(1));

        match node.execute(&inputs, &ctx) {
            Err(e) => {
                let full_err = format!("{:#}", e);
                assert!(
                    full_err.contains("circular reference"),
                    "expected circular reference error, got: {full_err}"
                );
            }
            Ok(_) => panic!("should detect circular reference in A→B→A cycle"),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_workflow_node_max_nesting_depth_exceeded() {
        let workflow_path = test_nested_workflow_path();
        let mut node = WorkflowNode {
            workflow_path: workflow_path.to_string_lossy().to_string(),
            interface_inputs: vec![],
            interface_outputs: vec![],
        };

        let ctx = ExecutionContext {
            nesting_depth: 10,
            ..ExecutionContext::default()
        };

        match node.execute(&HashMap::new(), &ctx) {
            Err(e) => assert!(
                e.to_string().contains("maximum nesting depth"),
                "expected max depth error, got: {}",
                e
            ),
            Ok(_) => panic!("should reject execution at max nesting depth"),
        }
    }

    #[test]
    fn test_workflow_node_depth_just_below_limit_succeeds() {
        let dir = make_test_dir();
        let wf_path = dir.join("passthrough.json");
        std::fs::write(
            &wf_path,
            serde_json::to_string_pretty(&simple_passthrough_workflow_json()).unwrap(),
        )
        .unwrap();

        let mut node = WorkflowNode {
            workflow_path: wf_path.to_string_lossy().to_string(),
            interface_inputs: vec![make_port("x", PortType::Int, None)],
            interface_outputs: vec![make_port("x", PortType::Int, None)],
        };

        let ctx = ExecutionContext {
            nesting_depth: 9,
            ..ExecutionContext::default()
        };

        let mut inputs = HashMap::new();
        inputs.insert("x".to_string(), PortData::Int(99));

        let outputs = node.execute(&inputs, &ctx).unwrap();
        match outputs.get("x") {
            Some(PortData::Int(v)) => assert_eq!(*v, 99),
            other => panic!("expected Int(99), got {:?}", other.map(|_| "something")),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_workflow_node_inner_error_propagates() {
        let dir = make_test_dir();
        let wf_path = dir.join("failing_inner.json");

        let failing_workflow = serde_json::json!({
            "nodes": [
                {
                    "id": "wf_in",
                    "node_type": "WorkflowInput",
                    "params": {
                        "ports": [{"name": "required_val", "port_type": "Str"}]
                    }
                },
                {
                    "id": "wf_out",
                    "node_type": "WorkflowOutput",
                    "params": {
                        "ports": [{"name": "required_val", "port_type": "Str"}]
                    }
                }
            ],
            "connections": [
                {
                    "from_node": "wf_in",
                    "from_port": "required_val",
                    "to_node": "wf_out",
                    "to_port": "required_val",
                    "port_type": "Str"
                }
            ]
        });

        std::fs::write(
            &wf_path,
            serde_json::to_string_pretty(&failing_workflow).unwrap(),
        )
        .unwrap();

        let mut node = WorkflowNode {
            workflow_path: wf_path.to_string_lossy().to_string(),
            interface_inputs: vec![],
            interface_outputs: vec![make_port("required_val", PortType::Str, None)],
        };

        let ctx = ExecutionContext::default();
        match node.execute(&HashMap::new(), &ctx) {
            Err(e) => {
                let full_err = format!("{:#}", e);
                assert!(
                    full_err.contains("missing value for port"),
                    "expected inner workflow error to propagate, got: {full_err}"
                );
            }
            Ok(_) => panic!("should propagate inner workflow error"),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_workflow_node_nested_depth_increments() {
        let dir = make_test_dir();
        let inner_path = dir.join("inner_depth.json");
        let outer_path = dir.join("outer_depth.json");

        std::fs::write(
            &inner_path,
            serde_json::to_string_pretty(&simple_passthrough_workflow_json()).unwrap(),
        )
        .unwrap();

        let outer_workflow = serde_json::json!({
            "nodes": [
                {
                    "id": "wf_in",
                    "node_type": "WorkflowInput",
                    "params": { "ports": [{"name": "x", "port_type": "Int"}] }
                },
                {
                    "id": "nested",
                    "node_type": "Workflow",
                    "params": {
                        "workflow_path": inner_path.to_string_lossy(),
                        "interface_inputs": [{"name": "x", "port_type": "Int"}],
                        "interface_outputs": [{"name": "x", "port_type": "Int"}]
                    }
                },
                {
                    "id": "wf_out",
                    "node_type": "WorkflowOutput",
                    "params": { "ports": [{"name": "x", "port_type": "Int"}] }
                }
            ],
            "connections": [
                { "from_node": "wf_in", "from_port": "x", "to_node": "nested", "to_port": "x", "port_type": "Int" },
                { "from_node": "nested", "from_port": "x", "to_node": "wf_out", "to_port": "x", "port_type": "Int" }
            ]
        });

        std::fs::write(
            &outer_path,
            serde_json::to_string_pretty(&outer_workflow).unwrap(),
        )
        .unwrap();

        let mut node = WorkflowNode {
            workflow_path: outer_path.to_string_lossy().to_string(),
            interface_inputs: vec![make_port("x", PortType::Int, None)],
            interface_outputs: vec![make_port("x", PortType::Int, None)],
        };

        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("x".to_string(), PortData::Int(7));

        let outputs = node.execute(&inputs, &ctx).unwrap();
        match outputs.get("x") {
            Some(PortData::Int(v)) => assert_eq!(*v, 7),
            other => panic!("expected Int(7), got {:?}", other.map(|_| "something")),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_nested_workflow_validates_the_inner_graph() {
        let dir = tempfile::tempdir().unwrap();
        let inner = dir.path().join("inner.json");
        std::fs::write(
            &inner,
            serde_json::json!({
                "nodes": [{"id": "bad", "node_type": "MissingNodeType", "params": {}}],
                "connections": []
            })
            .to_string(),
        )
        .unwrap();
        // A valid wrapper around it: the error must come from two levels down.
        let middle = dir.path().join("middle.json");
        std::fs::write(
            &middle,
            serde_json::json!({
                "nodes": [{"id": "deeper", "node_type": "Workflow", "params": {"workflow_path": inner}}],
                "connections": []
            })
            .to_string(),
        )
        .unwrap();
        let registry = crate::registry::build_default_registry();

        for target in [&inner, &middle] {
            let outer: crate::graph::PipelineGraph = serde_json::from_value(serde_json::json!({
                "nodes": [{"id": "nested", "node_type": "Workflow", "params": {"workflow_path": target}}],
                "connections": []
            }))
            .unwrap();
            let error = outer.validate(&registry).unwrap_err();
            assert!(
                format!("{error:#}").contains("MissingNodeType"),
                "{error:#}"
            );
        }
    }

    #[test]
    fn validate_nested_workflow_rejects_targets_that_cannot_run() {
        let dir = tempfile::tempdir().unwrap();
        let self_path = dir.path().join("self.json");
        let self_nesting = serde_json::json!({
            "nodes": [{"id": "again", "node_type": "Workflow", "params": {"workflow_path": self_path}}],
            "connections": []
        });
        std::fs::write(&self_path, self_nesting.to_string()).unwrap();
        let params = |path: &std::path::Path| {
            HashMap::from([("workflow_path".to_string(), serde_json::json!(path))])
        };

        let registry = crate::registry::build_default_registry();
        let missing =
            validate_nested_workflow(&params(&dir.path().join("missing.json")), &registry)
                .expect_err("an unreadable target must be rejected");
        let circular = validate_nested_workflow(&params(&self_path), &registry)
            .expect_err("a target that nests itself must be rejected");
        let empty = validate_nested_workflow(&HashMap::new(), &registry)
            .expect_err("a Workflow node without a target must be rejected");

        assert!(
            format!("{missing:#}").contains("failed to read"),
            "{missing:#}"
        );
        assert!(
            format!("{circular:#}").contains("circular reference"),
            "{circular:#}"
        );
        assert!(format!("{empty:#}").contains("workflow_path is empty"));
    }

    #[test]
    fn validate_nested_workflow_rejects_unconnected_or_duplicate_inner_outputs() {
        let dir = tempfile::tempdir().unwrap();
        let registry = crate::registry::build_default_registry();

        // Inner WorkflowOutput declares 'result' without any incoming connection or default.
        let unconnected = dir.path().join("unconnected_out.json");
        std::fs::write(
            &unconnected,
            serde_json::json!({
                "nodes": [
                    {"id": "wo", "node_type": "WorkflowOutput", "params": {
                        "ports": [{"name": "result", "port_type": "Str"}]
                    }}
                ],
                "connections": []
            })
            .to_string(),
        )
        .unwrap();
        let outer_unconnected: PipelineGraph = serde_json::from_value(serde_json::json!({
            "nodes": [{
                "id": "nested",
                "node_type": "Workflow",
                "params": {
                    "workflow_path": unconnected,
                    "interface_outputs": [{"name": "result", "port_type": "Str"}]
                }
            }],
            "connections": []
        }))
        .unwrap();
        let err = outer_unconnected.validate(&registry).unwrap_err();
        assert!(
            format!("{err:#}").contains("interface output 'result' is not connected"),
            "{err:#}"
        );

        // Two inner WorkflowOutput nodes both define 'result'.
        let duplicate = dir.path().join("duplicate_out.json");
        std::fs::write(
            &duplicate,
            serde_json::json!({
                "nodes": [
                    {"id": "c", "node_type": "Constant", "params": {"type": "Str", "value": "v"}},
                    {"id": "wo1", "node_type": "WorkflowOutput", "params": {
                        "ports": [{"name": "result", "port_type": "Str"}]
                    }},
                    {"id": "wo2", "node_type": "WorkflowOutput", "params": {
                        "ports": [{"name": "result", "port_type": "Str"}]
                    }}
                ],
                "connections": [
                    {"from_node": "c", "from_port": "value", "to_node": "wo1", "to_port": "result", "port_type": "Str"},
                    {"from_node": "c", "from_port": "value", "to_node": "wo2", "to_port": "result", "port_type": "Str"}
                ]
            })
            .to_string(),
        )
        .unwrap();
        let outer_duplicate: PipelineGraph = serde_json::from_value(serde_json::json!({
            "nodes": [{
                "id": "nested",
                "node_type": "Workflow",
                "params": {
                    "workflow_path": duplicate,
                    "interface_outputs": [{"name": "result", "port_type": "Str"}]
                }
            }],
            "connections": []
        }))
        .unwrap();
        let err = outer_duplicate.validate(&registry).unwrap_err();
        assert!(
            format!("{err:#}").contains("more than one WorkflowOutput for 'result'"),
            "{err:#}"
        );
    }

    #[test]
    fn validate_nested_workflow_rejects_conflicting_inner_inputs_and_malformed_interface() {
        let dir = tempfile::tempdir().unwrap();
        let registry = crate::registry::build_default_registry();

        let conflicting = dir.path().join("conflicting_in.json");
        std::fs::write(
            &conflicting,
            serde_json::json!({
                "nodes": [
                    {"id": "wi1", "node_type": "WorkflowInput", "params": {
                        "ports": [{"name": "x", "port_type": "Int", "default_value": 1}]
                    }},
                    {"id": "wi2", "node_type": "WorkflowInput", "params": {
                        "ports": [{"name": "x", "port_type": "Str", "default_value": "a"}]
                    }}
                ],
                "connections": []
            })
            .to_string(),
        )
        .unwrap();
        let outer_conflict: PipelineGraph = serde_json::from_value(serde_json::json!({
            "nodes": [{
                "id": "nested",
                "node_type": "Workflow",
                "params": {"workflow_path": conflicting}
            }],
            "connections": []
        }))
        .unwrap();
        let err = outer_conflict.validate(&registry).unwrap_err();
        assert!(format!("{err:#}").contains("conflicting types"), "{err:#}");

        let empty_inner = dir.path().join("empty_inner.json");
        std::fs::write(
            &empty_inner,
            serde_json::json!({"nodes": [], "connections": []}).to_string(),
        )
        .unwrap();
        let bad_type: PipelineGraph = serde_json::from_value(serde_json::json!({
            "nodes": [{
                "id": "nested",
                "node_type": "Workflow",
                "params": {
                    "workflow_path": empty_inner,
                    "interface_outputs": [{"name": "frames", "port_type": "VideoFrames"}]
                }
            }],
            "connections": []
        }))
        .unwrap();
        let err = bad_type.validate(&registry).unwrap_err();
        assert!(
            format!("{err:#}").contains("unsupported port_type 'VideoFrames'"),
            "{err:#}"
        );
    }
}
