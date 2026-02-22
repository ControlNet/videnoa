use std::collections::HashMap;

use anyhow::{bail, Result};

use crate::node::{ExecutionContext, Node, PortDefinition};
use crate::types::{PortData, PortType};

pub struct StringReplaceNode;

impl StringReplaceNode {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StringReplaceNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for StringReplaceNode {
    fn node_type(&self) -> &str {
        "StringReplace"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![
            PortDefinition {
                name: "input".to_string(),
                port_type: PortType::Str,
                required: true,
                default_value: None,
            },
            PortDefinition {
                name: "old".to_string(),
                port_type: PortType::Str,
                required: true,
                default_value: None,
            },
            PortDefinition {
                name: "new".to_string(),
                port_type: PortType::Str,
                required: true,
                default_value: None,
            },
        ]
    }

    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![PortDefinition {
            name: "output".to_string(),
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
        let input = match inputs.get("input") {
            Some(PortData::Str(v)) => v,
            _ => bail!("StringReplace requires input port 'input' of type Str"),
        };
        let old = match inputs.get("old") {
            Some(PortData::Str(v)) => v,
            _ => bail!("StringReplace requires input port 'old' of type Str"),
        };
        let new = match inputs.get("new") {
            Some(PortData::Str(v)) => v,
            _ => bail!("StringReplace requires input port 'new' of type Str"),
        };

        Ok(HashMap::from([(
            "output".to_string(),
            PortData::Str(input.replace(old, new)),
        )]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_replace_contract() {
        let node = StringReplaceNode::new();
        assert_eq!(node.node_type(), "StringReplace");

        let inputs = node.input_ports();
        assert_eq!(inputs.len(), 3);
        assert_eq!(inputs[0].name, "input");
        assert_eq!(inputs[0].port_type, PortType::Str);
        assert_eq!(inputs[1].name, "old");
        assert_eq!(inputs[1].port_type, PortType::Str);
        assert_eq!(inputs[2].name, "new");
        assert_eq!(inputs[2].port_type, PortType::Str);

        let outputs = node.output_ports();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].name, "output");
        assert_eq!(outputs[0].port_type, PortType::Str);
    }

    #[test]
    fn test_string_replace_replaces_all_occurrences() {
        let mut node = StringReplaceNode::new();
        let inputs = HashMap::from([
            ("input".to_string(), PortData::Str("aa-bb-aa".to_string())),
            ("old".to_string(), PortData::Str("aa".to_string())),
            ("new".to_string(), PortData::Str("cc".to_string())),
        ]);

        let outputs = node
            .execute(&inputs, &ExecutionContext::default())
            .expect("StringReplace execution");

        match outputs.get("output") {
            Some(PortData::Str(v)) => assert_eq!(v, "cc-bb-cc"),
            _ => panic!("expected Str output on 'output'"),
        }
    }

    #[test]
    fn test_string_replace_missing_required_input_fails_fast() {
        let mut node = StringReplaceNode::new();
        let inputs = HashMap::from([
            ("input".to_string(), PortData::Str("abc".to_string())),
            ("old".to_string(), PortData::Str("a".to_string())),
        ]);

        let err = match node.execute(&inputs, &ExecutionContext::default()) {
            Ok(_) => panic!("missing 'new' should fail"),
            Err(err) => err,
        };

        assert_eq!(
            err.to_string(),
            "StringReplace requires input port 'new' of type Str"
        );
    }

    #[test]
    fn test_string_replace_wrong_input_type_fails_fast() {
        let mut node = StringReplaceNode::new();
        let inputs = HashMap::from([
            ("input".to_string(), PortData::Str("abc".to_string())),
            ("old".to_string(), PortData::Int(1)),
            ("new".to_string(), PortData::Str("x".to_string())),
        ]);

        let err = match node.execute(&inputs, &ExecutionContext::default()) {
            Ok(_) => panic!("non-Str 'old' should fail"),
            Err(err) => err,
        };

        assert_eq!(
            err.to_string(),
            "StringReplace requires input port 'old' of type Str"
        );
    }
}
