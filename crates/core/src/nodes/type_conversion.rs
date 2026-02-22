use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};

use crate::node::{ExecutionContext, Node, PortDefinition};
use crate::types::{PortData, PortType};

pub struct TypeConversionNode {
    input_type: PortType,
    output_type: PortType,
}

impl TypeConversionNode {
    pub fn new() -> Self {
        Self {
            input_type: PortType::Int,
            output_type: PortType::Int,
        }
    }

    pub fn from_params(params: &HashMap<String, serde_json::Value>) -> Result<Self> {
        let input_type = parse_param_port_type(params, "input_type")?.unwrap_or(PortType::Int);
        let output_type = parse_param_port_type(params, "output_type")?.unwrap_or(PortType::Int);

        Ok(Self {
            input_type,
            output_type,
        })
    }

    fn parse_and_apply_types_from_inputs(
        &mut self,
        inputs: &HashMap<String, PortData>,
    ) -> Result<()> {
        let input_type =
            parse_input_port_type(inputs, "input_type")?.unwrap_or(self.input_type.clone());
        let output_type =
            parse_input_port_type(inputs, "output_type")?.unwrap_or(self.output_type.clone());

        self.input_type = input_type;
        self.output_type = output_type;

        Ok(())
    }
}

impl Default for TypeConversionNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for TypeConversionNode {
    fn node_type(&self) -> &str {
        "TypeConversion"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![
            PortDefinition {
                name: "input_type".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!(port_type_name(&self.input_type))),
            },
            PortDefinition {
                name: "output_type".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!(port_type_name(&self.output_type))),
            },
            PortDefinition {
                name: "value".to_string(),
                port_type: self.input_type.clone(),
                required: true,
                default_value: None,
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
        self.parse_and_apply_types_from_inputs(inputs)?;

        let value = inputs
            .get("value")
            .ok_or_else(|| anyhow!("TypeConversion: input 'value' is required"))?;

        let converted = convert_value(value, &self.input_type, &self.output_type)?;
        Ok(HashMap::from([("value".to_string(), converted)]))
    }
}

fn parse_param_port_type(
    params: &HashMap<String, serde_json::Value>,
    key: &str,
) -> Result<Option<PortType>> {
    let Some(value) = params.get(key) else {
        return Ok(None);
    };

    let value = value.as_str().ok_or_else(|| {
        anyhow!(
            "TypeConversion: param '{key}' must be a string type name (Int|Float|Str|Bool|Path)"
        )
    })?;

    Ok(Some(parse_supported_type(value, key)?))
}

fn parse_input_port_type(
    inputs: &HashMap<String, PortData>,
    key: &str,
) -> Result<Option<PortType>> {
    let Some(value) = inputs.get(key) else {
        return Ok(None);
    };

    let raw = match value {
        PortData::Str(raw) => raw.as_str(),
        other => {
            bail!(
                "TypeConversion: input '{key}' must be Str, got {}",
                port_data_kind(other)
            )
        }
    };

    Ok(Some(parse_supported_type(raw, key)?))
}

