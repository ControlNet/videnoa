use std::collections::HashMap;

use anyhow::{bail, Result};

use crate::node::{ExecutionContext, Node, PortDefinition};
use crate::types::{Frame, PortData, PortType};

const DOWNSCALE_WIDTH: usize = 160;
const DOWNSCALE_HEIGHT: usize = 90;

pub struct SceneDetectNode {
    threshold: f64,
}

impl SceneDetectNode {
    pub fn new() -> Self {
        Self { threshold: 0.3 }
    }

    /// Compare two frames and return `true` if a scene change is detected.
    ///
    /// Algorithm: downscale both frames, compute average luma per frame,
    /// return true if the absolute difference exceeds the threshold.
    /// Luma = R*0.299 + G*0.587 + B*0.114 (BT.601 coefficients).
    pub fn analyze_frame_pair(&self, frame0: &Frame, frame1: &Frame) -> Result<bool> {
        let luma0 = compute_average_luma_downscaled(frame0)?;
        let luma1 = compute_average_luma_downscaled(frame1)?;
        let diff = (luma0 - luma1).abs();
        Ok(diff > self.threshold)
    }

    pub fn threshold(&self) -> f64 {
        self.threshold
    }
}

impl Default for SceneDetectNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for SceneDetectNode {
    fn node_type(&self) -> &str {
        "SceneDetect"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![PortDefinition {
            name: "threshold".to_string(),
            port_type: PortType::Float,
            required: false,
            default_value: Some(serde_json::json!(0.3)),
        }]
    }

    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![PortDefinition {
            name: "is_scene_change".to_string(),
            port_type: PortType::Bool,
            required: true,
            default_value: None,
        }]
    }

    fn execute(
        &mut self,
        inputs: &HashMap<String, PortData>,
        _ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        if let Some(PortData::Float(t)) = inputs.get("threshold") {
            if *t < 0.0 || *t > 1.0 {
                bail!("threshold must be in [0.0, 1.0], got {t}");
            }
            self.threshold = *t;
        }

        Ok(HashMap::new())
    }
}

