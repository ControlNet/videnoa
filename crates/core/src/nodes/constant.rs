//! Constant node: outputs a user-configured constant value with a dynamic output port type.
//!
//! The output port type changes based on the `type` parameter (Int, Float, Str, Bool, Path).
//! The value is always stored as a string and parsed at execute time.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};

use crate::node::{ExecutionContext, Node, PortDefinition};
use crate::types::{PortData, PortType};

pub struct ConstantNode {
    output_type: PortType,
}

impl ConstantNode {
    pub fn new() -> Self {
        Self {
            output_type: PortType::Int,
        }
    }

    pub fn from_params(params: &HashMap<String, serde_json::Value>) -> Result<Self> {
        let output_type = parse_param_type(params)?.unwrap_or(PortType::Int);
        Ok(Self { output_type })
    }
}

impl Default for ConstantNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for ConstantNode {
    fn node_type(&self) -> &str {
        "Constant"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![
            PortDefinition {
                name: "type".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!(port_type_name(&self.output_type))),
            },
            PortDefinition {
                name: "value".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("0")),
            },
        ]
    }

    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![PortDefinition {
            name: "value".to_string(),
            port_type: self.output_type.clone(),
            required: true,
            default_value: None,
        }]
    }

    fn execute(
        &mut self,
        inputs: &HashMap<String, PortData>,
        _ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        self.output_type = match inputs.get("type") {
            Some(PortData::Str(raw)) => parse_supported_type(raw)?,
            Some(other) => {
                bail!(
                    "Constant: input 'type' must be Str, got {}",
                    port_data_kind(other)
                )
            }
            None => self.output_type.clone(),
        };

        let value_str = match inputs.get("value") {
            Some(PortData::Str(s)) => s.clone(),
            _ => "0".to_string(),
        };

        let port_data = match self.output_type {
            PortType::Int => {
                let v: i64 = value_str.parse().map_err(|e| {
                    anyhow::anyhow!("failed to parse '{}' as Int: {}", value_str, e)
                })?;
                PortData::Int(v)
            }
            PortType::Float => {
                let v: f64 = value_str.parse().map_err(|e| {
                    anyhow::anyhow!("failed to parse '{}' as Float: {}", value_str, e)
                })?;
                PortData::Float(v)
            }
            PortType::Str => PortData::Str(value_str),
            PortType::Bool => PortData::Bool(value_str == "true"),
            PortType::Path => PortData::Path(PathBuf::from(value_str)),
            _ => bail!("unsupported constant output type: {:?}", self.output_type),
        };

        let mut outputs = HashMap::new();
        outputs.insert("value".to_string(), port_data);
        Ok(outputs)
    }
}

fn parse_param_type(params: &HashMap<String, serde_json::Value>) -> Result<Option<PortType>> {
    let Some(value) = params.get("type") else {
        return Ok(None);
    };

    let raw = value.as_str().ok_or_else(|| {
        anyhow!("Constant: param 'type' must be a string type name (Int|Float|Str|Bool|Path)")
    })?;

    Ok(Some(parse_supported_type(raw)?))
}

