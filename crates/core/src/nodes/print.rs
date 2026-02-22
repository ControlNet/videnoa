use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};

use crate::node::{ExecutionContext, Node, PortDefinition};
use crate::types::{PortData, PortType};

pub struct PrintNode {
    value_type: PortType,
}

impl PrintNode {
    pub fn new() -> Self {
        Self {
            value_type: PortType::Str,
        }
    }

    fn parse_and_apply_value_type_from_inputs(
        &mut self,
        inputs: &HashMap<String, PortData>,
    ) -> Result<()> {
        let value_type = parse_input_value_type(inputs)?.unwrap_or(self.value_type.clone());
        self.value_type = value_type;
        Ok(())
    }
}

impl Default for PrintNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for PrintNode {
    fn node_type(&self) -> &str {
        "Print"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![
            PortDefinition {
                name: "value_type".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!(port_type_name(&self.value_type))),
            },
            PortDefinition {
                name: "value".to_string(),
                port_type: self.value_type.clone(),
                required: true,
                default_value: None,
            },
        ]
    }

    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![PortDefinition {
            name: "value".to_string(),
            port_type: self.value_type.clone(),
            required: true,
            default_value: None,
        }]
    }

    fn execute(
        &mut self,
        inputs: &HashMap<String, PortData>,
        _ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        self.parse_and_apply_value_type_from_inputs(inputs)?;

        let value = inputs
            .get("value")
            .ok_or_else(|| anyhow!("Print: input 'value' is required"))?;

        if !is_value_type_match(value, &self.value_type) {
            bail!(
                "Print: input 'value' expected {}, got {}",
                port_type_name(&self.value_type),
                port_data_kind(value)
            );
        }

        Ok(HashMap::from([(
            "value".to_string(),
            clone_port_data(value),
        )]))
    }
}

fn parse_input_value_type(inputs: &HashMap<String, PortData>) -> Result<Option<PortType>> {
    let Some(raw_value) = inputs.get("value_type") else {
        return Ok(None);
    };

    let raw = match raw_value {
        PortData::Str(raw) => raw.as_str(),
        other => {
            bail!(
                "Print: input 'value_type' must be Str, got {}",
                port_data_kind(other)
            )
        }
    };

    Ok(Some(parse_supported_type(raw)?))
}

fn parse_supported_type(raw: &str) -> Result<PortType> {
    match raw {
        "Int" => Ok(PortType::Int),
        "Float" => Ok(PortType::Float),
        "Str" => Ok(PortType::Str),
        "Bool" => Ok(PortType::Bool),
        "Path" => Ok(PortType::Path),
        other => bail!(
            "Print: unsupported value_type '{other}', expected one of Int|Float|Str|Bool|Path"
        ),
    }
}

fn is_value_type_match(value: &PortData, value_type: &PortType) -> bool {
    match (value_type, value) {
        (PortType::Int, PortData::Int(_)) => true,
        (PortType::Float, PortData::Float(_)) => true,
        (PortType::Str, PortData::Str(_)) => true,
        (PortType::Bool, PortData::Bool(_)) => true,
        (PortType::Path, PortData::Path(_)) => true,
        _ => false,
    }
}

fn clone_port_data(value: &PortData) -> PortData {
    match value {
        PortData::Int(v) => PortData::Int(*v),
        PortData::Float(v) => PortData::Float(*v),
        PortData::Str(v) => PortData::Str(v.clone()),
        PortData::Bool(v) => PortData::Bool(*v),
        PortData::Path(v) => PortData::Path(v.clone()),
        PortData::Metadata(_) => unreachable!("metadata is not supported by Print value_type"),
    }
}

fn port_type_name(port_type: &PortType) -> &'static str {
    match port_type {
        PortType::Int => "Int",
        PortType::Float => "Float",
        PortType::Str => "Str",
        PortType::Bool => "Bool",
        PortType::Path => "Path",
        _ => "Unsupported",
    }
}

