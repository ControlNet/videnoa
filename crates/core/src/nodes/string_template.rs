use std::collections::HashMap;

use anyhow::{bail, Result};

use crate::node::{ExecutionContext, Node, PortDefinition};
use crate::types::{PortData, PortType};

pub struct StringTemplateNode {
    num_input: usize,
}

impl StringTemplateNode {
    pub fn new() -> Self {
        Self { num_input: 0 }
    }

    pub fn from_params(params: &HashMap<String, serde_json::Value>) -> Self {
        let num_input = params
            .get("num_input")
            .and_then(serde_json::Value::as_i64)
            .map(|v| v.max(0) as usize)
            .unwrap_or(0);

        Self { num_input }
    }

    fn parse_num_input(value: i64) -> Result<usize> {
        if value < 0 {
            bail!("StringTemplate: num_input must be >= 0, got {value}");
        }

        Ok(value as usize)
    }
}

impl Default for StringTemplateNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for StringTemplateNode {
    fn node_type(&self) -> &str {
        "StringTemplate"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        let mut ports = vec![
            PortDefinition {
                name: "num_input".to_string(),
                port_type: PortType::Int,
                required: false,
                default_value: Some(serde_json::json!(self.num_input as i64)),
            },
            PortDefinition {
                name: "template".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("")),
            },
            PortDefinition {
                name: "strict".to_string(),
                port_type: PortType::Bool,
                required: false,
                default_value: Some(serde_json::json!(true)),
            },
        ];

        for idx in 0..self.num_input {
            ports.push(PortDefinition {
                name: format!("str{idx}"),
                port_type: PortType::Str,
                required: false,
                default_value: None,
            });
        }

        ports
    }

    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![PortDefinition {
            name: "value".to_string(),
            port_type: PortType::Str,
            required: true,
            default_value: None,
        }]
    }

    fn execute(
        &mut self,
        inputs: &HashMap<String, PortData>,
        _ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        let num_input = match inputs.get("num_input") {
            Some(PortData::Int(v)) => Self::parse_num_input(*v)?,
            Some(_) => bail!("StringTemplate: input 'num_input' must be Int"),
            None => self.num_input,
        };
        self.num_input = num_input;

        let template = match inputs.get("template") {
            Some(PortData::Str(value)) => value.as_str(),
            Some(_) => bail!("StringTemplate: input 'template' must be Str"),
            None => "",
        };

        let strict = match inputs.get("strict") {
            Some(PortData::Bool(value)) => *value,
            Some(_) => bail!("StringTemplate: input 'strict' must be Bool"),
            None => true,
        };

        let rendered = render_template_v1(template, inputs, num_input, strict)?;

        Ok(HashMap::from([(
            "value".to_string(),
            PortData::Str(rendered),
        )]))
    }
}

fn render_template_v1(
    template: &str,
    inputs: &HashMap<String, PortData>,
    num_input: usize,
    strict: bool,
) -> Result<String> {
    let mut result = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut cursor = 0;

    while cursor < bytes.len() {
        if bytes[cursor] == b'{' {
            if let Some(end_rel) = template[cursor + 1..].find('}') {
                let end = cursor + 1 + end_rel;
                let token = &template[cursor + 1..end];

                if let Some(index) = parse_placeholder_index(token) {
                    let port_name = format!("str{index}");

                    if index >= num_input {
                        if strict {
                            bail!(
                                "StringTemplate: unknown placeholder '{{{}}}' for num_input={}",
                                port_name,
                                num_input
                            );
                        }
                        result.push_str(&template[cursor..=end]);
                        cursor = end + 1;
                        continue;
                    }

                    match inputs.get(&port_name) {
                        Some(PortData::Str(value)) => result.push_str(value),
                        Some(_) => bail!(
                            "StringTemplate: placeholder '{{{}}}' expects Str input",
                            port_name
                        ),
                        None if strict => {
                            bail!(
                                "StringTemplate: missing value for placeholder '{{{}}}'",
                                port_name
                            )
                        }
                        None => result.push_str(&template[cursor..=end]),
                    }

                    cursor = end + 1;
                    continue;
                }

                if strict {
                    bail!("StringTemplate: unknown placeholder '{{{token}}}'");
                }

                result.push_str(&template[cursor..=end]);
                cursor = end + 1;
                continue;
            }
        }

        let ch = template[cursor..]
            .chars()
            .next()
            .expect("cursor must be within string bounds");
        result.push(ch);
        cursor += ch.len_utf8();
    }

    Ok(result)
}

