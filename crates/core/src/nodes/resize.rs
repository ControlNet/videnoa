//! Resize node: pure-Rust Lanczos/bilinear/nearest-neighbor frame resizing.

use std::collections::HashMap;

use anyhow::{bail, Result};

use crate::node::{ExecutionContext, FrameProcessor, Node, PortDefinition};
use crate::types::{Frame, PortData, PortType};

/// Supported resize algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeAlgorithm {
    Lanczos,
    Bilinear,
    Nearest,
}

impl ResizeAlgorithm {
    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "lanczos" => Self::Lanczos,
            "nearest" | "neighbor" | "nn" => Self::Nearest,
            _ => Self::Bilinear,
        }
    }

    /// FFmpeg swscale flag name for this algorithm.
    pub fn ffmpeg_flag(self) -> &'static str {
        match self {
            Self::Lanczos => "lanczos",
            Self::Bilinear => "bilinear",
            Self::Nearest => "neighbor",
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
            algorithm: ResizeAlgorithm::Lanczos,
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
                default_value: Some(serde_json::json!("lanczos")),
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

                let out_data = resize_rgb24(
                    self.algorithm,
                    data,
                    in_w as usize,
                    in_h as usize,
                    self.out_width as usize,
                    self.out_height as usize,
                );

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

/// Resize 8-bit RGB24 data with the given algorithm.
pub(crate) fn resize_rgb24(
    algorithm: ResizeAlgorithm,
    src: &[u8],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
) -> Vec<u8> {
    match algorithm {
        ResizeAlgorithm::Lanczos => resize_lanczos(src, src_w, src_h, dst_w, dst_h),
        ResizeAlgorithm::Bilinear => resize_bilinear(src, src_w, src_h, dst_w, dst_h),
        ResizeAlgorithm::Nearest => resize_nearest(src, src_w, src_h, dst_w, dst_h),
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

const LANCZOS_LOBES: f64 = 3.0;

fn lanczos_kernel(x: f64) -> f64 {
    if x == 0.0 {
        return 1.0;
    }
    if x.abs() >= LANCZOS_LOBES {
        return 0.0;
    }
    let px = std::f64::consts::PI * x;
    LANCZOS_LOBES * px.sin() * (px / LANCZOS_LOBES).sin() / (px * px)
}

/// Normalized Lanczos-3 taps `(source index, weight)` for each destination sample.
/// The kernel is widened when downscaling so it also acts as a low-pass filter.
fn lanczos_taps(src_len: usize, dst_len: usize) -> Vec<Vec<(usize, f64)>> {
    let ratio = src_len as f64 / dst_len as f64;
    let support = ratio.max(1.0);
    let radius = LANCZOS_LOBES * support;
    let last = src_len as isize - 1;

    (0..dst_len)
        .map(|dst| {
            let center = (dst as f64 + 0.5) * ratio - 0.5;
            let first = (center - radius).floor() as isize;
            let end = (center + radius).ceil() as isize;
            let mut taps: Vec<(usize, f64)> = Vec::new();
            for src in first..=end {
                let weight = lanczos_kernel((src as f64 - center) / support);
                if weight == 0.0 {
                    continue;
                }
                let index = src.clamp(0, last) as usize;
                match taps.iter_mut().find(|(existing, _)| *existing == index) {
                    Some((_, accumulated)) => *accumulated += weight,
                    None => taps.push((index, weight)),
                }
            }
            let sum: f64 = taps.iter().map(|(_, weight)| weight).sum();
            for (_, weight) in &mut taps {
                *weight /= sum;
            }
            taps
        })
        .collect()
}

/// Separable Lanczos-3 resize for 8-bit RGB24 data.
pub(crate) fn resize_lanczos(
    src: &[u8],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
) -> Vec<u8> {
    let x_taps = lanczos_taps(src_w, dst_w);
    let y_taps = lanczos_taps(src_h, dst_h);

    // Horizontal pass: src_h rows of dst_w pixels, kept in f64 to avoid double rounding.
    let mut horizontal = vec![0.0f64; src_h * dst_w * 3];
    for y in 0..src_h {
        let src_row = &src[y * src_w * 3..(y + 1) * src_w * 3];
        let dst_row = &mut horizontal[y * dst_w * 3..(y + 1) * dst_w * 3];
        for (x, taps) in x_taps.iter().enumerate() {
            for c in 0..3 {
                dst_row[x * 3 + c] = taps
                    .iter()
                    .map(|&(sx, weight)| src_row[sx * 3 + c] as f64 * weight)
                    .sum();
            }
        }
    }

    let mut dst = vec![0u8; dst_w * dst_h * 3];
    for (y, taps) in y_taps.iter().enumerate() {
        for x in 0..dst_w {
            for c in 0..3 {
                let value: f64 = taps
                    .iter()
                    .map(|&(sy, weight)| horizontal[(sy * dst_w + x) * 3 + c] * weight)
                    .sum();
                dst[(y * dst_w + x) * 3 + c] = value.round().clamp(0.0, 255.0) as u8;
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
        for pixel in data.as_chunks_mut::<3>().0 {
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
                for pixel in data.as_chunks::<3>().0 {
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
                for pixel in data.as_chunks::<3>().0 {
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
            ResizeAlgorithm::Lanczos
        );
        assert_eq!(
            ResizeAlgorithm::from_str_lossy("Lanczos"),
            ResizeAlgorithm::Lanczos
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

    #[test]
    fn test_resize_defaults_to_lanczos() {
        let node = ResizeNode::new();
        let algorithm = node
            .input_ports()
            .into_iter()
            .find(|port| port.name == "algorithm")
            .expect("Resize should expose algorithm");
        assert_eq!(algorithm.default_value, Some(serde_json::json!("lanczos")));
    }

    #[test]
    fn test_resize_algorithm_ffmpeg_flags() {
        assert_eq!(ResizeAlgorithm::Lanczos.ffmpeg_flag(), "lanczos");
        assert_eq!(ResizeAlgorithm::Bilinear.ffmpeg_flag(), "bilinear");
        assert_eq!(ResizeAlgorithm::Nearest.ffmpeg_flag(), "neighbor");
    }

    #[test]
    fn test_resize_lanczos_solid_color() {
        let mut node = ResizeNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("width".to_string(), PortData::Int(3));
        inputs.insert("height".to_string(), PortData::Int(5));
        inputs.insert(
            "algorithm".to_string(),
            PortData::Str("lanczos".to_string()),
        );
        node.execute(&inputs, &ctx).unwrap();

        let result = node
            .process_frame(make_solid_frame(8, 8, 10, 128, 250), &ctx)
            .unwrap();
        match result {
            Frame::CpuRgb {
                data,
                width,
                height,
                bit_depth,
            } => {
                assert_eq!((width, height, bit_depth), (3, 5, 8));
                for pixel in data.as_chunks::<3>().0 {
                    assert_eq!(pixel, &[10, 128, 250]);
                }
            }
            _ => panic!("Expected CpuRgb frame"),
        }
    }

    #[test]
    fn test_resize_lanczos_identity_preserves_pixels() {
        let src: Vec<u8> = (0..5 * 4 * 3).map(|i| (i * 37 % 256) as u8).collect();
        let result = resize_lanczos(&src, 5, 4, 5, 4);
        assert_eq!(result, src);
    }

    #[test]
    fn test_resize_lanczos_downscale_stays_within_source_range() {
        // A hard edge must not overshoot past the representable range.
        let mut src = vec![0u8; 16 * 2 * 3];
        for y in 0..2 {
            for x in 8..16 {
                let i = (y * 16 + x) * 3;
                src[i..i + 3].copy_from_slice(&[255, 255, 255]);
            }
        }
        let result = resize_lanczos(&src, 16, 2, 12, 2);
        assert_eq!(result.len(), 12 * 2 * 3);
        assert_eq!(&result[0..3], &[0, 0, 0]);
        assert_eq!(&result[result.len() - 3..], &[255, 255, 255]);
    }
}
