use std::collections::HashMap;

use anyhow::{bail, Result};

use crate::node::{ExecutionContext, FrameProcessor, Node, PortDefinition};
use crate::types::{Frame, PortData, PortType};

use super::resize::ResizeAlgorithm;

pub struct RescaleNode {
    scale_factor: f64,
    algorithm: ResizeAlgorithm,
}

impl RescaleNode {
    pub fn new() -> Self {
        Self {
            scale_factor: 0.0,
            algorithm: ResizeAlgorithm::Bilinear,
        }
    }
}

impl Default for RescaleNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for RescaleNode {
    fn node_type(&self) -> &str {
        "Rescale"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![
            PortDefinition {
                name: "scale_factor".to_string(),
                port_type: PortType::Float,
                required: true,
                default_value: None,
            },
            PortDefinition {
                name: "algorithm".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("bilinear")),
            },
        ]
    }

    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![]
    }

    fn execute(
        &mut self,
        inputs: &HashMap<String, PortData>,
        _ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        match inputs.get("scale_factor") {
            Some(PortData::Float(scale_factor)) => {
                if *scale_factor <= 0.0 {
                    bail!("scale_factor must be positive, got {scale_factor}");
                }
                self.scale_factor = *scale_factor;
            }
            Some(_) => bail!("scale_factor must be a Float"),
            None => bail!("scale_factor is required"),
        }

        if let Some(PortData::Str(algo)) = inputs.get("algorithm") {
            self.algorithm = ResizeAlgorithm::from_str_lossy(algo);
        }

        Ok(HashMap::new())
    }
}

