//! Resize node: pure-Rust bilinear/nearest-neighbor frame resizing.

use std::collections::HashMap;

use anyhow::{bail, Result};

use crate::node::{ExecutionContext, FrameProcessor, Node, PortDefinition};
use crate::types::{Frame, PortData, PortType};

/// Supported resize algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeAlgorithm {
    Bilinear,
    Nearest,
}

impl ResizeAlgorithm {
    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "nearest" | "neighbor" | "nn" => Self::Nearest,
            _ => Self::Bilinear,
        }
    }
}

pub struct ResizeNode {
    out_width: u32,
    out_height: u32,
    algorithm: ResizeAlgorithm,
}

impl ResizeNode {
    pub fn new() -> Self {
        Self {
            out_width: 0,
            out_height: 0,
            algorithm: ResizeAlgorithm::Bilinear,
        }
    }
}

impl Default for ResizeNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for ResizeNode {
    fn node_type(&self) -> &str {
        "Resize"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![
            PortDefinition {
                name: "width".to_string(),
                port_type: PortType::Int,
                required: true,
                default_value: None,
            },
            PortDefinition {
                name: "height".to_string(),
                port_type: PortType::Int,
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
        match inputs.get("width") {
            Some(PortData::Int(w)) => {
                if *w <= 0 {
                    bail!("width must be positive, got {w}");
                }
                self.out_width = *w as u32;
            }
            Some(_) => bail!("width must be an Int"),
            None => bail!("width is required"),
        }

        match inputs.get("height") {
            Some(PortData::Int(h)) => {
                if *h <= 0 {
                    bail!("height must be positive, got {h}");
                }
                self.out_height = *h as u32;
            }
            Some(_) => bail!("height must be an Int"),
            None => bail!("height is required"),
        }

        if let Some(PortData::Str(algo)) = inputs.get("algorithm") {
            self.algorithm = ResizeAlgorithm::from_str_lossy(algo);
        }

        Ok(HashMap::new())
    }
}

impl FrameProcessor for ResizeNode {
    fn process_frame(&mut self, frame: Frame, _ctx: &ExecutionContext) -> Result<Frame> {
        if self.out_width == 0 || self.out_height == 0 {
            bail!("Resize dimensions not configured — call execute() first");
        }

        match frame {
            Frame::CpuRgb {
                ref data,
                width: in_w,
                height: in_h,
                bit_depth,
            } => {
                if bit_depth != 8 {
                    bail!("ResizeNode only supports 8-bit RGB frames, got {bit_depth}-bit");
                }

                let expected_len = in_w as usize * in_h as usize * 3;
                if data.len() != expected_len {
                    bail!(
                        "Frame data length mismatch: expected {expected_len}, got {}",
                        data.len()
                    );
                }

                let out_data = match self.algorithm {
                    ResizeAlgorithm::Bilinear => resize_bilinear(
                        data,
                        in_w as usize,
                        in_h as usize,
                        self.out_width as usize,
                        self.out_height as usize,
                    ),
                    ResizeAlgorithm::Nearest => resize_nearest(
                        data,
                        in_w as usize,
                        in_h as usize,
                        self.out_width as usize,
                        self.out_height as usize,
                    ),
                };

                Ok(Frame::CpuRgb {
                    data: out_data,
                    width: self.out_width,
                    height: self.out_height,
                    bit_depth: 8,
                })
            }
            _ => bail!("ResizeNode only supports Frame::CpuRgb input"),
        }
    }
}

/// Nearest-neighbor resize for 8-bit RGB24 data.
pub(crate) fn resize_nearest(
    src: &[u8],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
) -> Vec<u8> {
    let mut dst = vec![0u8; dst_w * dst_h * 3];

    for dst_y in 0..dst_h {
        let src_y = ((dst_y as f64 + 0.5) * src_h as f64 / dst_h as f64) as usize;
        let src_y = src_y.min(src_h - 1);

        for dst_x in 0..dst_w {
            let src_x = ((dst_x as f64 + 0.5) * src_w as f64 / dst_w as f64) as usize;
            let src_x = src_x.min(src_w - 1);

            let si = (src_y * src_w + src_x) * 3;
            let di = (dst_y * dst_w + dst_x) * 3;
            dst[di] = src[si];
            dst[di + 1] = src[si + 1];
            dst[di + 2] = src[si + 2];
        }
    }

    dst
}

/// Bilinear interpolation resize for 8-bit RGB24 data.
pub(crate) fn resize_bilinear(
    src: &[u8],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
) -> Vec<u8> {
    let mut dst = vec![0u8; dst_w * dst_h * 3];

    for dst_y in 0..dst_h {
        // Map destination pixel center to source coordinates
        let src_yf = (dst_y as f64 + 0.5) * src_h as f64 / dst_h as f64 - 0.5;
        let src_y0 = src_yf.floor().max(0.0) as usize;
        let src_y1 = (src_y0 + 1).min(src_h - 1);
        let fy = (src_yf - src_y0 as f64).clamp(0.0, 1.0);

        for dst_x in 0..dst_w {
            let src_xf = (dst_x as f64 + 0.5) * src_w as f64 / dst_w as f64 - 0.5;
            let src_x0 = src_xf.floor().max(0.0) as usize;
            let src_x1 = (src_x0 + 1).min(src_w - 1);
            let fx = (src_xf - src_x0 as f64).clamp(0.0, 1.0);

            let di = (dst_y * dst_w + dst_x) * 3;

            for c in 0..3 {
                let p00 = src[(src_y0 * src_w + src_x0) * 3 + c] as f64;
                let p10 = src[(src_y0 * src_w + src_x1) * 3 + c] as f64;
                let p01 = src[(src_y1 * src_w + src_x0) * 3 + c] as f64;
                let p11 = src[(src_y1 * src_w + src_x1) * 3 + c] as f64;

                let top = p00 * (1.0 - fx) + p10 * fx;
                let bot = p01 * (1.0 - fx) + p11 * fx;
                let val = top * (1.0 - fy) + bot * fy;

                dst[di + c] = val.round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    dst
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a solid-color test frame.
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
    fn test_resize_node_ports() {
        let node = ResizeNode::new();
        assert_eq!(node.node_type(), "Resize");

        let inputs = node.input_ports();
        assert_eq!(inputs.len(), 3);
        assert_eq!(inputs[0].name, "width");
        assert_eq!(inputs[0].port_type, PortType::Int);
        assert!(inputs[0].required);
        assert_eq!(inputs[1].name, "height");
        assert_eq!(inputs[1].port_type, PortType::Int);
        assert!(inputs[1].required);
        assert_eq!(inputs[2].name, "algorithm");
        assert_eq!(inputs[2].port_type, PortType::Str);
        assert!(!inputs[2].required);

        let outputs = node.output_ports();
        assert!(outputs.is_empty());
    }

    #[test]
    fn test_resize_execute_missing_width() {
        let mut node = ResizeNode::new();
        let ctx = ExecutionContext::default();
        let inputs = HashMap::new();
        let result = node.execute(&inputs, &ctx);
        let err = result.err().expect("should fail");
        assert!(err.to_string().contains("width is required"));
    }

    #[test]
    fn test_resize_execute_missing_height() {
        let mut node = ResizeNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("width".to_string(), PortData::Int(640));
        let result = node.execute(&inputs, &ctx);
        let err = result.err().expect("should fail");
        assert!(err.to_string().contains("height is required"));
    }

    #[test]
    fn test_resize_execute_negative_width() {
        let mut node = ResizeNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("width".to_string(), PortData::Int(-1));
        inputs.insert("height".to_string(), PortData::Int(480));
        let result = node.execute(&inputs, &ctx);
        let err = result.err().expect("should fail");
        assert!(err.to_string().contains("width must be positive"));
    }

    #[test]
    fn test_resize_process_frame_without_execute() {
        let mut node = ResizeNode::new();
        let ctx = ExecutionContext::default();
        let frame = make_solid_frame(4, 4, 128, 128, 128);
        let result = node.process_frame(frame, &ctx);
        let err = result.err().expect("should fail");
        assert!(err.to_string().contains("not configured"));
    }

    #[test]
    fn test_resize_nearest_solid_color() {
        let mut node = ResizeNode::new();
        let ctx = ExecutionContext::default();

        let mut inputs = HashMap::new();
        inputs.insert("width".to_string(), PortData::Int(8));
        inputs.insert("height".to_string(), PortData::Int(8));
        inputs.insert(
            "algorithm".to_string(),
            PortData::Str("nearest".to_string()),
        );
        node.execute(&inputs, &ctx).unwrap();

        let frame = make_solid_frame(4, 4, 200, 100, 50);
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
                    assert_eq!(pixel[0], 200);
                    assert_eq!(pixel[1], 100);
                    assert_eq!(pixel[2], 50);
                }
            }
            _ => panic!("Expected CpuRgb frame"),
        }
    }

    #[test]
    fn test_resize_bilinear_solid_color() {
        let mut node = ResizeNode::new();
        let ctx = ExecutionContext::default();

        let mut inputs = HashMap::new();
        inputs.insert("width".to_string(), PortData::Int(8));
        inputs.insert("height".to_string(), PortData::Int(8));
        inputs.insert(
            "algorithm".to_string(),
            PortData::Str("bilinear".to_string()),
        );
        node.execute(&inputs, &ctx).unwrap();

        let frame = make_solid_frame(4, 4, 200, 100, 50);
        let result = node.process_frame(frame, &ctx).unwrap();

        match result {
            Frame::CpuRgb {
                data,
                width,
                height,
                ..
            } => {
                assert_eq!(width, 8);
                assert_eq!(height, 8);
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
    fn test_resize_downscale_dimensions() {
        let mut node = ResizeNode::new();
        let ctx = ExecutionContext::default();

        let mut inputs = HashMap::new();
        inputs.insert("width".to_string(), PortData::Int(2));
        inputs.insert("height".to_string(), PortData::Int(2));
        node.execute(&inputs, &ctx).unwrap();

        let frame = make_solid_frame(8, 8, 128, 64, 32);
        let result = node.process_frame(frame, &ctx).unwrap();

        match result {
            Frame::CpuRgb {
                width,
                height,
                data,
                ..
            } => {
                assert_eq!(width, 2);
                assert_eq!(height, 2);
                assert_eq!(data.len(), 2 * 2 * 3);
            }
            _ => panic!("Expected CpuRgb frame"),
        }
    }

    #[test]
    fn test_resize_identity() {
        let mut node = ResizeNode::new();
        let ctx = ExecutionContext::default();

        let mut inputs = HashMap::new();
        inputs.insert("width".to_string(), PortData::Int(4));
        inputs.insert("height".to_string(), PortData::Int(4));
        inputs.insert(
            "algorithm".to_string(),
            PortData::Str("nearest".to_string()),
        );
        node.execute(&inputs, &ctx).unwrap();

        let mut src_data = vec![0u8; 4 * 4 * 3];
        for (i, byte) in src_data.iter_mut().enumerate() {
            *byte = (i * 5 % 256) as u8;
        }
        let frame = Frame::CpuRgb {
            data: src_data.clone(),
            width: 4,
            height: 4,
            bit_depth: 8,
        };

        let result = node.process_frame(frame, &ctx).unwrap();
        match result {
            Frame::CpuRgb { data, .. } => {
                assert_eq!(data, src_data);
            }
            _ => panic!("Expected CpuRgb frame"),
        }
    }

    #[test]
    fn test_resize_algorithm_from_str() {
        assert_eq!(
            ResizeAlgorithm::from_str_lossy("nearest"),
            ResizeAlgorithm::Nearest
        );
        assert_eq!(
            ResizeAlgorithm::from_str_lossy("neighbor"),
            ResizeAlgorithm::Nearest
        );
        assert_eq!(
            ResizeAlgorithm::from_str_lossy("nn"),
            ResizeAlgorithm::Nearest
        );
        assert_eq!(
            ResizeAlgorithm::from_str_lossy("bilinear"),
            ResizeAlgorithm::Bilinear
        );
        assert_eq!(
            ResizeAlgorithm::from_str_lossy("lanczos"),
            ResizeAlgorithm::Bilinear
        );
        assert_eq!(
            ResizeAlgorithm::from_str_lossy("bicubic"),
            ResizeAlgorithm::Bilinear
        );
        assert_eq!(
            ResizeAlgorithm::from_str_lossy("unknown"),
            ResizeAlgorithm::Bilinear
        );
    }

    #[test]
    fn test_resize_2x2_checkerboard_nearest() {
        let src = vec![0, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0, 0];

        let result = resize_nearest(&src, 2, 2, 4, 4);
        assert_eq!(result.len(), 4 * 4 * 3);

        assert_eq!(&result[0..3], &[0, 0, 0]);
        assert_eq!(&result[(4 * 3 - 3)..(4 * 3)], &[255, 255, 255]);
    }
}
