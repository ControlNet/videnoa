use std::collections::HashMap;

use anyhow::{anyhow, Result};

use crate::node::Node;
use crate::nodes::rescale::RescaleNode;

type NodeFactory =
    dyn Fn(HashMap<String, serde_json::Value>) -> Result<Box<dyn Node>> + Send + Sync;

pub struct NodeRegistry {
    factories: HashMap<String, Box<NodeFactory>>,
}

impl NodeRegistry {
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    pub fn register<F>(&mut self, node_type: &str, factory: F)
    where
        F: Fn(HashMap<String, serde_json::Value>) -> Result<Box<dyn Node>> + Send + Sync + 'static,
    {
        self.factories
            .insert(node_type.to_string(), Box::new(factory));
    }

    pub fn create(
        &self,
        node_type: &str,
        params: HashMap<String, serde_json::Value>,
    ) -> Result<Box<dyn Node>> {
        let factory = self
            .factories
            .get(node_type)
            .ok_or_else(|| anyhow!("unknown node type: {node_type}"))?;

        factory(params)
    }

    pub fn list_node_types(&self) -> Vec<&str> {
        let mut node_types: Vec<&str> = self.factories.keys().map(|v| v.as_str()).collect();
        node_types.sort_unstable();
        node_types
    }
}

impl Default for NodeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub fn register_rescale_node(registry: &mut NodeRegistry) {
    registry.register("Rescale", |_params| Ok(Box::new(RescaleNode::new())));
}

/// Register all real node types from the `videnoa-core` crate.
///
/// The keys match the frontend `NodeTypeName` values so that workflow JSON
/// round-trips cleanly between UI and backend.
pub fn register_all_nodes(registry: &mut NodeRegistry) {
    use crate::nodes::color_space::ColorSpaceNode;
    use crate::nodes::constant::ConstantNode;
    use crate::nodes::downloader::DownloaderNode;
    use crate::nodes::frame_interpolation::FrameInterpolationNode;
    use crate::nodes::http_request::HttpRequestNode;
    use crate::nodes::jellyfin_video::JellyfinVideoNode;
    use crate::nodes::path_divider::PathDividerNode;
    use crate::nodes::path_joiner::PathJoinerNode;
    use crate::nodes::print::PrintNode;
    use crate::nodes::resize::ResizeNode;
    use crate::nodes::scene_detect::SceneDetectNode;
    use crate::nodes::stream_output::StreamOutputNode;
    use crate::nodes::string_replace::StringReplaceNode;
    use crate::nodes::string_template::StringTemplateNode;
    use crate::nodes::super_res::SuperResNode;
    use crate::nodes::type_conversion::TypeConversionNode;
    use crate::nodes::video_input::VideoInputNode;
    use crate::nodes::video_output::VideoOutputNode;
    use crate::nodes::workflow_io::{WorkflowInputNode, WorkflowNode, WorkflowOutputNode};

    registry.register("VideoInput", |params| {
        Ok(Box::new(VideoInputNode::new(&params)?))
    });
    registry.register("JellyfinVideo", |params| {
        Ok(Box::new(JellyfinVideoNode::new(&params)?))
    });
    registry.register("SuperResolution", |_params| {
        Ok(Box::new(SuperResNode::new()))
    });
    registry.register("FrameInterpolation", |_params| {
        Ok(Box::new(FrameInterpolationNode::new()))
    });
    registry.register("VideoOutput", |_params| {
        Ok(Box::new(VideoOutputNode::new()))
    });
    registry.register("Downloader", |_params| Ok(Box::new(DownloaderNode::new())));
    registry.register("PathDivider", |_params| {
        Ok(Box::new(PathDividerNode::new()))
    });
    registry.register("PathJoiner", |_params| Ok(Box::new(PathJoinerNode::new())));
    registry.register("Print", |_params| Ok(Box::new(PrintNode::new())));
    registry.register("StringTemplate", |params| {
        Ok(Box::new(StringTemplateNode::from_params(&params)))
    });
    registry.register("StringReplace", |_params| {
        Ok(Box::new(StringReplaceNode::new()))
    });
    registry.register("TypeConversion", |params| {
        Ok(Box::new(TypeConversionNode::from_params(&params)?))
    });
    registry.register("HttpRequest", |_params| {
        Ok(Box::new(HttpRequestNode::new()))
    });
    registry.register("StreamOutput", |_params| {
        Ok(Box::new(StreamOutputNode::new()))
    });
    registry.register("Resize", |_params| Ok(Box::new(ResizeNode::new())));
    register_rescale_node(registry);
    registry.register("ColorSpace", |_params| Ok(Box::new(ColorSpaceNode::new())));
    registry.register("SceneDetect", |_params| {
        Ok(Box::new(SceneDetectNode::new()))
    });
    registry.register("Constant", |params| {
        Ok(Box::new(ConstantNode::from_params(&params)?))
    });
    registry.register("WorkflowInput", |params| {
        Ok(Box::new(WorkflowInputNode::from_params(&params)))
    });
    registry.register("WorkflowOutput", |params| {
        Ok(Box::new(WorkflowOutputNode::from_params(&params)))
    });
    registry.register("Workflow", |params| {
        Ok(Box::new(WorkflowNode::from_params(&params)))
    });
}