impl FrameProcessor for RescaleNode {
    fn process_frame(&mut self, frame: Frame, _ctx: &ExecutionContext) -> Result<Frame> {
        if self.scale_factor <= 0.0 {
            bail!("Rescale scale_factor not configured — call execute() first");
        }

        match frame {
            Frame::CpuRgb {
                ref data,
                width: in_w,
                height: in_h,
                bit_depth,
            } => {
                if bit_depth != 8 {
                    bail!("RescaleNode only supports 8-bit RGB frames, got {bit_depth}-bit");
                }

                let expected_len = in_w as usize * in_h as usize * 3;
                if data.len() != expected_len {
                    bail!(
                        "Frame data length mismatch: expected {expected_len}, got {}",
                        data.len()
                    );
                }

                let out_width = (in_w as f64 * self.scale_factor).round() as u32;
                let out_height = (in_h as f64 * self.scale_factor).round() as u32;
                if out_width == 0 || out_height == 0 {
                    bail!(
                        "scaled dimensions must be positive, got {out_width}x{out_height} from {in_w}x{in_h}"
                    );
                }

                let out_data = match self.algorithm {
                    ResizeAlgorithm::Bilinear => super::resize::resize_bilinear(
                        data,
                        in_w as usize,
                        in_h as usize,
                        out_width as usize,
                        out_height as usize,
                    ),
                    ResizeAlgorithm::Nearest => super::resize::resize_nearest(
                        data,
                        in_w as usize,
                        in_h as usize,
                        out_width as usize,
                        out_height as usize,
                    ),
                };

                Ok(Frame::CpuRgb {
                    data: out_data,
                    width: out_width,
                    height: out_height,
                    bit_depth: 8,
                })
            }
            _ => bail!("RescaleNode only supports Frame::CpuRgb input"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_solid_frame(w: u32, h: u32, r: u8, g: u8, b: u8) -> Frame {
        let mut data = vec![0u8; w as usize * h as usize * 3];
        for pixel in data.chunks_exact_mut(3) {
            pixel[0] = r;
            pixel[1] = g;
            pixel[2] = b;
        }
        Frame::CpuRgb {
            data,
            width: w,
            height: h,
            bit_depth: 8,
        }
    }

    #[test]
    fn test_rescale_node_ports() {
        let node = RescaleNode::new();
        assert_eq!(node.node_type(), "Rescale");

        let inputs = node.input_ports();
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].name, "scale_factor");
        assert_eq!(inputs[0].port_type, PortType::Float);
        assert!(inputs[0].required);
        assert_eq!(inputs[1].name, "algorithm");
        assert_eq!(inputs[1].port_type, PortType::Str);
        assert!(!inputs[1].required);

        let outputs = node.output_ports();
        assert!(outputs.is_empty());
    }

    #[test]
    fn test_rescale_execute_missing_scale_factor() {
        let mut node = RescaleNode::new();
        let ctx = ExecutionContext::default();
        let inputs = HashMap::new();
        let result = node.execute(&inputs, &ctx);
        let err = result.err().expect("should fail");
        assert!(err.to_string().contains("scale_factor is required"));
    }

    #[test]
    fn test_rescale_execute_invalid_scale_zero() {
        let mut node = RescaleNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("scale_factor".to_string(), PortData::Float(0.0));
        let result = node.execute(&inputs, &ctx);
        let err = result.err().expect("should fail");
        assert!(err.to_string().contains("scale_factor must be positive"));
    }

    #[test]
    fn test_rescale_execute_invalid_scale_negative() {
        let mut node = RescaleNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("scale_factor".to_string(), PortData::Float(-0.5));
        let result = node.execute(&inputs, &ctx);
        let err = result.err().expect("should fail");
        assert!(err.to_string().contains("scale_factor must be positive"));
    }

    #[test]
    fn test_rescale_process_frame_2x_dimensions() {
        let mut node = RescaleNode::new();
        let ctx = ExecutionContext::default();

        let mut inputs = HashMap::new();
        inputs.insert("scale_factor".to_string(), PortData::Float(2.0));
        inputs.insert(
            "algorithm".to_string(),
            PortData::Str("nearest".to_string()),
        );
        node.execute(&inputs, &ctx).unwrap();

        let frame = make_solid_frame(4, 4, 10, 20, 30);
        let result = node.process_frame(frame, &ctx).unwrap();

        match result {
            Frame::CpuRgb {
                data,
                width,
                height,
                bit_depth,
            } => {
                assert_eq!(width, 8);
                assert_eq!(height, 8);
                assert_eq!(bit_depth, 8);
                assert_eq!(data.len(), 8 * 8 * 3);
                for pixel in data.chunks_exact(3) {
                    assert_eq!(pixel[0], 10);
                    assert_eq!(pixel[1], 20);
                    assert_eq!(pixel[2], 30);
                }
            }
            _ => panic!("Expected CpuRgb frame"),
        }
    }

    #[test]
    fn test_rescale_process_frame_0_5x_dimensions() {
        let mut node = RescaleNode::new();
        let ctx = ExecutionContext::default();

        let mut inputs = HashMap::new();
        inputs.insert("scale_factor".to_string(), PortData::Float(0.5));
        inputs.insert(
            "algorithm".to_string(),
            PortData::Str("nearest".to_string()),
        );
        node.execute(&inputs, &ctx).unwrap();

        let frame = make_solid_frame(8, 8, 200, 100, 50);
        let result = node.process_frame(frame, &ctx).unwrap();

        match result {
            Frame::CpuRgb {
                data,
                width,
                height,
                ..
            } => {
                assert_eq!(width, 4);
                assert_eq!(height, 4);
                assert_eq!(data.len(), 4 * 4 * 3);
                for pixel in data.chunks_exact(3) {
                    assert_eq!(pixel[0], 200);
                    assert_eq!(pixel[1], 100);
                    assert_eq!(pixel[2], 50);
                }
            }
            _ => panic!("Expected CpuRgb frame"),
        }
    }

    #[test]
    fn test_rescale_process_frame_identity_1x() {
        let mut node = RescaleNode::new();
        let ctx = ExecutionContext::default();

        let mut inputs = HashMap::new();
        inputs.insert("scale_factor".to_string(), PortData::Float(1.0));
        inputs.insert(
            "algorithm".to_string(),
            PortData::Str("nearest".to_string()),
        );
        node.execute(&inputs, &ctx).unwrap();

        let mut src_data = vec![0u8; 4 * 4 * 3];
        for (i, byte) in src_data.iter_mut().enumerate() {
            *byte = (i * 7 % 256) as u8;
        }

        let frame = Frame::CpuRgb {
            data: src_data.clone(),
            width: 4,
            height: 4,
            bit_depth: 8,
        };

        let result = node.process_frame(frame, &ctx).unwrap();
        match result {
            Frame::CpuRgb {
                data,
                width,
                height,
                ..
            } => {
                assert_eq!(width, 4);
                assert_eq!(height, 4);
                assert_eq!(data, src_data);
            }
            _ => panic!("Expected CpuRgb frame"),
        }
    }
}
