use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use crate::node::{ExecutionContext, Node, PortDefinition};
use crate::types::{PortData, PortType};

pub struct PathDividerNode;

impl PathDividerNode {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PathDividerNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for PathDividerNode {
    fn node_type(&self) -> &str {
        "PathDivider"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![PortDefinition {
            name: "path".to_string(),
            port_type: PortType::Path,
            required: true,
            default_value: None,
        }]
    }

    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![
            PortDefinition {
                name: "parent_path".to_string(),
                port_type: PortType::Path,
                required: true,
                default_value: None,
            },
            PortDefinition {
                name: "file_name".to_string(),
                port_type: PortType::Str,
                required: true,
                default_value: None,
            },
            PortDefinition {
                name: "file_stem".to_string(),
                port_type: PortType::Str,
                required: true,
                default_value: None,
            },
            PortDefinition {
                name: "file_extension".to_string(),
                port_type: PortType::Str,
                required: true,
                default_value: None,
            },
        ]
    }

    fn execute(
        &mut self,
        inputs: &HashMap<String, PortData>,
        _ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        let input_path = match inputs.get("path") {
            Some(PortData::Path(path)) => path,
            _ => bail!("PathDivider requires input port 'path' of type Path"),
        };

        let path = Path::new(input_path);

        let parent_path = path.parent().map(PathBuf::from).unwrap_or_default();
        let file_name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let file_stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let file_extension = path
            .extension()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        let mut outputs = HashMap::new();
        outputs.insert("parent_path".to_string(), PortData::Path(parent_path));
        outputs.insert("file_name".to_string(), PortData::Str(file_name));
        outputs.insert("file_stem".to_string(), PortData::Str(file_stem));
        outputs.insert("file_extension".to_string(), PortData::Str(file_extension));
        Ok(outputs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_node(path: &str) -> HashMap<String, PortData> {
        let mut node = PathDividerNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("path".to_string(), PortData::Path(PathBuf::from(path)));
        node.execute(&inputs, &ctx).expect("PathDivider execution")
    }

    fn expect_str(outputs: &HashMap<String, PortData>, key: &str) -> String {
        match outputs.get(key) {
            Some(PortData::Str(v)) => v.clone(),
            _ => panic!("expected string output for key '{key}'"),
        }
    }

    fn expect_path(outputs: &HashMap<String, PortData>, key: &str) -> PathBuf {
        match outputs.get(key) {
            Some(PortData::Path(v)) => v.clone(),
            _ => panic!("expected path output for key '{key}'"),
        }
    }

    #[test]
    fn test_path_divider_contract() {
        let node = PathDividerNode::new();
        assert_eq!(node.node_type(), "PathDivider");

        let inputs = node.input_ports();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].name, "path");
        assert_eq!(inputs[0].port_type, PortType::Path);

        let outputs = node.output_ports();
        assert_eq!(outputs.len(), 4);
        assert_eq!(outputs[0].name, "parent_path");
        assert_eq!(outputs[0].port_type, PortType::Path);
        assert_eq!(outputs[1].name, "file_name");
        assert_eq!(outputs[1].port_type, PortType::Str);
        assert_eq!(outputs[2].name, "file_stem");
        assert_eq!(outputs[2].port_type, PortType::Str);
        assert_eq!(outputs[3].name, "file_extension");
        assert_eq!(outputs[3].port_type, PortType::Str);
    }

    #[test]
    fn test_path_divider_standard_path() {
        let outputs = run_node("anime/episode01.mkv");

        assert_eq!(expect_path(&outputs, "parent_path"), PathBuf::from("anime"));
        assert_eq!(expect_str(&outputs, "file_name"), "episode01.mkv");
        assert_eq!(expect_str(&outputs, "file_stem"), "episode01");
        assert_eq!(expect_str(&outputs, "file_extension"), "mkv");
    }

    #[test]
    fn test_path_divider_hidden_file() {
        let outputs = run_node(".env");

        assert_eq!(expect_path(&outputs, "parent_path"), PathBuf::new());
        assert_eq!(expect_str(&outputs, "file_name"), ".env");
        assert_eq!(expect_str(&outputs, "file_stem"), ".env");
        assert_eq!(expect_str(&outputs, "file_extension"), "");
    }

    #[test]
    fn test_path_divider_no_extension() {
        let outputs = run_node("downloads/readme");

        assert_eq!(
            expect_path(&outputs, "parent_path"),
            PathBuf::from("downloads")
        );
        assert_eq!(expect_str(&outputs, "file_name"), "readme");
        assert_eq!(expect_str(&outputs, "file_stem"), "readme");
        assert_eq!(expect_str(&outputs, "file_extension"), "");
    }

    #[test]
    fn test_path_divider_trailing_slash() {
        let outputs = run_node("downloads/season1/");

        assert_eq!(
            expect_path(&outputs, "parent_path"),
            PathBuf::from("downloads")
        );
        assert_eq!(expect_str(&outputs, "file_name"), "season1");
        assert_eq!(expect_str(&outputs, "file_stem"), "season1");
        assert_eq!(expect_str(&outputs, "file_extension"), "");
    }

    #[test]
    fn test_path_divider_root_path_missing_parts() {
        let outputs = run_node("/");

        assert_eq!(expect_path(&outputs, "parent_path"), PathBuf::new());
        assert_eq!(expect_str(&outputs, "file_name"), "");
        assert_eq!(expect_str(&outputs, "file_stem"), "");
        assert_eq!(expect_str(&outputs, "file_extension"), "");
    }

    #[test]
    fn test_path_divider_missing_required_input_fails_fast() {
        let mut node = PathDividerNode::new();
        let inputs = HashMap::new();

        let err = match node.execute(&inputs, &ExecutionContext::default()) {
            Ok(_) => panic!("missing 'path' should fail"),
            Err(err) => err,
        };
        assert_eq!(
            err.to_string(),
            "PathDivider requires input port 'path' of type Path"
        );
    }

    #[test]
    fn test_path_divider_wrong_input_type_fails_fast() {
        let mut node = PathDividerNode::new();
        let inputs = HashMap::from([("path".to_string(), PortData::Str("/tmp/a.mkv".to_string()))]);

        let err = match node.execute(&inputs, &ExecutionContext::default()) {
            Ok(_) => panic!("non-Path 'path' should fail"),
            Err(err) => err,
        };
        assert_eq!(
            err.to_string(),
            "PathDivider requires input port 'path' of type Path"
        );
    }
}