pub fn build_default_registry() -> NodeRegistry {
    let mut registry = NodeRegistry::new();
    register_all_nodes(&mut registry);
    registry
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{ExecutionContext, PortDefinition};
    use crate::types::{PortData, PortType};

    struct DummyNode;

    impl Node for DummyNode {
        fn node_type(&self) -> &str {
            "dummy"
        }

        fn input_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "in".to_string(),
                port_type: PortType::Str,
                required: true,
                default_value: None,
            }]
        }

        fn output_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "out".to_string(),
                port_type: PortType::Str,
                required: true,
                default_value: None,
            }]
        }

        fn execute(
            &mut self,
            _inputs: &HashMap<String, PortData>,
            _ctx: &ExecutionContext,
        ) -> Result<HashMap<String, PortData>> {
            Ok(HashMap::new())
        }
    }

    #[test]
    fn test_node_registry_register_and_create() {
        let mut registry = NodeRegistry::new();
        registry.register("dummy", |_| Ok(Box::new(DummyNode)));

        let node = registry
            .create("dummy", HashMap::new())
            .expect("dummy node should be created");

        assert_eq!(node.node_type(), "dummy");
        assert_eq!(node.input_ports().len(), 1);
        assert_eq!(node.output_ports().len(), 1);
        assert_eq!(registry.list_node_types(), vec!["dummy"]);
    }

    #[test]
    fn test_node_registry_unknown_type_errors() {
        let registry = NodeRegistry::new();

        for node_type in ["unknown", "StreamInput", "JellyfinInput"] {
            let err = match registry.create(node_type, HashMap::new()) {
                Ok(_) => panic!("unknown node type should error"),
                Err(err) => err,
            };

            assert_eq!(err.to_string(), format!("unknown node type: {node_type}"));
        }
    }

    #[test]
    fn test_register_rescale_node() {
        let mut registry = NodeRegistry::new();
        register_rescale_node(&mut registry);

        let node = registry
            .create("Rescale", HashMap::new())
            .expect("rescale node should be created");

        assert_eq!(node.node_type(), "Rescale");
    }

    #[test]
    fn test_register_all_nodes_expected_set() {
        let mut registry = NodeRegistry::new();
        register_all_nodes(&mut registry);

        let expected = vec![
            "ColorSpace",
            "Constant",
            "Downloader",
            "FrameInterpolation",
            "HttpRequest",
            "JellyfinVideo",
            "PathDivider",
            "PathJoiner",
            "Print",
            "Rescale",
            "Resize",
            "SceneDetect",
            "StreamOutput",
            "StringReplace",
            "StringTemplate",
            "SuperResolution",
            "TypeConversion",
            "VideoInput",
            "VideoOutput",
            "Workflow",
            "WorkflowInput",
            "WorkflowOutput",
        ];

        assert_eq!(registry.list_node_types(), expected);
    }

    #[test]
    fn test_register_all_nodes_rejects_legacy_jellyfin_and_stream_input_types() {
        let mut registry = NodeRegistry::new();
        register_all_nodes(&mut registry);

        for node_type in ["StreamInput", "JellyfinInput"] {
            let err = match registry.create(node_type, HashMap::new()) {
                Ok(_) => panic!("legacy aliases must stay rejected"),
                Err(err) => err,
            };
            assert_eq!(err.to_string(), format!("unknown node type: {node_type}"));
        }
    }

    #[test]
    fn test_constant_factory_applies_params_type_for_validation_time_ports() {
        let mut registry = NodeRegistry::new();
        register_all_nodes(&mut registry);

        let params = HashMap::from([("type".to_string(), serde_json::json!("Str"))]);
        let node = registry
            .create("Constant", params)
            .expect("constant should be created from params");

        assert_eq!(node.output_ports()[0].port_type, PortType::Str);
    }

    #[test]
    fn test_constant_factory_rejects_invalid_type_param_value() {
        let mut registry = NodeRegistry::new();
        register_all_nodes(&mut registry);

        let params = HashMap::from([("type".to_string(), serde_json::json!("VideoFrames"))]);
        let err = match registry.create("Constant", params) {
            Ok(_) => panic!("invalid constant params.type should fail deterministically"),
            Err(err) => err,
        };

        assert_eq!(
            err.to_string(),
            "Constant: unsupported type 'VideoFrames', expected one of Int|Float|Str|Bool|Path"
        );
    }
}
