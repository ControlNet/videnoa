use std::collections::HashMap;

#[cfg(test)]
use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::node::{ExecutionContext, Node, PortDefinition};
use crate::types::{PortData, PortType};

pub struct PathJoinerNode;

impl PathJoinerNode {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PathJoinerNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for PathJoinerNode {
    fn node_type(&self) -> &str {
        "PathJoiner"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![
            PortDefinition {
                name: "parent_path".to_string(),
                port_type: PortType::Path,
                required: true,
                default_value: None,
            },
            PortDefinition {
                name: "sub_path".to_string(),
                port_type: PortType::Path,
                required: false,
                default_value: Some(serde_json::json!("")),
            },
            PortDefinition {
                name: "file_name".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("")),
            },
        ]
    }

    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![PortDefinition {
            name: "path".to_string(),
            port_type: PortType::Path,
            required: true,
            default_value: None,
        }]
    }

    fn execute(
        &mut self,
        inputs: &HashMap<String, PortData>,
        _ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        let parent_path = match inputs.get("parent_path") {
            Some(PortData::Path(path)) => path,
            _ => bail!("PathJoiner requires input port 'parent_path' of type Path"),
        };

        let mut joined = parent_path.clone();

        match inputs.get("sub_path") {
            Some(PortData::Path(sub_path)) => {
                if !sub_path.as_os_str().is_empty() {
                    joined = joined.join(sub_path);
                }
            }
            Some(_) => {
                bail!("PathJoiner optional input port 'sub_path' must be type Path when provided")
            }
            None => {}
        }

        match inputs.get("file_name") {
            Some(PortData::Str(file_name)) => {
                if !file_name.is_empty() {
                    joined = joined.join(file_name);
                }
            }
            Some(_) => {
                bail!("PathJoiner optional input port 'file_name' must be type Str when provided")
            }
            None => {}
        }

        Ok(HashMap::from([(
            "path".to_string(),
            PortData::Path(joined),
        )]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_node(
        parent_path: &str,
        sub_path: Option<PathBuf>,
        file_name: Option<&str>,
    ) -> HashMap<String, PortData> {
        let mut node = PathJoinerNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::from([(
            "parent_path".to_string(),
            PortData::Path(PathBuf::from(parent_path)),
        )]);
        if let Some(sub_path) = sub_path {
            inputs.insert("sub_path".to_string(), PortData::Path(sub_path));
        }
        if let Some(file_name) = file_name {
            inputs.insert(
                "file_name".to_string(),
                PortData::Str(file_name.to_string()),
            );
        }

        node.execute(&inputs, &ctx).expect("PathJoiner execution")
    }

    fn expect_path(outputs: &HashMap<String, PortData>, key: &str) -> PathBuf {
        match outputs.get(key) {
            Some(PortData::Path(v)) => v.clone(),
            _ => panic!("expected path output for key '{key}'"),
        }
    }

    #[test]
    fn test_path_joiner_contract() {
        let node = PathJoinerNode::new();
        assert_eq!(node.node_type(), "PathJoiner");

        let inputs = node.input_ports();
        assert_eq!(inputs.len(), 3);
        assert_eq!(inputs[0].name, "parent_path");
        assert_eq!(inputs[0].port_type, PortType::Path);
        assert!(inputs[0].required);

        assert_eq!(inputs[1].name, "sub_path");
        assert_eq!(inputs[1].port_type, PortType::Path);
        assert!(!inputs[1].required);
        assert_eq!(inputs[1].default_value, Some(serde_json::json!("")));

        assert_eq!(inputs[2].name, "file_name");
        assert_eq!(inputs[2].port_type, PortType::Str);
        assert!(!inputs[2].required);
        assert_eq!(inputs[2].default_value, Some(serde_json::json!("")));

        let outputs = node.output_ports();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].name, "path");
        assert_eq!(outputs[0].port_type, PortType::Path);
    }

    #[test]
    fn test_path_joiner_joins_parent_sub_and_file_name() {
        let outputs = run_node(
            "anime",
            Some(PathBuf::from("season1")),
            Some("episode01.mkv"),
        );

        assert_eq!(
            expect_path(&outputs, "path"),
            PathBuf::from("anime").join("season1").join("episode01.mkv")
        );
    }

    #[test]
    fn test_path_joiner_skips_empty_optional_segments() {
        let both_empty = run_node("anime", Some(PathBuf::new()), Some(""));
        assert_eq!(expect_path(&both_empty, "path"), PathBuf::from("anime"));

        let empty_sub = run_node("anime", Some(PathBuf::new()), Some("episode01.mkv"));
        assert_eq!(
            expect_path(&empty_sub, "path"),
            PathBuf::from("anime").join("episode01.mkv")
        );

        let empty_file = run_node("anime", Some(PathBuf::from("season1")), Some(""));
        assert_eq!(
            expect_path(&empty_file, "path"),
            PathBuf::from("anime").join("season1")
        );
    }

    #[test]
    fn test_path_joiner_returns_parent_when_optional_inputs_missing() {
        let outputs = run_node("anime", None, None);
        assert_eq!(expect_path(&outputs, "path"), PathBuf::from("anime"));
    }

    #[test]
    fn test_path_joiner_missing_parent_fails_fast() {
        let mut node = PathJoinerNode::new();
        let inputs = HashMap::new();

        let err = match node.execute(&inputs, &ExecutionContext::default()) {
            Ok(_) => panic!("missing 'parent_path' should fail"),
            Err(err) => err,
        };
        assert_eq!(
            err.to_string(),
            "PathJoiner requires input port 'parent_path' of type Path"
        );
    }

    #[test]
    fn test_path_joiner_wrong_parent_type_fails_fast() {
        let mut node = PathJoinerNode::new();
        let inputs = HashMap::from([(
            "parent_path".to_string(),
            PortData::Str("anime".to_string()),
        )]);

        let err = match node.execute(&inputs, &ExecutionContext::default()) {
            Ok(_) => panic!("non-Path 'parent_path' should fail"),
            Err(err) => err,
        };
        assert_eq!(
            err.to_string(),
            "PathJoiner requires input port 'parent_path' of type Path"
        );
    }

    #[test]
    fn test_path_joiner_wrong_optional_types_fail_fast() {
        let mut node = PathJoinerNode::new();

        let wrong_sub_path = HashMap::from([
            (
                "parent_path".to_string(),
                PortData::Path(PathBuf::from("anime")),
            ),
            ("sub_path".to_string(), PortData::Str("season1".to_string())),
        ]);
        let sub_err = match node.execute(&wrong_sub_path, &ExecutionContext::default()) {
            Ok(_) => panic!("non-Path 'sub_path' should fail"),
            Err(err) => err,
        };
        assert_eq!(
            sub_err.to_string(),
            "PathJoiner optional input port 'sub_path' must be type Path when provided"
        );

        let wrong_file_name = HashMap::from([
            (
                "parent_path".to_string(),
                PortData::Path(PathBuf::from("anime")),
            ),
            (
                "file_name".to_string(),
                PortData::Path(PathBuf::from("episode01.mkv")),
            ),
        ]);
        let file_err = match node.execute(&wrong_file_name, &ExecutionContext::default()) {
            Ok(_) => panic!("non-Str 'file_name' should fail"),
            Err(err) => err,
        };
        assert_eq!(
            file_err.to_string(),
            "PathJoiner optional input port 'file_name' must be type Str when provided"
        );
    }
}
