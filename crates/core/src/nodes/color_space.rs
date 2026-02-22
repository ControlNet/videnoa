//! ColorSpace node: stores zscale conversion config and passes frames through unchanged.
//!
//! The actual color space conversion happens in VideoOutput's zscale filter.
//! This node captures the desired settings and exposes them as a JSON config string.

use std::collections::HashMap;

use anyhow::Result;

use crate::node::{ExecutionContext, FrameProcessor, Node, PortDefinition};
use crate::types::{Frame, PortData, PortType};

#[derive(Debug, Clone)]
pub struct ColorSpaceConfig {
    pub matrix: String,
    pub range: String,
    pub transfer: String,
    pub primaries: String,
    pub dither: String,
}

impl Default for ColorSpaceConfig {
    fn default() -> Self {
        Self {
            matrix: "bt709".to_string(),
            range: "limited".to_string(),
            transfer: "bt709".to_string(),
            primaries: "bt709".to_string(),
            dither: "error_diffusion".to_string(),
        }
    }
}

impl ColorSpaceConfig {
    /// Serialize to JSON string for downstream consumption by VideoOutput.
    pub fn to_json(&self) -> String {
        serde_json::json!({
            "matrix": self.matrix,
            "range": self.range,
            "transfer": self.transfer,
            "primaries": self.primaries,
            "dither": self.dither,
        })
        .to_string()
    }

    /// Build the zscale filter string for FFmpeg.
    pub fn to_zscale_filter(&self) -> String {
        format!(
            "zscale=matrix={}:range={}:transfer={}:primaries={}:dither={}",
            self.matrix, self.range, self.transfer, self.primaries, self.dither
        )
    }
}

pub struct ColorSpaceNode {
    config: ColorSpaceConfig,
}

impl ColorSpaceNode {
    pub fn new() -> Self {
        Self {
            config: ColorSpaceConfig::default(),
        }
    }

    pub fn config(&self) -> &ColorSpaceConfig {
        &self.config
    }
}

impl Default for ColorSpaceNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for ColorSpaceNode {
    fn node_type(&self) -> &str {
        "ColorSpace"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![
            PortDefinition {
                name: "matrix".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("bt709")),
            },
            PortDefinition {
                name: "range".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("limited")),
            },
            PortDefinition {
                name: "transfer".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("bt709")),
            },
            PortDefinition {
                name: "primaries".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("bt709")),
            },
            PortDefinition {
                name: "dither".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("error_diffusion")),
            },
        ]
    }

    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![PortDefinition {
            name: "config".to_string(),
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
        if let Some(PortData::Str(v)) = inputs.get("matrix") {
            self.config.matrix = v.clone();
        }
        if let Some(PortData::Str(v)) = inputs.get("range") {
            self.config.range = v.clone();
        }
        if let Some(PortData::Str(v)) = inputs.get("transfer") {
            self.config.transfer = v.clone();
        }
        if let Some(PortData::Str(v)) = inputs.get("primaries") {
            self.config.primaries = v.clone();
        }
        if let Some(PortData::Str(v)) = inputs.get("dither") {
            self.config.dither = v.clone();
        }

        let mut outputs = HashMap::new();
        outputs.insert("config".to_string(), PortData::Str(self.config.to_json()));
        Ok(outputs)
    }
}