/// Compute average luma of a frame after downscaling to a small fixed resolution.
///
/// Uses area-average downscaling and BT.601 luma: Y = R*0.299 + G*0.587 + B*0.114.
/// Returns luma normalized to [0.0, 1.0].
fn compute_average_luma_downscaled(frame: &Frame) -> Result<f64> {
    let (data, src_w, src_h, bit_depth) = match frame {
        Frame::CpuRgb {
            data,
            width,
            height,
            bit_depth,
        } => (data, *width as usize, *height as usize, *bit_depth),
        _ => bail!("SceneDetect only supports Frame::CpuRgb"),
    };

    if bit_depth != 8 {
        bail!("SceneDetect only supports 8-bit frames, got {bit_depth}-bit");
    }

    let expected_len = src_w * src_h * 3;
    if data.len() != expected_len {
        bail!(
            "Frame data length mismatch: expected {expected_len}, got {}",
            data.len()
        );
    }

    let dst_w = DOWNSCALE_WIDTH.min(src_w);
    let dst_h = DOWNSCALE_HEIGHT.min(src_h);

    let mut total_luma = 0.0f64;
    let pixel_count = dst_w * dst_h;

    for dst_y in 0..dst_h {
        let src_y0 = dst_y * src_h / dst_h;
        let src_y1 = ((dst_y + 1) * src_h / dst_h).min(src_h);

        for dst_x in 0..dst_w {
            let src_x0 = dst_x * src_w / dst_w;
            let src_x1 = ((dst_x + 1) * src_w / dst_w).min(src_w);

            let mut r_sum = 0u64;
            let mut g_sum = 0u64;
            let mut b_sum = 0u64;
            let mut count = 0u64;

            for sy in src_y0..src_y1 {
                for sx in src_x0..src_x1 {
                    let idx = (sy * src_w + sx) * 3;
                    r_sum += data[idx] as u64;
                    g_sum += data[idx + 1] as u64;
                    b_sum += data[idx + 2] as u64;
                    count += 1;
                }
            }

            if count > 0 {
                let r = r_sum as f64 / count as f64;
                let g = g_sum as f64 / count as f64;
                let b = b_sum as f64 / count as f64;
                total_luma += r * 0.299 + g * 0.587 + b * 0.114;
            }
        }
    }

    Ok(total_luma / (pixel_count as f64 * 255.0))
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
    fn test_scene_detect_node_ports() {
        let node = SceneDetectNode::new();
        assert_eq!(node.node_type(), "SceneDetect");

        let inputs = node.input_ports();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].name, "threshold");
        assert_eq!(inputs[0].port_type, PortType::Float);
        assert!(!inputs[0].required);

        let outputs = node.output_ports();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].name, "is_scene_change");
        assert_eq!(outputs[0].port_type, PortType::Bool);
    }

    #[test]
    fn test_scene_detect_default_threshold() {
        let node = SceneDetectNode::new();
        assert!((node.threshold() - 0.3).abs() < f64::EPSILON);
    }

    #[test]
    fn test_scene_detect_configure_threshold() {
        let mut node = SceneDetectNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("threshold".to_string(), PortData::Float(0.5));
        node.execute(&inputs, &ctx).unwrap();
        assert!((node.threshold() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_scene_detect_invalid_threshold() {
        let mut node = SceneDetectNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("threshold".to_string(), PortData::Float(1.5));
        let result = node.execute(&inputs, &ctx);
        let err = result.err().expect("should fail");
        assert!(err.to_string().contains("threshold"));
    }

    #[test]
    fn test_identical_frames_no_scene_change() {
        let node = SceneDetectNode::new();
        let frame0 = make_solid_frame(320, 240, 128, 128, 128);
        let frame1 = make_solid_frame(320, 240, 128, 128, 128);
        let is_scene = node.analyze_frame_pair(&frame0, &frame1).unwrap();
        assert!(!is_scene);
    }

    #[test]
    fn test_very_different_frames_scene_change() {
        let node = SceneDetectNode::new();
        let frame0 = make_solid_frame(320, 240, 0, 0, 0);
        let frame1 = make_solid_frame(320, 240, 255, 255, 255);
        let is_scene = node.analyze_frame_pair(&frame0, &frame1).unwrap();
        assert!(is_scene);
    }

    #[test]
    fn test_similar_frames_no_scene_change() {
        let node = SceneDetectNode::new();
        let frame0 = make_solid_frame(320, 240, 128, 128, 128);
        let frame1 = make_solid_frame(320, 240, 130, 130, 130);
        let is_scene = node.analyze_frame_pair(&frame0, &frame1).unwrap();
        assert!(!is_scene);
    }

    #[test]
    fn test_scene_detect_with_low_threshold() {
        let mut node = SceneDetectNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("threshold".to_string(), PortData::Float(0.01));
        node.execute(&inputs, &ctx).unwrap();

        let frame0 = make_solid_frame(320, 240, 100, 100, 100);
        let frame1 = make_solid_frame(320, 240, 110, 110, 110);
        let is_scene = node.analyze_frame_pair(&frame0, &frame1).unwrap();
        assert!(is_scene);
    }

    #[test]
    fn test_scene_detect_black_to_white() {
        let node = SceneDetectNode::new();
        let black = make_solid_frame(640, 480, 0, 0, 0);
        let white = make_solid_frame(640, 480, 255, 255, 255);

        let luma_black = compute_average_luma_downscaled(&black).unwrap();
        let luma_white = compute_average_luma_downscaled(&white).unwrap();

        assert!(luma_black < 0.01);
        assert!((luma_white - 1.0).abs() < 0.01);

        let is_scene = node.analyze_frame_pair(&black, &white).unwrap();
        assert!(is_scene);
    }

    #[test]
    fn test_scene_detect_small_frame() {
        let node = SceneDetectNode::new();
        let frame0 = make_solid_frame(4, 4, 0, 0, 0);
        let frame1 = make_solid_frame(4, 4, 255, 255, 255);
        let is_scene = node.analyze_frame_pair(&frame0, &frame1).unwrap();
        assert!(is_scene);
    }

    #[test]
    fn test_luma_calculation_red() {
        let frame = make_solid_frame(320, 240, 255, 0, 0);
        let luma = compute_average_luma_downscaled(&frame).unwrap();
        let expected = 255.0 * 0.299 / 255.0;
        assert!((luma - expected).abs() < 0.01);
    }

    #[test]
    fn test_luma_calculation_green() {
        let frame = make_solid_frame(320, 240, 0, 255, 0);
        let luma = compute_average_luma_downscaled(&frame).unwrap();
        let expected = 255.0 * 0.587 / 255.0;
        assert!((luma - expected).abs() < 0.01);
    }

    #[test]
    fn test_luma_calculation_blue() {
        let frame = make_solid_frame(320, 240, 0, 0, 255);
        let luma = compute_average_luma_downscaled(&frame).unwrap();
        let expected = 255.0 * 0.114 / 255.0;
        assert!((luma - expected).abs() < 0.01);
    }
}
