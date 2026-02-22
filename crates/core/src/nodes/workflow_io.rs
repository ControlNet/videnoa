//! Workflow I/O nodes: WorkflowInput, WorkflowOutput, and Workflow (nested execution).
//!
//! These nodes enable "workflow-as-function" — making workflows reusable and parameterizable.
//! WorkflowInput outputs injected params, WorkflowOutput collects results, and Workflow
//! loads and executes a nested workflow with circular reference detection.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::executor::{port_data_from_json, SequentialExecutor};
use crate::graph::PipelineGraph;
use crate::node::{ExecutionContext, Node, PortDefinition};
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
            .map(|arr| parse_port_definitions(arr))
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
            .map(|arr| parse_port_definitions(arr))
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
            .map(|arr| parse_port_definitions(arr))
            .unwrap_or_default();

        let interface_outputs = params
            .get("interface_outputs")
            .and_then(|v| v.as_array())
            .map(|arr| parse_port_definitions(arr))
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
        const MAX_NESTING_DEPTH: u32 = 10;

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

        // Load and parse the workflow
        let json_str = std::fs::read_to_string(&path)
            .with_context(|| format!("Workflow node: failed to read '{}'", self.workflow_path))?;
        let workflow_value: serde_json::Value = serde_json::from_str(&json_str)
            .with_context(|| format!("Workflow node: failed to parse '{}'", self.workflow_path))?;

        // Support preset envelope
        let workflow_value = unwrap_workflow(workflow_value);

        let graph: PipelineGraph = serde_json::from_value(workflow_value)
            .with_context(|| format!("Workflow node: invalid graph in '{}'", self.workflow_path))?;

        // Build inner registry (same as outer — reuse the standard set)
        let registry = crate::registry::build_default_registry();

        let mut inner_ctx = ExecutionContext::default();
        inner_ctx.executing_workflows = ctx.executing_workflows.clone();
        inner_ctx.executing_workflows.insert(path);
        inner_ctx.nesting_depth = ctx.nesting_depth + 1;

        // Inject our inputs as params for the inner WorkflowInput node
        let mut inner_params = HashMap::new();
        for (key, value) in inputs {
            inner_params.insert(key.clone(), clone_port_data(value));
        }

        let outputs =
            SequentialExecutor::execute_with_params(&graph, &registry, inner_params, &inner_ctx)?;

        // Collect results from the inner WorkflowOutput node
        let mut results = HashMap::new();
        for (_node_id, node_outputs) in &outputs {
            for (port_name, port_data) in node_outputs {
                if self.interface_outputs.iter().any(|p| p.name == *port_name) {
                    results.insert(port_name.clone(), clone_port_data(port_data));
                }
            }
        }

        Ok(results)
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

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

fn parse_port_definitions(arr: &[serde_json::Value]) -> Vec<PortDefinition> {
    arr.iter()
        .filter_map(|item| {
            let name = item.get("name")?.as_str()?.to_string();
            let type_str = item.get("port_type")?.as_str()?;
            let port_type = parse_port_type(type_str)?;
            let default_value = item.get("default_value").cloned();
            Some(PortDefinition {
                name,
                port_type,
                required: false,
                default_value,
            })
        })
        .collect()
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

        let mut ctx = ExecutionContext::default();
        ctx.nesting_depth = 10;

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

        let mut ctx = ExecutionContext::default();
        ctx.nesting_depth = 9;

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
}