fn parse_supported_type(raw: &str, key: &str) -> Result<PortType> {
    match raw {
        "Int" => Ok(PortType::Int),
        "Float" => Ok(PortType::Float),
        "Str" => Ok(PortType::Str),
        "Bool" => Ok(PortType::Bool),
        "Path" => Ok(PortType::Path),
        other => bail!(
            "TypeConversion: unsupported {key} '{other}', expected one of Int|Float|Str|Bool|Path"
        ),
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

fn validate_str_for_path(value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("TypeConversion: cannot convert Str to Path from empty or whitespace-only value");
    }

    if value.contains('\0') {
        bail!("TypeConversion: cannot convert Str to Path because value contains NUL ('\\0')");
    }

    Ok(())
}

fn convert_value(
    value: &PortData,
    input_type: &PortType,
    output_type: &PortType,
) -> Result<PortData> {
    let mismatch = || {
        anyhow!(
            "TypeConversion: input 'value' expected {}, got {}",
            port_type_name(input_type),
            port_data_kind(value)
        )
    };

    match (input_type, output_type) {
        (PortType::Int, PortType::Int) => match value {
            PortData::Int(v) => Ok(PortData::Int(*v)),
            _ => Err(mismatch()),
        },
        (PortType::Int, PortType::Float) => match value {
            PortData::Int(v) => Ok(PortData::Float(*v as f64)),
            _ => Err(mismatch()),
        },
        (PortType::Int, PortType::Str) => match value {
            PortData::Int(v) => Ok(PortData::Str(v.to_string())),
            _ => Err(mismatch()),
        },
        (PortType::Int, PortType::Bool) => match value {
            PortData::Int(v) => match *v {
                0 => Ok(PortData::Bool(false)),
                1 => Ok(PortData::Bool(true)),
                other => bail!(
                    "TypeConversion: cannot convert Int '{}' to Bool; expected 0 or 1",
                    other
                ),
            },
            _ => Err(mismatch()),
        },
        (PortType::Int, PortType::Path) => match value {
            PortData::Int(_) => bail!("TypeConversion: conversion Int -> Path is not supported"),
            _ => Err(mismatch()),
        },

        (PortType::Float, PortType::Int) => match value {
            PortData::Float(v) => {
                if !v.is_finite() {
                    bail!(
                        "TypeConversion: cannot convert non-finite Float '{}' to Int",
                        v
                    );
                }
                if v.fract() != 0.0 {
                    bail!(
                        "TypeConversion: cannot convert Float '{}' to Int without precision loss",
                        v
                    );
                }
                if *v < i64::MIN as f64 || *v > i64::MAX as f64 {
                    bail!("TypeConversion: Float '{}' is out of Int range", v);
                }
                Ok(PortData::Int(*v as i64))
            }
            _ => Err(mismatch()),
        },
        (PortType::Float, PortType::Float) => match value {
            PortData::Float(v) => Ok(PortData::Float(*v)),
            _ => Err(mismatch()),
        },
        (PortType::Float, PortType::Str) => match value {
            PortData::Float(v) => Ok(PortData::Str(v.to_string())),
            _ => Err(mismatch()),
        },
        (PortType::Float, PortType::Bool) => match value {
            PortData::Float(v) => {
                if *v == 0.0 {
                    Ok(PortData::Bool(false))
                } else if *v == 1.0 {
                    Ok(PortData::Bool(true))
                } else {
                    bail!(
                        "TypeConversion: cannot convert Float '{}' to Bool; expected 0.0 or 1.0",
                        v
                    );
                }
            }
            _ => Err(mismatch()),
        },
        (PortType::Float, PortType::Path) => match value {
            PortData::Float(_) => {
                bail!("TypeConversion: conversion Float -> Path is not supported")
            }
            _ => Err(mismatch()),
        },

        (PortType::Str, PortType::Int) => match value {
            PortData::Str(v) => {
                let parsed = v.parse::<i64>().map_err(|e| {
                    anyhow!("TypeConversion: failed to parse '{}' as Int: {}", v, e)
                })?;
                Ok(PortData::Int(parsed))
            }
            _ => Err(mismatch()),
        },
        (PortType::Str, PortType::Float) => match value {
            PortData::Str(v) => {
                let parsed = v.parse::<f64>().map_err(|e| {
                    anyhow!("TypeConversion: failed to parse '{}' as Float: {}", v, e)
                })?;
                if !parsed.is_finite() {
                    bail!(
                        "TypeConversion: cannot convert Str '{}' to Float; parsed value is non-finite",
                        v
                    );
                }
                Ok(PortData::Float(parsed))
            }
            _ => Err(mismatch()),
        },
        (PortType::Str, PortType::Str) => match value {
            PortData::Str(v) => Ok(PortData::Str(v.clone())),
            _ => Err(mismatch()),
        },
        (PortType::Str, PortType::Bool) => match value {
            PortData::Str(v) => match v.as_str() {
                "true" => Ok(PortData::Bool(true)),
                "false" => Ok(PortData::Bool(false)),
                _ => bail!(
                    "TypeConversion: cannot convert Str '{}' to Bool; expected 'true' or 'false'",
                    v
                ),
            },
            _ => Err(mismatch()),
        },
        (PortType::Str, PortType::Path) => match value {
            PortData::Str(v) => {
                validate_str_for_path(v)?;
                Ok(PortData::Path(PathBuf::from(v)))
            }
            _ => Err(mismatch()),
        },

        (PortType::Bool, PortType::Int) => match value {
            PortData::Bool(v) => Ok(PortData::Int(if *v { 1 } else { 0 })),
            _ => Err(mismatch()),
        },
        (PortType::Bool, PortType::Float) => match value {
            PortData::Bool(v) => Ok(PortData::Float(if *v { 1.0 } else { 0.0 })),
            _ => Err(mismatch()),
        },
        (PortType::Bool, PortType::Str) => match value {
            PortData::Bool(v) => Ok(PortData::Str(v.to_string())),
            _ => Err(mismatch()),
        },
        (PortType::Bool, PortType::Bool) => match value {
            PortData::Bool(v) => Ok(PortData::Bool(*v)),
            _ => Err(mismatch()),
        },
        (PortType::Bool, PortType::Path) => match value {
            PortData::Bool(_) => bail!("TypeConversion: conversion Bool -> Path is not supported"),
            _ => Err(mismatch()),
        },

        (PortType::Path, PortType::Int) => match value {
            PortData::Path(_) => bail!("TypeConversion: conversion Path -> Int is not supported"),
            _ => Err(mismatch()),
        },
        (PortType::Path, PortType::Float) => match value {
            PortData::Path(_) => bail!("TypeConversion: conversion Path -> Float is not supported"),
            _ => Err(mismatch()),
        },
        (PortType::Path, PortType::Str) => match value {
            PortData::Path(v) => Ok(PortData::Str(v.to_string_lossy().into_owned())),
            _ => Err(mismatch()),
        },
        (PortType::Path, PortType::Bool) => match value {
            PortData::Path(_) => bail!("TypeConversion: conversion Path -> Bool is not supported"),
            _ => Err(mismatch()),
        },
        (PortType::Path, PortType::Path) => match value {
            PortData::Path(v) => Ok(PortData::Path(v.clone())),
            _ => Err(mismatch()),
        },

        _ => bail!(
            "TypeConversion: unsupported conversion {} -> {}",
            port_type_name(input_type),
            port_type_name(output_type)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_conversion_contract_and_default_ports() {
        let node = TypeConversionNode::new();

        assert_eq!(node.node_type(), "TypeConversion");

        let inputs = node.input_ports();
        assert_eq!(inputs.len(), 3);
        assert_eq!(inputs[0].name, "input_type");
        assert_eq!(inputs[0].port_type, PortType::Str);
        assert_eq!(inputs[0].default_value, Some(serde_json::json!("Int")));
        assert_eq!(inputs[1].name, "output_type");
        assert_eq!(inputs[1].port_type, PortType::Str);
        assert_eq!(inputs[1].default_value, Some(serde_json::json!("Int")));
        assert_eq!(inputs[2].name, "value");
        assert_eq!(inputs[2].port_type, PortType::Int);

        let outputs = node.output_ports();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].name, "value");
        assert_eq!(outputs[0].port_type, PortType::Int);
    }

    #[test]
    fn test_type_conversion_dynamic_ports_follow_configured_types_from_params() {
        let params = HashMap::from([
            ("input_type".to_string(), serde_json::json!("Path")),
            ("output_type".to_string(), serde_json::json!("Str")),
        ]);

        let node = TypeConversionNode::from_params(&params).expect("node from params should build");

        let input_ports = node.input_ports();
        assert_eq!(input_ports[2].name, "value");
        assert_eq!(input_ports[2].port_type, PortType::Path);

        let output_ports = node.output_ports();
        assert_eq!(output_ports[0].name, "value");
        assert_eq!(output_ports[0].port_type, PortType::Str);
    }

    #[test]
    fn test_type_conversion_str_to_int_success() {
        let mut node = TypeConversionNode::new();
        let ctx = ExecutionContext::default();

        let inputs = HashMap::from([
            ("input_type".to_string(), PortData::Str("Str".to_string())),
            ("output_type".to_string(), PortData::Str("Int".to_string())),
            ("value".to_string(), PortData::Str("42".to_string())),
        ]);

        let outputs = node
            .execute(&inputs, &ctx)
            .expect("Str -> Int should succeed");
        match outputs.get("value") {
            Some(PortData::Int(v)) => assert_eq!(*v, 42),
            _ => panic!("expected Int output"),
        }
    }

    #[test]
    fn test_type_conversion_str_to_int_invalid_failure() {
        let mut node = TypeConversionNode::new();
        let ctx = ExecutionContext::default();

        let inputs = HashMap::from([
            ("input_type".to_string(), PortData::Str("Str".to_string())),
            ("output_type".to_string(), PortData::Str("Int".to_string())),
            ("value".to_string(), PortData::Str("12x".to_string())),
        ]);

        let err = match node.execute(&inputs, &ctx) {
            Ok(_) => panic!("invalid Str -> Int must fail deterministically"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("failed to parse '12x' as Int"));
    }

    #[test]
    fn test_type_conversion_int_to_str_success() {
        let mut node = TypeConversionNode::new();
        let ctx = ExecutionContext::default();

        let inputs = HashMap::from([
            ("input_type".to_string(), PortData::Str("Int".to_string())),
            ("output_type".to_string(), PortData::Str("Str".to_string())),
            ("value".to_string(), PortData::Int(7)),
        ]);

        let outputs = node
            .execute(&inputs, &ctx)
            .expect("Int -> Str should succeed");
        match outputs.get("value") {
            Some(PortData::Str(v)) => assert_eq!(v, "7"),
            _ => panic!("expected Str output"),
        }
    }

    #[test]
    fn test_type_conversion_str_to_path_and_path_to_str_success() {
        let mut node = TypeConversionNode::new();
        let ctx = ExecutionContext::default();

        let str_to_path = HashMap::from([
            ("input_type".to_string(), PortData::Str("Str".to_string())),
            ("output_type".to_string(), PortData::Str("Path".to_string())),
            (
                "value".to_string(),
                PortData::Str("/tmp/episode01.mkv".to_string()),
            ),
        ]);
        let outputs = node
            .execute(&str_to_path, &ctx)
            .expect("Str -> Path should succeed for non-empty values");
        match outputs.get("value") {
            Some(PortData::Path(v)) => assert_eq!(v, &PathBuf::from("/tmp/episode01.mkv")),
            _ => panic!("expected Path output from Str -> Path"),
        }

        let path_to_str = HashMap::from([
            ("input_type".to_string(), PortData::Str("Path".to_string())),
            ("output_type".to_string(), PortData::Str("Str".to_string())),
            (
                "value".to_string(),
                PortData::Path(PathBuf::from("relative/episode01.mkv")),
            ),
        ]);
        let outputs = node
            .execute(&path_to_str, &ctx)
            .expect("Path -> Str should remain supported");
        match outputs.get("value") {
            Some(PortData::Str(v)) => assert_eq!(v, "relative/episode01.mkv"),
            _ => panic!("expected Str output from Path -> Str"),
        }
    }

    #[test]
    fn test_type_conversion_str_to_path_rejects_empty_or_whitespace_only() {
        let mut node = TypeConversionNode::new();
        let ctx = ExecutionContext::default();

        for bad in ["", "   ", "\t\n"] {
            let inputs = HashMap::from([
                ("input_type".to_string(), PortData::Str("Str".to_string())),
                ("output_type".to_string(), PortData::Str("Path".to_string())),
                ("value".to_string(), PortData::Str(bad.to_string())),
            ]);

            let err = match node.execute(&inputs, &ctx) {
                Ok(_) => panic!("invalid Str -> Path value '{bad:?}' must fail"),
                Err(err) => err,
            };
            assert_eq!(
                err.to_string(),
                "TypeConversion: cannot convert Str to Path from empty or whitespace-only value"
            );
        }
    }

    #[test]
    fn test_type_conversion_str_to_path_rejects_nul_characters() {
        let mut node = TypeConversionNode::new();
        let ctx = ExecutionContext::default();

        let inputs = HashMap::from([
            ("input_type".to_string(), PortData::Str("Str".to_string())),
            ("output_type".to_string(), PortData::Str("Path".to_string())),
            (
                "value".to_string(),
                PortData::Str("episode\0name.mkv".to_string()),
            ),
        ]);

        let err = match node.execute(&inputs, &ctx) {
            Ok(_) => panic!("NUL-containing Str -> Path input must fail deterministically"),
            Err(err) => err,
        };
        assert_eq!(
            err.to_string(),
            "TypeConversion: cannot convert Str to Path because value contains NUL ('\\0')"
        );
    }

    #[test]
    fn test_type_conversion_bool_string_clarity() {
        let mut node = TypeConversionNode::new();
        let ctx = ExecutionContext::default();

        let bool_to_str = HashMap::from([
            ("input_type".to_string(), PortData::Str("Bool".to_string())),
            ("output_type".to_string(), PortData::Str("Str".to_string())),
            ("value".to_string(), PortData::Bool(true)),
        ]);
        let outputs = node
            .execute(&bool_to_str, &ctx)
            .expect("Bool -> Str should succeed");
        match outputs.get("value") {
            Some(PortData::Str(v)) => assert_eq!(v, "true"),
            _ => panic!("expected Str output from Bool -> Str"),
        }

        let str_to_bool = HashMap::from([
            ("input_type".to_string(), PortData::Str("Str".to_string())),
            ("output_type".to_string(), PortData::Str("Bool".to_string())),
            ("value".to_string(), PortData::Str("false".to_string())),
        ]);
        let outputs = node
            .execute(&str_to_bool, &ctx)
            .expect("Str -> Bool should succeed");
        match outputs.get("value") {
            Some(PortData::Bool(v)) => assert!(!v),
            _ => panic!("expected Bool output from Str -> Bool"),
        }

        let ambiguous = HashMap::from([
            ("input_type".to_string(), PortData::Str("Str".to_string())),
            ("output_type".to_string(), PortData::Str("Bool".to_string())),
            ("value".to_string(), PortData::Str("TRUE".to_string())),
        ]);
        let err = match node.execute(&ambiguous, &ctx) {
            Ok(_) => panic!("ambiguous Str -> Bool value must fail"),
            Err(err) => err,
        };
        assert!(err
            .to_string()
            .contains("cannot convert Str 'TRUE' to Bool; expected 'true' or 'false'"));
    }

    #[test]
    fn test_type_conversion_dynamic_ports_update_after_execute() {
        let mut node = TypeConversionNode::new();
        let ctx = ExecutionContext::default();

        assert_eq!(node.input_ports()[2].port_type, PortType::Int);
        assert_eq!(node.output_ports()[0].port_type, PortType::Int);

        let inputs = HashMap::from([
            ("input_type".to_string(), PortData::Str("Path".to_string())),
            ("output_type".to_string(), PortData::Str("Str".to_string())),
            (
                "value".to_string(),
                PortData::Path(PathBuf::from("/tmp/episode01.mkv")),
            ),
        ]);
        node.execute(&inputs, &ctx)
            .expect("Path -> Str should succeed and update dynamic ports");

        assert_eq!(node.input_ports()[2].port_type, PortType::Path);
        assert_eq!(node.output_ports()[0].port_type, PortType::Str);
    }

    #[test]
    fn test_type_conversion_value_type_mismatch_is_deterministic() {
        let mut node = TypeConversionNode::new();
        let ctx = ExecutionContext::default();

        let inputs = HashMap::from([
            ("input_type".to_string(), PortData::Str("Int".to_string())),
            ("output_type".to_string(), PortData::Str("Str".to_string())),
            ("value".to_string(), PortData::Str("12".to_string())),
        ]);

        let err = match node.execute(&inputs, &ctx) {
            Ok(_) => panic!("mismatched value type should fail"),
            Err(err) => err,
        };
        assert!(err
            .to_string()
            .contains("TypeConversion: input 'value' expected Int, got Str"));
    }

    #[test]
    fn test_type_conversion_rejects_unsupported_dynamic_type_name() {
        let mut node = TypeConversionNode::new();
        let ctx = ExecutionContext::default();

        let inputs = HashMap::from([
            (
                "input_type".to_string(),
                PortData::Str("VideoFrames".to_string()),
            ),
            ("output_type".to_string(), PortData::Str("Str".to_string())),
            ("value".to_string(), PortData::Str("x".to_string())),
        ]);

        let err = match node.execute(&inputs, &ctx) {
            Ok(_) => panic!("unsupported type names must fail"),
            Err(err) => err,
        };
        assert_eq!(
            err.to_string(),
            "TypeConversion: unsupported input_type 'VideoFrames', expected one of Int|Float|Str|Bool|Path"
        );
    }

    #[test]
    fn test_type_conversion_fails_fast_when_value_missing() {
        let mut node = TypeConversionNode::new();
        let ctx = ExecutionContext::default();

        let inputs = HashMap::from([
            ("input_type".to_string(), PortData::Str("Str".to_string())),
            ("output_type".to_string(), PortData::Str("Int".to_string())),
        ]);

        let err = match node.execute(&inputs, &ctx) {
            Ok(_) => panic!("missing value input should fail"),
            Err(err) => err,
        };
        assert_eq!(err.to_string(), "TypeConversion: input 'value' is required");
    }

    #[test]
    fn test_type_conversion_rejects_precision_losing_float_to_int() {
        let mut node = TypeConversionNode::new();
        let ctx = ExecutionContext::default();

        let inputs = HashMap::from([
            ("input_type".to_string(), PortData::Str("Float".to_string())),
            ("output_type".to_string(), PortData::Str("Int".to_string())),
            ("value".to_string(), PortData::Float(3.14)),
        ]);

        let err = match node.execute(&inputs, &ctx) {
            Ok(_) => panic!("non-integer float should not silently truncate"),
            Err(err) => err,
        };
        assert_eq!(
            err.to_string(),
            "TypeConversion: cannot convert Float '3.14' to Int without precision loss"
        );
    }
}