fn parse_supported_type(raw: &str) -> Result<PortType> {
    match raw {
        "Int" => Ok(PortType::Int),
        "Float" => Ok(PortType::Float),
        "Str" => Ok(PortType::Str),
        "Bool" => Ok(PortType::Bool),
        "Path" => Ok(PortType::Path),
        other => {
            bail!("Constant: unsupported type '{other}', expected one of Int|Float|Str|Bool|Path")
        }
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
    use super::*;

    #[test]
    fn test_constant_default() {
        let node = ConstantNode::new();
        assert_eq!(node.output_type, PortType::Int);
        assert_eq!(node.node_type(), "Constant");

        let inputs = node.input_ports();
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].name, "type");
        assert_eq!(inputs[1].name, "value");

        let outputs = node.output_ports();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].name, "value");
        assert_eq!(outputs[0].port_type, PortType::Int);
    }

    #[test]
    fn test_constant_from_params_sets_dynamic_output_type_before_execute() {
        let params = HashMap::from([("type".to_string(), serde_json::json!("Str"))]);
        let node = ConstantNode::from_params(&params).expect("params should initialize Constant");

        assert_eq!(node.output_ports()[0].port_type, PortType::Str);
        assert_eq!(
            node.input_ports()[0].default_value,
            Some(serde_json::json!("Str"))
        );
    }

    #[test]
    fn test_constant_from_params_rejects_invalid_type_value() {
        let params = HashMap::from([("type".to_string(), serde_json::json!("VideoFrames"))]);
        let err = match ConstantNode::from_params(&params) {
            Ok(_) => panic!("unsupported params.type should fail deterministically"),
            Err(err) => err,
        };

        assert_eq!(
            err.to_string(),
            "Constant: unsupported type 'VideoFrames', expected one of Int|Float|Str|Bool|Path"
        );
    }

    #[test]
    fn test_constant_int_output() {
        let mut node = ConstantNode::new();
        let ctx = ExecutionContext::default();

        let mut inputs = HashMap::new();
        inputs.insert("type".to_string(), PortData::Str("Int".to_string()));
        inputs.insert("value".to_string(), PortData::Str("42".to_string()));

        let outputs = node.execute(&inputs, &ctx).unwrap();
        match outputs.get("value") {
            Some(PortData::Int(v)) => assert_eq!(*v, 42),
            _ => panic!("expected PortData::Int(42)"),
        }
    }

    #[test]
    fn test_constant_str_output() {
        let mut node = ConstantNode::new();
        let ctx = ExecutionContext::default();

        let mut inputs = HashMap::new();
        inputs.insert("type".to_string(), PortData::Str("Str".to_string()));
        inputs.insert("value".to_string(), PortData::Str("hello".to_string()));

        let outputs = node.execute(&inputs, &ctx).unwrap();
        match outputs.get("value") {
            Some(PortData::Str(v)) => assert_eq!(v, "hello"),
            _ => panic!("expected PortData::Str(\"hello\")"),
        }
    }

    #[test]
    fn test_constant_bool_output() {
        let mut node = ConstantNode::new();
        let ctx = ExecutionContext::default();

        let mut inputs = HashMap::new();
        inputs.insert("type".to_string(), PortData::Str("Bool".to_string()));
        inputs.insert("value".to_string(), PortData::Str("true".to_string()));

        let outputs = node.execute(&inputs, &ctx).unwrap();
        match outputs.get("value") {
            Some(PortData::Bool(v)) => assert!(*v),
            _ => panic!("expected PortData::Bool(true)"),
        }
    }

    #[test]
    fn test_constant_float_output() {
        let mut node = ConstantNode::new();
        let ctx = ExecutionContext::default();

        let mut inputs = HashMap::new();
        inputs.insert("type".to_string(), PortData::Str("Float".to_string()));
        inputs.insert("value".to_string(), PortData::Str("3.14".to_string()));

        let outputs = node.execute(&inputs, &ctx).unwrap();
        match outputs.get("value") {
            Some(PortData::Float(v)) => assert!((v - 3.14).abs() < f64::EPSILON),
            _ => panic!("expected PortData::Float(3.14)"),
        }
    }

    #[test]
    fn test_constant_dynamic_output_ports() {
        let mut node = ConstantNode::new();
        let ctx = ExecutionContext::default();

        assert_eq!(node.output_ports()[0].port_type, PortType::Int);

        let mut inputs = HashMap::new();
        inputs.insert("type".to_string(), PortData::Str("Str".to_string()));
        inputs.insert("value".to_string(), PortData::Str("test".to_string()));

        node.execute(&inputs, &ctx).unwrap();
        assert_eq!(node.output_ports()[0].port_type, PortType::Str);
    }

    #[test]
    fn test_constant_runtime_type_input_overrides_initialized_type() {
        let params = HashMap::from([("type".to_string(), serde_json::json!("Str"))]);
        let mut node = ConstantNode::from_params(&params)
            .expect("params should initialize Constant with Str output");
        let ctx = ExecutionContext::default();

        let inputs = HashMap::from([
            ("type".to_string(), PortData::Str("Int".to_string())),
            ("value".to_string(), PortData::Str("12".to_string())),
        ]);

        let outputs = node
            .execute(&inputs, &ctx)
            .expect("runtime type input should override initialized type");
        match outputs.get("value") {
            Some(PortData::Int(v)) => assert_eq!(*v, 12),
            _ => panic!("expected Int output after runtime override"),
        }
        assert_eq!(node.output_ports()[0].port_type, PortType::Int);
    }
}