impl FrameProcessor for ColorSpaceNode {
    fn process_frame(&mut self, frame: Frame, _ctx: &ExecutionContext) -> Result<Frame> {
        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_space_node_ports() {
        let node = ColorSpaceNode::new();
        assert_eq!(node.node_type(), "ColorSpace");

        let inputs = node.input_ports();
        assert_eq!(inputs.len(), 5);
        assert_eq!(inputs[0].name, "matrix");
        assert_eq!(inputs[1].name, "range");
        assert_eq!(inputs[2].name, "transfer");
        assert_eq!(inputs[3].name, "primaries");
        assert_eq!(inputs[4].name, "dither");
        for port in &inputs {
            assert_eq!(port.port_type, PortType::Str);
            assert!(!port.required);
        }

        let outputs = node.output_ports();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].name, "config");
        assert_eq!(outputs[0].port_type, PortType::Str);
    }

    #[test]
    fn test_color_space_default_config() {
        let mut node = ColorSpaceNode::new();
        let ctx = ExecutionContext::default();
        let inputs = HashMap::new();
        let outputs = node.execute(&inputs, &ctx).unwrap();

        let config_json = match outputs.get("config") {
            Some(PortData::Str(s)) => s.clone(),
            _ => panic!("Expected Str output for config"),
        };

        let parsed: serde_json::Value = serde_json::from_str(&config_json).unwrap();
        assert_eq!(parsed["matrix"], "bt709");
        assert_eq!(parsed["range"], "limited");
        assert_eq!(parsed["transfer"], "bt709");
        assert_eq!(parsed["primaries"], "bt709");
        assert_eq!(parsed["dither"], "error_diffusion");
    }

    #[test]
    fn test_color_space_custom_config() {
        let mut node = ColorSpaceNode::new();
        let ctx = ExecutionContext::default();

        let mut inputs = HashMap::new();
        inputs.insert("matrix".to_string(), PortData::Str("bt601".to_string()));
        inputs.insert("range".to_string(), PortData::Str("full".to_string()));
        inputs.insert("transfer".to_string(), PortData::Str("srgb".to_string()));
        inputs.insert(
            "primaries".to_string(),
            PortData::Str("bt601-525".to_string()),
        );
        inputs.insert("dither".to_string(), PortData::Str("none".to_string()));

        let outputs = node.execute(&inputs, &ctx).unwrap();
        let config_json = match outputs.get("config") {
            Some(PortData::Str(s)) => s.clone(),
            _ => panic!("Expected Str output"),
        };

        let parsed: serde_json::Value = serde_json::from_str(&config_json).unwrap();
        assert_eq!(parsed["matrix"], "bt601");
        assert_eq!(parsed["range"], "full");
        assert_eq!(parsed["transfer"], "srgb");
        assert_eq!(parsed["primaries"], "bt601-525");
        assert_eq!(parsed["dither"], "none");
    }

    #[test]
    fn test_color_space_passthrough_frame() {
        let mut node = ColorSpaceNode::new();
        let ctx = ExecutionContext::default();

        let original_data = vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120];
        let frame = Frame::CpuRgb {
            data: original_data.clone(),
            width: 2,
            height: 2,
            bit_depth: 8,
        };

        let result = node.process_frame(frame, &ctx).unwrap();
        match result {
            Frame::CpuRgb {
                data,
                width,
                height,
                bit_depth,
            } => {
                assert_eq!(data, original_data);
                assert_eq!(width, 2);
                assert_eq!(height, 2);
                assert_eq!(bit_depth, 8);
            }
            _ => panic!("Expected CpuRgb frame"),
        }
    }

    #[test]
    fn test_color_space_zscale_filter() {
        let config = ColorSpaceConfig::default();
        assert_eq!(
            config.to_zscale_filter(),
            "zscale=matrix=bt709:range=limited:transfer=bt709:primaries=bt709:dither=error_diffusion"
        );
    }

    #[test]
    fn test_color_space_config_accessor() {
        let node = ColorSpaceNode::new();
        let config = node.config();
        assert_eq!(config.matrix, "bt709");
        assert_eq!(config.range, "limited");
    }

    #[test]
    fn test_color_space_partial_override() {
        let mut node = ColorSpaceNode::new();
        let ctx = ExecutionContext::default();

        let mut inputs = HashMap::new();
        inputs.insert("matrix".to_string(), PortData::Str("bt601".to_string()));

        let outputs = node.execute(&inputs, &ctx).unwrap();
        let config_json = match outputs.get("config") {
            Some(PortData::Str(s)) => s.clone(),
            _ => panic!("Expected Str output"),
        };

        let parsed: serde_json::Value = serde_json::from_str(&config_json).unwrap();
        assert_eq!(parsed["matrix"], "bt601");
        assert_eq!(parsed["range"], "limited");
        assert_eq!(parsed["transfer"], "bt709");
    }
}