fn port_data_kind(data: &PortData) -> &'static str {
    match data {
        PortData::Int(_) => "Int",
        PortData::Float(_) => "Float",
        PortData::Str(_) => "Str",
        PortData::Bool(_) => "Bool",
        PortData::Path(_) => "Path",
        PortData::Metadata(_) => "Metadata",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_print_node_contract_and_default_ports() {
        let node = PrintNode::new();

        assert_eq!(node.node_type(), "Print");

        let input_ports = node.input_ports();
        assert_eq!(input_ports.len(), 2);
        assert_eq!(input_ports[0].name, "value_type");
        assert_eq!(input_ports[0].port_type, PortType::Str);
        assert_eq!(input_ports[0].default_value, Some(serde_json::json!("Str")));
        assert_eq!(input_ports[1].name, "value");
        assert_eq!(input_ports[1].port_type, PortType::Str);

        let output_ports = node.output_ports();
        assert_eq!(output_ports.len(), 1);
        assert_eq!(output_ports[0].name, "value");
        assert_eq!(output_ports[0].port_type, PortType::Str);
    }

    #[test]
    fn test_execute_pass_through_int() {
        let mut node = PrintNode::new();
        let ctx = ExecutionContext::default();

        let inputs = HashMap::from([
            ("value_type".to_string(), PortData::Str("Int".to_string())),
            ("value".to_string(), PortData::Int(42)),
        ]);

        let outputs = node
            .execute(&inputs, &ctx)
            .expect("Print Int pass-through should succeed");

        match outputs.get("value") {
            Some(PortData::Int(v)) => assert_eq!(*v, 42),
            _ => panic!("expected Int output"),
        }
    }

    #[test]
    fn test_execute_pass_through_str() {
        let mut node = PrintNode::new();
        let ctx = ExecutionContext::default();

        let inputs = HashMap::from([
            ("value_type".to_string(), PortData::Str("Str".to_string())),
            (
                "value".to_string(),
                PortData::Str("hello print".to_string()),
            ),
        ]);

        let outputs = node
            .execute(&inputs, &ctx)
            .expect("Print Str pass-through should succeed");

        match outputs.get("value") {
            Some(PortData::Str(v)) => assert_eq!(v, "hello print"),
            _ => panic!("expected Str output"),
        }
    }

    #[test]
    fn test_execute_pass_through_path() {
        let mut node = PrintNode::new();
        let ctx = ExecutionContext::default();

        let path = PathBuf::from("/tmp/episode01.mkv");
        let inputs = HashMap::from([
            ("value_type".to_string(), PortData::Str("Path".to_string())),
            ("value".to_string(), PortData::Path(path.clone())),
        ]);

        let outputs = node
            .execute(&inputs, &ctx)
            .expect("Print Path pass-through should succeed");

        match outputs.get("value") {
            Some(PortData::Path(v)) => assert_eq!(v, &path),
            _ => panic!("expected Path output"),
        }
    }

    #[test]
    fn test_invalid_value_type_rejected() {
        let mut node = PrintNode::new();
        let ctx = ExecutionContext::default();

        let inputs = HashMap::from([
            (
                "value_type".to_string(),
                PortData::Str("VideoFrames".to_string()),
            ),
            ("value".to_string(), PortData::Str("x".to_string())),
        ]);

        let err = match node.execute(&inputs, &ctx) {
            Ok(_) => panic!("unsupported value_type must fail"),
            Err(err) => err,
        };
        assert_eq!(
            err.to_string(),
            "Print: unsupported value_type 'VideoFrames', expected one of Int|Float|Str|Bool|Path"
        );
    }

    #[test]
    fn test_value_type_and_value_mismatch_rejected() {
        let mut node = PrintNode::new();
        let ctx = ExecutionContext::default();

        let inputs = HashMap::from([
            ("value_type".to_string(), PortData::Str("Int".to_string())),
            ("value".to_string(), PortData::Str("42".to_string())),
        ]);

        let err = match node.execute(&inputs, &ctx) {
            Ok(_) => panic!("value mismatch must fail"),
            Err(err) => err,
        };

        assert_eq!(
            err.to_string(),
            "Print: input 'value' expected Int, got Str"
        );
    }
}