fn parse_placeholder_index(token: &str) -> Option<usize> {
    let suffix = token.strip_prefix("str")?;
    if suffix.is_empty() || !suffix.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }

    suffix.parse::<usize>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_num_input_port_generation_from_params() {
        let mut params = HashMap::new();
        params.insert("num_input".to_string(), serde_json::json!(3));

        let node = StringTemplateNode::from_params(&params);
        let ports = node.input_ports();

        assert_eq!(ports[0].name, "num_input");
        assert_eq!(ports[0].port_type, PortType::Int);
        assert_eq!(ports[1].name, "template");
        assert_eq!(ports[1].port_type, PortType::Str);
        assert_eq!(ports[2].name, "strict");
        assert_eq!(ports[2].port_type, PortType::Bool);

        let dynamic: Vec<&str> = ports[3..].iter().map(|p| p.name.as_str()).collect();
        assert_eq!(dynamic, vec!["str0", "str1", "str2"]);
    }

    #[test]
    fn test_successful_render_with_str1_placeholder() {
        let mut node = StringTemplateNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("num_input".to_string(), PortData::Int(2));
        inputs.insert(
            "template".to_string(),
            PortData::Str("a {str1} b".to_string()),
        );
        inputs.insert("strict".to_string(), PortData::Bool(true));
        inputs.insert("str1".to_string(), PortData::Str("X".to_string()));

        let outputs = node.execute(&inputs, &ctx).unwrap();
        match outputs.get("value") {
            Some(PortData::Str(value)) => assert_eq!(value, "a X b"),
            _ => panic!("expected rendered string output"),
        }

        let port_names: Vec<String> = node.input_ports().into_iter().map(|p| p.name).collect();
        assert!(port_names.contains(&"str0".to_string()));
        assert!(port_names.contains(&"str1".to_string()));
    }

    #[test]
    fn test_strict_mode_errors_on_unknown_placeholder() {
        let mut node = StringTemplateNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("num_input".to_string(), PortData::Int(1));
        inputs.insert(
            "template".to_string(),
            PortData::Str("a {str9} b".to_string()),
        );
        inputs.insert("strict".to_string(), PortData::Bool(true));

        let err = match node.execute(&inputs, &ctx) {
            Ok(_) => panic!("strict mode must fail"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("unknown placeholder"));
        assert!(err.to_string().contains("{str9}"));
    }

    #[test]
    fn test_non_strict_mode_keeps_unknown_placeholder_unchanged() {
        let mut node = StringTemplateNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("num_input".to_string(), PortData::Int(1));
        inputs.insert(
            "template".to_string(),
            PortData::Str("a {str9} {x} b".to_string()),
        );
        inputs.insert("strict".to_string(), PortData::Bool(false));

        let outputs = node.execute(&inputs, &ctx).unwrap();
        match outputs.get("value") {
            Some(PortData::Str(value)) => assert_eq!(value, "a {str9} {x} b"),
            _ => panic!("expected rendered string output"),
        }
    }

    #[test]
    fn test_strict_mode_errors_on_missing_placeholder_value() {
        let mut node = StringTemplateNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("num_input".to_string(), PortData::Int(2));
        inputs.insert(
            "template".to_string(),
            PortData::Str("prefix-{str0}-{str1}".to_string()),
        );
        inputs.insert("strict".to_string(), PortData::Bool(true));
        inputs.insert("str0".to_string(), PortData::Str("A".to_string()));

        let err = match node.execute(&inputs, &ctx) {
            Ok(_) => panic!("strict mode should fail when placeholder value is missing"),
            Err(err) => err,
        };
        assert_eq!(
            err.to_string(),
            "StringTemplate: missing value for placeholder '{str1}'"
        );
    }

    #[test]
    fn test_non_strict_mode_keeps_missing_placeholder_value_unchanged() {
        let mut node = StringTemplateNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("num_input".to_string(), PortData::Int(2));
        inputs.insert(
            "template".to_string(),
            PortData::Str("prefix-{str0}-{str1}".to_string()),
        );
        inputs.insert("strict".to_string(), PortData::Bool(false));
        inputs.insert("str0".to_string(), PortData::Str("A".to_string()));

        let outputs = node
            .execute(&inputs, &ctx)
            .expect("non-strict mode should keep unresolved token");
        match outputs.get("value") {
            Some(PortData::Str(value)) => assert_eq!(value, "prefix-A-{str1}"),
            _ => panic!("expected rendered string output"),
        }
    }

    #[test]
    fn test_execute_rejects_invalid_num_input_type() {
        let mut node = StringTemplateNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("num_input".to_string(), PortData::Str("2".to_string()));
        inputs.insert("template".to_string(), PortData::Str("{str0}".to_string()));

        let err = match node.execute(&inputs, &ctx) {
            Ok(_) => panic!("num_input must be an Int input"),
            Err(err) => err,
        };
        assert_eq!(
            err.to_string(),
            "StringTemplate: input 'num_input' must be Int"
        );
    }
}
