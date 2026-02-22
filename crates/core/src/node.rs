use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use anyhow::Result;

use crate::types::{Frame, PortData, PortType};

#[derive(Debug, Clone, PartialEq)]
pub struct PortDefinition {
    pub name: String,
    pub port_type: PortType,
    pub required: bool,
    pub default_value: Option<serde_json::Value>,
}

#[derive(Default)]
pub struct ExecutionContext {
    pub total_frames: Option<u64>,
    pub current_frame: u64,
    pub executing_workflows: HashSet<PathBuf>,
    pub nesting_depth: u32,
}

impl ExecutionContext {
    pub fn progress(&self) -> Option<f32> {
        let total = self.total_frames?;
        if total == 0 {
            return Some(0.0);
        }

        Some((self.current_frame as f32 / total as f32).clamp(0.0, 1.0))
    }
}

/// Core node trait that all nodes implement.
pub trait Node: Send + Sync {
    fn node_type(&self) -> &str;
    fn input_ports(&self) -> Vec<PortDefinition>;
    fn output_ports(&self) -> Vec<PortDefinition>;
    fn execute(
        &mut self,
        inputs: &HashMap<String, PortData>,
        ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>>;
}

/// Sub-trait for nodes that process frames one-at-a-time.
pub trait FrameProcessor: Node {
    fn process_frame(&mut self, frame: Frame, ctx: &ExecutionContext) -> Result<Frame>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_definition_creation() {
        let input = PortDefinition {
            name: "input".to_string(),
            port_type: PortType::VideoFrames,
            required: true,
            default_value: None,
        };

        let output = PortDefinition {
            name: "strength".to_string(),
            port_type: PortType::Float,
            required: false,
            default_value: Some(serde_json::json!(1.0)),
        };

        assert_eq!(input.name, "input");
        assert_eq!(input.port_type, PortType::VideoFrames);
        assert!(input.required);
        assert!(input.default_value.is_none());

        assert_eq!(output.name, "strength");
        assert_eq!(output.port_type, PortType::Float);
        assert!(!output.required);
        assert_eq!(output.default_value, Some(serde_json::json!(1.0)));
    }
}
