//! SuperResolution node: upscaling via `ort::Session` + CUDA/TensorRT EP.
//!
//! Supports both FP32 models (e.g. Real-ESRGAN, value range 0–255)
//! and FP16 models (e.g. AnimeJaNai, value range 0–1).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use half::f16;
use half::slice::HalfFloatSliceExt;
use ndarray::{s, Array4};
use ort::{session::Session, value::Tensor};
use tracing::debug;

use crate::node::{ExecutionContext, FrameProcessor, Node, PortDefinition};
use crate::types::{Frame, PortData, PortType};

use crate::nodes::backend::{build_session, InferenceBackend, SessionConfig};

/// Tile overlap in pixels per side — prevents seam artifacts between tiles.
const DEFAULT_TILE_OVERLAP: usize = 16;

/// Model requires spatial dimensions to be multiples of this.
const PAD_ALIGN: usize = 4;

pub struct SuperResNode {
    session: Option<Arc<Mutex<Session>>>,
    scale: u32,
    tile_size: u32,
    backend: InferenceBackend,
    use_iobinding: bool,
    trt_cache_dir: Option<PathBuf>,
    input_name: Option<String>,
    output_name: Option<String>,
    is_fp16_model: bool,
    /// Reusable f32 NCHW buffer for FP32 path — avoids ~24 MB allocation per frame at 1080p.
    f32_nchw_buf: Option<Array4<f32>>,
    /// Reusable f16 NCHW buffer for FP16 path — avoids ~12 MB allocation per frame at 1080p.
    f16_nchw_buf: Option<ndarray::ArrayD<f16>>,
    /// When true and model is FP16, emit Frame::NchwF16 instead of CpuRgb.
    /// Set by compile_graph when downstream node can accept tensor input.
    pub emit_tensor: bool,
}

impl SuperResNode {
    pub fn new() -> Self {
        Self {
            session: None,
            scale: 4,
            tile_size: 0,
            backend: InferenceBackend::default(),
            use_iobinding: true,
            trt_cache_dir: None,
            input_name: None,
            output_name: None,
            is_fp16_model: false,
            f32_nchw_buf: None,
            f16_nchw_buf: None,
            emit_tensor: false,
        }
    }

    pub fn set_emit_tensor(&mut self, emit: bool) {
        self.emit_tensor = emit;
    }

    pub fn set_trt_cache_dir(&mut self, dir: PathBuf) {
        self.trt_cache_dir = Some(dir);
    }

    pub fn is_fp16(&self) -> bool {
        self.is_fp16_model
    }

    pub fn tile_size(&self) -> u32 {
        self.tile_size
    }
}

impl Default for SuperResNode {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of splitting a loaded `SuperResNode` into three independent micro-stages
/// for pipeline parallelism. Each stage runs on its own thread; CPU work (pre/post)
/// overlaps with GPU inference on different frames.
///
/// Only supported for **FP16 models with tile_size=0** (full-frame inference).
pub struct SuperResMicroStages {
    pub preprocess: SuperResPreprocess,
    pub inference: SuperResInference,
    pub postprocess: SuperResPostprocess,
}

impl SuperResNode {
    /// Split a loaded `SuperResNode` into three pipeline-parallel micro-stages:
    ///
    /// 1. **Preprocess** (CPU, ~10ms): `CpuRgb → NchwF16` — u8→f16 ÷255, HWC→CHW
    /// 2. **Inference** (GPU, ~53ms): `NchwF16 → NchwF16` — pad → session.run → unpad
    /// 3. **Postprocess** (CPU, ~30ms): `NchwF16 → CpuRgb` — f16→u8 ×255, CHW→HWC
    ///
    /// Returns `None` if the model is FP32 or uses tiled inference (tile_size > 0),
    /// in which case the caller should fall back to using the whole `SuperResNode`
    /// as a single `FrameProcessor` stage.
    pub fn into_micro_stages(self) -> Option<SuperResMicroStages> {
        if !self.is_fp16_model || self.tile_size > 0 {
            return None;
        }
        let session = self.session?;
        let input_name = self.input_name?;
        let output_name = self.output_name?;

        Some(SuperResMicroStages {
            preprocess: SuperResPreprocess { f16_nchw_buf: None },
            inference: SuperResInference {
                session,
                scale: self.scale as usize,
                input_name,
                output_name,
            },
            postprocess: SuperResPostprocess,
        })
    }
}

// ---------------------------------------------------------------------------
// Micro-stage 1: Preprocess (CPU-only, ~10ms)
// ---------------------------------------------------------------------------

/// Converts `Frame::CpuRgb` → `Frame::NchwF16`.
///
/// Performs u8 → f16 with ÷255 normalization and HWC → CHW deinterleave.
/// Does NOT pad — padding is handled by the inference stage.
pub struct SuperResPreprocess {
    f16_nchw_buf: Option<ndarray::ArrayD<f16>>,
}

impl Node for SuperResPreprocess {
    fn node_type(&self) -> &str {
        "SuperResPreprocess"
    }
    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![]
    }
    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![]
    }
    fn execute(
        &mut self,
        _inputs: &HashMap<String, PortData>,
        _ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        Ok(HashMap::new())
    }
}

impl FrameProcessor for SuperResPreprocess {
    fn process_frame(&mut self, frame: Frame, _ctx: &ExecutionContext) -> Result<Frame> {
        match frame {
            Frame::CpuRgb {
                data,
                width,
                height,
                bit_depth,
            } => {
                let h = height as usize;
                let w = width as usize;
                let expected_len = match bit_depth {
                    8 => h * w * 3,
                    9..=16 => h * w * 3 * 2,
                    _ => {
                        bail!(
                            "SuperResPreprocess: unsupported bit depth {bit_depth} (expected 8..=16)"
                        )
                    }
                };

                if data.len() != expected_len {
                    bail!(
                        "SuperResPreprocess: data length mismatch: expected {} ({}x{}x{}), got {}",
                        expected_len,
                        h,
                        w,
                        if bit_depth == 8 { 3 } else { 6 },
                        data.len()
                    );
                }

                let target_shape: &[usize] = &[1, 3, h, w];
                let mut nchw = match self.f16_nchw_buf.take() {
                    Some(mut arr) if arr.shape() == target_shape => {
                        arr.fill(f16::ZERO);
                        arr
                    }
                    _ => ndarray::ArrayD::from_elem(ndarray::IxDyn(&[1, 3, h, w]), f16::ZERO),
                };
                let hw = h * w;
                let nchw_slice = nchw.as_slice_mut().unwrap();

                const CHUNK: usize = 4096;
                let mut r_buf = [0.0f32; CHUNK];
                let mut g_buf = [0.0f32; CHUNK];
                let mut b_buf = [0.0f32; CHUNK];
                let source_max = if bit_depth > 8 {
                    Some(infer_high_bit_source_max(bit_depth, &data))
                } else {
                    None
                };

                let mut offset = 0;
                while offset < hw {
                    let len = CHUNK.min(hw - offset);
                    for j in 0..len {
                        if bit_depth == 8 {
                            let src = (offset + j) * 3;
                            r_buf[j] = data[src] as f32 / 255.0;
                            g_buf[j] = data[src + 1] as f32 / 255.0;
                            b_buf[j] = data[src + 2] as f32 / 255.0;
                        } else {
                            let src = (offset + j) * 6;
                            let source_max = source_max.expect("high bit-depth source max present");
                            let r = quantize_high_bit_sample_to_u8(
                                u16::from_le_bytes([data[src], data[src + 1]]) as u32,
                                source_max,
                            );
                            let g = quantize_high_bit_sample_to_u8(
                                u16::from_le_bytes([data[src + 2], data[src + 3]]) as u32,
                                source_max,
                            );
                            let b = quantize_high_bit_sample_to_u8(
                                u16::from_le_bytes([data[src + 4], data[src + 5]]) as u32,
                                source_max,
                            );
                            r_buf[j] = r as f32 / 255.0;
                            g_buf[j] = g as f32 / 255.0;
                            b_buf[j] = b as f32 / 255.0;
                        }
                    }
                    nchw_slice[offset..offset + len].convert_from_f32_slice(&r_buf[..len]);
                    nchw_slice[hw + offset..hw + offset + len]
                        .convert_from_f32_slice(&g_buf[..len]);
                    nchw_slice[2 * hw + offset..2 * hw + offset + len]
                        .convert_from_f32_slice(&b_buf[..len]);
                    offset += len;
                }

                let out_data: Vec<u16> = nchw_slice.iter().map(|v| v.to_bits()).collect();
                self.f16_nchw_buf = Some(nchw);

                Ok(Frame::NchwF16 {
                    data: out_data,
                    height,
                    width,
                })
            }
            Frame::NchwF32 {
                data,
                width,
                height,
            } => {
                let h = height as usize;
                let w = width as usize;
                let expected = 3 * h * w;

                if data.len() != expected {
                    bail!(
                        "SuperResPreprocess: NchwF32 length mismatch: expected {} (3×{}×{}), got {}",
                        expected,
                        h,
                        w,
                        data.len()
                    );
                }

                let target_shape: &[usize] = &[1, 3, h, w];
                let mut nchw = match self.f16_nchw_buf.take() {
                    Some(mut arr) if arr.shape() == target_shape => {
                        arr.fill(f16::ZERO);
                        arr
                    }
                    _ => ndarray::ArrayD::from_elem(ndarray::IxDyn(&[1, 3, h, w]), f16::ZERO),
                };
                let nchw_slice = nchw.as_slice_mut().unwrap();

                const CHUNK: usize = 4096;
                let mut offset = 0;
                while offset < expected {
                    let len = CHUNK.min(expected - offset);
                    nchw_slice[offset..offset + len]
                        .convert_from_f32_slice(&data[offset..offset + len]);
                    offset += len;
                }

                let out_data: Vec<u16> = nchw_slice.iter().map(|v| v.to_bits()).collect();
                self.f16_nchw_buf = Some(nchw);

                Ok(Frame::NchwF16 {
                    data: out_data,
                    height,
                    width,
                })
            }
            _ => bail!("SuperResPreprocess: expected CpuRgb or NchwF32, got other variant"),
        }
    }
}

// ---------------------------------------------------------------------------
// Micro-stage 2: Inference (GPU, ~53ms)
// ---------------------------------------------------------------------------

/// Runs ORT/TensorRT FP16 inference: `NchwF16 → NchwF16`.
///
/// Internally pads to `PAD_ALIGN` multiples, runs `session.run()`, and crops
/// the output back to `(orig_h * scale, orig_w * scale)`.
pub struct SuperResInference {
    session: Arc<Mutex<Session>>,
    scale: usize,
    input_name: String,
    output_name: String,
}

impl Node for SuperResInference {
    fn node_type(&self) -> &str {
        "SuperResInference"
    }
    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![]
    }
    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![]
    }
    fn execute(
        &mut self,
        _inputs: &HashMap<String, PortData>,
        _ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        Ok(HashMap::new())
    }
}

impl FrameProcessor for SuperResInference {
    fn process_frame(&mut self, frame: Frame, _ctx: &ExecutionContext) -> Result<Frame> {
        let Frame::NchwF16 {
            data,
            height,
            width,
        } = frame
        else {
            bail!("SuperResInference: expected NchwF16, got other variant");
        };

        let h = height as usize;
        let w = width as usize;

        let f16_vec: Vec<f16> = data.into_iter().map(f16::from_bits).collect();
        let input_arr = ndarray::ArrayD::from_shape_vec(ndarray::IxDyn(&[1, 3, h, w]), f16_vec)
            .context("SuperResInference: failed to reshape input")?;

        let padded = pad_f16_nchw(&input_arr, h, w);

        let output_owned = {
            let mut session = self.session.lock().unwrap();
            run_direct_fp16_inference(&mut session, &padded, &self.input_name, &self.output_name)?
        };

        let out_h = h * self.scale;
        let out_w = w * self.scale;
        let padded_h = padded.shape()[2];
        let padded_w = padded.shape()[3];
        let pad_h = padded_h - h;
        let pad_w = padded_w - w;

        let final_arr = if pad_h > 0 || pad_w > 0 {
            output_owned
                .slice(s![.., .., ..out_h, ..out_w])
                .to_owned()
                .into_dyn()
        } else {
            output_owned
        };

        let owned_contig;
        let slice = if let Some(s) = final_arr.as_slice() {
            s
        } else {
            owned_contig = final_arr.as_standard_layout().into_owned();
            owned_contig.as_slice().unwrap()
        };
        let out_data: Vec<u16> = slice.iter().map(|v| v.to_bits()).collect();

        Ok(Frame::NchwF16 {
            data: out_data,
            height: out_h as u32,
            width: out_w as u32,
        })
    }
}

// ---------------------------------------------------------------------------
// Micro-stage 3: Postprocess (CPU-only, ~30ms)
// ---------------------------------------------------------------------------

/// Converts `Frame::NchwF16` → `Frame::CpuRgb`.
///
/// Performs f16 → u8 with ×255 denormalization and CHW → HWC interleave.
pub struct SuperResPostprocess;

impl Node for SuperResPostprocess {
    fn node_type(&self) -> &str {
        "SuperResPostprocess"
    }
    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![]
    }
    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![]
    }
    fn execute(
        &mut self,
        _inputs: &HashMap<String, PortData>,
        _ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        Ok(HashMap::new())
    }
}

impl FrameProcessor for SuperResPostprocess {
    fn process_frame(&mut self, frame: Frame, _ctx: &ExecutionContext) -> Result<Frame> {
        let Frame::NchwF16 {
            data,
            height,
            width,
        } = frame
        else {
            bail!("SuperResPostprocess: expected NchwF16, got other variant");
        };

        let h = height as usize;
        let w = width as usize;
        let hw = h * w;

        let f16_vec: Vec<f16> = data.into_iter().map(f16::from_bits).collect();
        if f16_vec.len() != 3 * hw {
            bail!(
                "SuperResPostprocess: f16 data length mismatch: expected {}, got {}",
                3 * hw,
                f16_vec.len()
            );
        }

        let r_chan = &f16_vec[..hw];
        let g_chan = &f16_vec[hw..2 * hw];
        let b_chan = &f16_vec[2 * hw..3 * hw];

        const CHUNK: usize = 4096;
        let mut r_buf = [0.0f32; CHUNK];
        let mut g_buf = [0.0f32; CHUNK];
        let mut b_buf = [0.0f32; CHUNK];

        let mut rgb = vec![0u8; hw * 3];
        let mut offset = 0;
        while offset < hw {
            let len = CHUNK.min(hw - offset);
            r_chan[offset..offset + len].convert_to_f32_slice(&mut r_buf[..len]);
            g_chan[offset..offset + len].convert_to_f32_slice(&mut g_buf[..len]);
            b_chan[offset..offset + len].convert_to_f32_slice(&mut b_buf[..len]);
            for j in 0..len {
                let dst = (offset + j) * 3;
                rgb[dst] = (r_buf[j] * 255.0).clamp(0.0, 255.0) as u8;
                rgb[dst + 1] = (g_buf[j] * 255.0).clamp(0.0, 255.0) as u8;
                rgb[dst + 2] = (b_buf[j] * 255.0).clamp(0.0, 255.0) as u8;
            }
            offset += len;
        }

        Ok(Frame::CpuRgb {
            data: rgb,
            width: width,
            height: height,
            bit_depth: 8,
        })
    }
}

impl Node for SuperResNode {
    fn node_type(&self) -> &str {
        "SuperResolution"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![
            PortDefinition {
                name: "model_path".to_string(),
                port_type: PortType::Path,
                required: true,
                default_value: None,
            },
            PortDefinition {
                name: "scale".to_string(),
                port_type: PortType::Int,
                required: false,
                default_value: Some(serde_json::json!(4)),
            },
            PortDefinition {
                name: "tile_size".to_string(),
                port_type: PortType::Int,
                required: false,
                default_value: Some(serde_json::json!(0)),
            },
            PortDefinition {
                name: "backend".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("cuda")),
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
        let model_path = match inputs.get("model_path") {
            Some(PortData::Path(p)) => p.clone(),
            Some(_) => bail!("model_path must be a Path"),
            None => bail!("model_path is required"),
        };

        if let Some(PortData::Int(s)) = inputs.get("scale") {
            self.scale = *s as u32;
        }

        if let Some(PortData::Int(t)) = inputs.get("tile_size") {
            self.tile_size = *t as u32;
        }

        if let Some(PortData::Str(b)) = inputs.get("backend") {
            self.backend = InferenceBackend::from_str_lossy(b);
        }

        debug!(
            model = %model_path.display(),
            scale = self.scale,
            tile_size = self.tile_size,
            backend = %self.backend,
            use_iobinding = self.use_iobinding,
            "Loading ONNX super-resolution model"
        );

        let config = SessionConfig {
            model_path: &model_path,
            backend: &self.backend,
            trt_cache_dir: self.trt_cache_dir.as_deref(),
        };

        let session = build_session(&config)?;

        let input_name = session.inputs()[0].name().to_string();
        let output_name = session.outputs()[0].name().to_string();
        let is_fp16 = match session.inputs()[0].dtype() {
            ort::value::ValueType::Tensor { ty, .. } => {
                *ty == ort::tensor::TensorElementType::Float16
            }
            _ => false,
        };

        debug!(
            %input_name, %output_name, is_fp16,
            "Detected model IO"
        );

        self.input_name = Some(input_name);
        self.output_name = Some(output_name);
        self.is_fp16_model = is_fp16;

        self.session = Some(Arc::new(Mutex::new(session)));
        debug!("Model loaded successfully");

        Ok(HashMap::new())
    }
}

impl FrameProcessor for SuperResNode {
    fn process_frame(&mut self, frame: Frame, _ctx: &ExecutionContext) -> Result<Frame> {
        let session_arc = self
            .session
            .as_ref()
            .context("Model not loaded — call execute() first")?
            .clone();

        let use_iobinding = self.use_iobinding;

        match frame {
            Frame::CpuRgb {
                data,
                width,
                height,
                bit_depth,
            } => {
                let scale = self.scale as usize;
                let tile_size = self.tile_size as usize;
                let in_name = self.input_name.as_deref().unwrap_or("image.1");
                let out_name = self.output_name.as_deref().unwrap_or("image");

                if self.is_fp16_model {
                    let (input_f16, orig_h, orig_w) = cpu_rgb_to_f16_nchw_into(
                        &data,
                        width,
                        height,
                        bit_depth,
                        &mut self.f16_nchw_buf,
                    )?;

                    let output_f16 = if tile_size > 0 {
                        run_tiled_f16_inference(
                            &session_arc,
                            &input_f16,
                            orig_h,
                            orig_w,
                            tile_size,
                            scale,
                            in_name,
                            out_name,
                        )?
                    } else {
                        run_single_f16_inference(
                            &session_arc,
                            &input_f16,
                            orig_h,
                            orig_w,
                            scale,
                            in_name,
                            out_name,
                        )?
                    };

                    let out_h = orig_h * scale;
                    let out_w = orig_w * scale;

                    if self.emit_tensor {
                        let owned_contig;
                        let slice = if let Some(s) = output_f16.as_slice() {
                            s
                        } else {
                            owned_contig = output_f16.as_standard_layout().into_owned();
                            owned_contig.as_slice().unwrap()
                        };
                        let data: Vec<u16> = slice.iter().map(|v| v.to_bits()).collect();
                        Ok(Frame::NchwF16 {
                            data,
                            height: out_h as u32,
                            width: out_w as u32,
                        })
                    } else {
                        let out_data = f16_nchw_to_cpu_rgb(&output_f16, out_h, out_w)?;
                        Ok(Frame::CpuRgb {
                            data: out_data,
                            width: out_w as u32,
                            height: out_h as u32,
                            bit_depth: 8,
                        })
                    }
                } else {
                    let (input_array, orig_h, orig_w) = cpu_rgb_to_nchw_into(
                        &data,
                        width,
                        height,
                        bit_depth,
                        &mut self.f32_nchw_buf,
                    )?;

                    let output_array = if tile_size > 0 {
                        run_tiled_inference(
                            &session_arc,
                            &input_array,
                            orig_h,
                            orig_w,
                            tile_size,
                            scale,
                            use_iobinding,
                            in_name,
                            out_name,
                            false,
                        )?
                    } else {
                        run_single_inference(
                            &session_arc,
                            &input_array,
                            orig_h,
                            orig_w,
                            scale,
                            use_iobinding,
                            in_name,
                            out_name,
                            false,
                        )?
                    };

                    let out_h = orig_h * scale;
                    let out_w = orig_w * scale;
                    let out_data = nchw_to_cpu_rgb(&output_array, out_h, out_w)?;

                    Ok(Frame::CpuRgb {
                        data: out_data,
                        width: out_w as u32,
                        height: out_h as u32,
                        bit_depth: 8,
                    })
                }
            }
            Frame::NchwF32 {
                data,
                width,
                height,
            } => {
                let scale = self.scale as usize;
                let tile_size = self.tile_size as usize;
                let in_name = self.input_name.as_deref().unwrap_or("image.1");
                let out_name = self.output_name.as_deref().unwrap_or("image");
                let h = height as usize;
                let w = width as usize;

                if self.is_fp16_model {
                    let input_f16 = nchw_f32_to_f16_padded(&data, h, w)?;

                    let output_f16 = if tile_size > 0 {
                        run_tiled_f16_inference(
                            &session_arc,
                            &input_f16,
                            h,
                            w,
                            tile_size,
                            scale,
                            in_name,
                            out_name,
                        )?
                    } else {
                        run_single_f16_inference(
                            &session_arc,
                            &input_f16,
                            h,
                            w,
                            scale,
                            in_name,
                            out_name,
                        )?
                    };

                    let out_h = h * scale;
                    let out_w = w * scale;

                    if self.emit_tensor {
                        let owned_contig;
                        let slice = if let Some(s) = output_f16.as_slice() {
                            s
                        } else {
                            owned_contig = output_f16.as_standard_layout().into_owned();
                            owned_contig.as_slice().unwrap()
                        };
                        let tensor_data: Vec<u16> = slice.iter().map(|v| v.to_bits()).collect();
                        Ok(Frame::NchwF16 {
                            data: tensor_data,
                            height: out_h as u32,
                            width: out_w as u32,
                        })
                    } else {
                        let out_data = f16_nchw_to_cpu_rgb(&output_f16, out_h, out_w)?;
                        Ok(Frame::CpuRgb {
                            data: out_data,
                            width: out_w as u32,
                            height: out_h as u32,
                            bit_depth: 8,
                        })
                    }
                } else {
                    // FP32 models (Real-ESRGAN) expect [0,255] range
                    let rescaled: Vec<f32> = data.iter().map(|&v| v * 255.0).collect();
                    let arr = Array4::from_shape_vec((1, 3, h, w), rescaled)
                        .context("SuperResNode: failed to reshape NchwF32 input")?;
                    let padded = pad_nchw(&arr, h, w);

                    let output_array = if tile_size > 0 {
                        run_tiled_inference(
                            &session_arc,
                            &padded,
                            h,
                            w,
                            tile_size,
                            scale,
                            use_iobinding,
                            in_name,
                            out_name,
                            false,
                        )?
                    } else {
                        run_single_inference(
                            &session_arc,
                            &padded,
                            h,
                            w,
                            scale,
                            use_iobinding,
                            in_name,
                            out_name,
                            false,
                        )?
                    };

                    let out_h = h * scale;
                    let out_w = w * scale;
                    let out_data = nchw_to_cpu_rgb(&output_array, out_h, out_w)?;

                    Ok(Frame::CpuRgb {
                        data: out_data,
                        width: out_w as u32,
                        height: out_h as u32,
                        bit_depth: 8,
                    })
                }
            }
            _ => bail!("SuperResNode only supports Frame::CpuRgb or NchwF32 input"),
        }
    }
}

/// Convert interleaved HWC CPU RGB bytes → NCHW `[1,3,H,W]` float32 (0–255 range).
///
/// Returns `(padded_array, original_h, original_w)`. The array is reflection-padded
/// so H and W are multiples of [`PAD_ALIGN`].
#[cfg(test)]
fn cpu_rgb_to_nchw(
    data: &[u8],
    width: u32,
    height: u32,
    bit_depth: u8,
) -> Result<(Array4<f32>, usize, usize)> {
    cpu_rgb_to_nchw_into(data, width, height, bit_depth, &mut None)
}

fn cpu_rgb_to_nchw_into(
    data: &[u8],
    width: u32,
    height: u32,
    bit_depth: u8,
    buf: &mut Option<Array4<f32>>,
) -> Result<(Array4<f32>, usize, usize)> {
    let h = height as usize;
    let w = width as usize;

    let target_shape = [1, 3, h, w];
    let mut nchw = match buf.take() {
        Some(mut arr) if arr.shape() == target_shape => {
            arr.fill(0.0);
            arr
        }
        _ => Array4::<f32>::zeros((1, 3, h, w)),
    };
    assert!(nchw.as_slice().is_some(), "nchw must be C-contiguous");
    let slice = nchw.as_slice_mut().unwrap();
    let hw = h * w;

    match bit_depth {
        8 => {
            if data.len() != h * w * 3 {
                bail!(
                    "Data length mismatch: expected {} ({}x{}x3), got {}",
                    h * w * 3,
                    h,
                    w,
                    data.len()
                );
            }
            // Real-ESRGAN expects 0-255 range, NOT 0-1
            for y in 0..h {
                for x in 0..w {
                    let src_idx = (y * w + x) * 3;
                    let pixel_idx = y * w + x;
                    slice[pixel_idx] = data[src_idx] as f32; // R channel: offset 0
                    slice[hw + pixel_idx] = data[src_idx + 1] as f32; // G channel: offset H*W
                    slice[2 * hw + pixel_idx] = data[src_idx + 2] as f32; // B channel: offset 2*H*W
                }
            }
        }
        16 => {
            let expected = h * w * 3 * 2;
            if data.len() != expected {
                bail!(
                    "Data length mismatch for 16-bit: expected {}, got {}",
                    expected,
                    data.len()
                );
            }
            // u16 LE pairs → f32, scaled from 0-65535 to 0-255
            for y in 0..h {
                for x in 0..w {
                    let src_idx = (y * w + x) * 3;
                    let pixel_idx = y * w + x;
                    let r = u16::from_le_bytes([data[src_idx * 2], data[src_idx * 2 + 1]]);
                    let g =
                        u16::from_le_bytes([data[(src_idx + 1) * 2], data[(src_idx + 1) * 2 + 1]]);
                    let b =
                        u16::from_le_bytes([data[(src_idx + 2) * 2], data[(src_idx + 2) * 2 + 1]]);
                    let scale = 255.0 / 65535.0;
                    slice[pixel_idx] = r as f32 * scale;
                    slice[hw + pixel_idx] = g as f32 * scale;
                    slice[2 * hw + pixel_idx] = b as f32 * scale;
                }
            }
        }
        9..=15 => {
            let expected = h * w * 3 * 2;
            if data.len() != expected {
                bail!(
                    "Data length mismatch for {bit_depth}-bit: expected {}, got {}",
                    expected,
                    data.len()
                );
            }

            let source_max = infer_high_bit_source_max(bit_depth, data);
            for y in 0..h {
                for x in 0..w {
                    let src_idx = (y * w + x) * 3;
                    let pixel_idx = y * w + x;
                    let r = quantize_high_bit_sample_to_u8(
                        u16::from_le_bytes([data[src_idx * 2], data[src_idx * 2 + 1]]) as u32,
                        source_max,
                    );
                    let g = quantize_high_bit_sample_to_u8(
                        u16::from_le_bytes([data[(src_idx + 1) * 2], data[(src_idx + 1) * 2 + 1]])
                            as u32,
                        source_max,
                    );
                    let b = quantize_high_bit_sample_to_u8(
                        u16::from_le_bytes([data[(src_idx + 2) * 2], data[(src_idx + 2) * 2 + 1]])
                            as u32,
                        source_max,
                    );
                    slice[pixel_idx] = r as f32;
                    slice[hw + pixel_idx] = g as f32;
                    slice[2 * hw + pixel_idx] = b as f32;
                }
            }
        }
        _ => bail!("Unsupported bit depth: {bit_depth} (expected 8..=16)"),
    };

    let padded = pad_nchw(&nchw, h, w);
    *buf = Some(nchw);
    Ok((padded, h, w))
}

fn infer_high_bit_source_max(bit_depth: u8, data: &[u8]) -> u32 {
    let native_max = (1u32 << bit_depth) - 1;
    let has_wide_samples = data
        .chunks_exact(2)
        .any(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]) as u32 > native_max);

    if has_wide_samples {
        u16::MAX as u32
    } else {
        native_max
    }
}

fn quantize_high_bit_sample_to_u8(sample: u32, source_max: u32) -> u8 {
    let clamped = sample.min(source_max);
    (((clamped * 255) + source_max / 2) / source_max) as u8
}

/// Reflection-pad NCHW array so H and W are multiples of [`PAD_ALIGN`].
fn pad_nchw(arr: &Array4<f32>, h: usize, w: usize) -> Array4<f32> {
    let pad_h = (PAD_ALIGN - (h % PAD_ALIGN)) % PAD_ALIGN;
    let pad_w = (PAD_ALIGN - (w % PAD_ALIGN)) % PAD_ALIGN;

    if pad_h == 0 && pad_w == 0 {
        return arr.clone();
    }

    let new_h = h + pad_h;
    let new_w = w + pad_w;
    let mut padded = Array4::<f32>::zeros((1, 3, new_h, new_w));

    padded
        .slice_mut(s![.., .., ..h, ..w])
        .assign(&arr.slice(s![.., .., ..h, ..w]));

    for y in 0..pad_h {
        let src_y = h - 1 - y;
        for c in 0..3 {
            for x in 0..w {
                padded[[0, c, h + y, x]] = arr[[0, c, src_y, x]];
            }
        }
    }

    for x in 0..pad_w {
        let src_x = w - 1 - x;
        for c in 0..3 {
            for y in 0..new_h {
                let src_y = if y < h { y } else { h - 1 - (y - h) };
                padded[[0, c, y, w + x]] = arr[[0, c, src_y, src_x]];
            }
        }
    }

    padded
}

fn pad_amount(dim: usize) -> usize {
    (PAD_ALIGN - (dim % PAD_ALIGN)) % PAD_ALIGN
}

fn nchw_f32_to_f16_padded(data: &[f32], h: usize, w: usize) -> Result<ndarray::ArrayD<f16>> {
    let expected = 3 * h * w;
    if data.len() != expected {
        bail!(
            "nchw_f32_to_f16_padded: expected {} (3×{}×{}), got {}",
            expected,
            h,
            w,
            data.len()
        );
    }
    let target_shape: &[usize] = &[1, 3, h, w];
    let mut nchw = ndarray::ArrayD::from_elem(ndarray::IxDyn(target_shape), f16::ZERO);
    let nchw_slice = nchw.as_slice_mut().unwrap();

    const CHUNK: usize = 4096;
    let mut offset = 0;
    while offset < expected {
        let len = CHUNK.min(expected - offset);
        nchw_slice[offset..offset + len].convert_from_f32_slice(&data[offset..offset + len]);
        offset += len;
    }

    Ok(pad_f16_nchw(&nchw, h, w))
}

fn pad_f16_nchw(arr: &ndarray::ArrayD<f16>, h: usize, w: usize) -> ndarray::ArrayD<f16> {
    let pad_h = (PAD_ALIGN - (h % PAD_ALIGN)) % PAD_ALIGN;
    let pad_w = (PAD_ALIGN - (w % PAD_ALIGN)) % PAD_ALIGN;

    if pad_h == 0 && pad_w == 0 {
        return arr.clone();
    }

    let new_h = h + pad_h;
    let new_w = w + pad_w;
    let mut padded = ndarray::ArrayD::from_elem(ndarray::IxDyn(&[1, 3, new_h, new_w]), f16::ZERO);

    padded
        .slice_mut(s![.., .., ..h, ..w])
        .assign(&arr.slice(s![.., .., ..h, ..w]));

    for y in 0..pad_h {
        let src_y = h - 1 - y;
        for c in 0..3usize {
            for x in 0..w {
                padded[[0, c, h + y, x]] = arr[[0, c, src_y, x]];
            }
        }
    }

    for x in 0..pad_w {
        let src_x = w - 1 - x;
        for c in 0..3usize {
            for y in 0..new_h {
                let src_y = if y < h { y } else { h - 1 - (y - h) };
                padded[[0, c, y, w + x]] = arr[[0, c, src_y, src_x]];
            }
        }
    }

    padded
}

/// Convert interleaved HWC u8 RGB → f16 NCHW `[1,3,H,W]` with /255.0 normalization.
///
/// For FP16 models (e.g. AnimeJaNai) that expect 0–1 range.
/// Returns `(padded_array, original_h, original_w)`.
#[cfg(test)]
fn cpu_rgb_to_f16_nchw(
    data: &[u8],
    width: u32,
    height: u32,
) -> Result<(ndarray::ArrayD<f16>, usize, usize)> {
    cpu_rgb_to_f16_nchw_into(data, width, height, 8, &mut None)
}

fn cpu_rgb_to_f16_nchw_into(
    data: &[u8],
    width: u32,
    height: u32,
    bit_depth: u8,
    buf: &mut Option<ndarray::ArrayD<f16>>,
) -> Result<(ndarray::ArrayD<f16>, usize, usize)> {
    let h = height as usize;
    let w = width as usize;

    let expected_len = match bit_depth {
        8 => h * w * 3,
        9..=16 => h * w * 3 * 2,
        _ => bail!("Unsupported bit depth: {bit_depth} (expected 8..=16)"),
    };

    if data.len() != expected_len {
        bail!(
            "Data length mismatch: expected {} ({}x{}x{}), got {}",
            expected_len,
            h,
            w,
            if bit_depth == 8 { 3 } else { 6 },
            data.len()
        );
    }

    let target_shape: &[usize] = &[1, 3, h, w];
    let mut nchw = match buf.take() {
        Some(mut arr) if arr.shape() == target_shape => {
            arr.fill(f16::ZERO);
            arr
        }
        _ => ndarray::ArrayD::from_elem(ndarray::IxDyn(&[1, 3, h, w]), f16::ZERO),
    };
    let hw = h * w;
    let nchw_slice = nchw.as_slice_mut().unwrap();

    const CHUNK: usize = 4096;
    let mut r_buf = [0.0f32; CHUNK];
    let mut g_buf = [0.0f32; CHUNK];
    let mut b_buf = [0.0f32; CHUNK];
    let source_max = if bit_depth > 8 {
        Some(infer_high_bit_source_max(bit_depth, data))
    } else {
        None
    };

    let mut offset = 0;
    while offset < hw {
        let len = CHUNK.min(hw - offset);
        for j in 0..len {
            if bit_depth == 8 {
                let src = (offset + j) * 3;
                r_buf[j] = data[src] as f32 / 255.0;
                g_buf[j] = data[src + 1] as f32 / 255.0;
                b_buf[j] = data[src + 2] as f32 / 255.0;
            } else {
                let src = (offset + j) * 6;
                let source_max = source_max.expect("high bit-depth source max present");
                let r = quantize_high_bit_sample_to_u8(
                    u16::from_le_bytes([data[src], data[src + 1]]) as u32,
                    source_max,
                );
                let g = quantize_high_bit_sample_to_u8(
                    u16::from_le_bytes([data[src + 2], data[src + 3]]) as u32,
                    source_max,
                );
                let b = quantize_high_bit_sample_to_u8(
                    u16::from_le_bytes([data[src + 4], data[src + 5]]) as u32,
                    source_max,
                );
                r_buf[j] = r as f32 / 255.0;
                g_buf[j] = g as f32 / 255.0;
                b_buf[j] = b as f32 / 255.0;
            }
        }
        nchw_slice[offset..offset + len].convert_from_f32_slice(&r_buf[..len]);
        nchw_slice[hw + offset..hw + offset + len].convert_from_f32_slice(&g_buf[..len]);
        nchw_slice[2 * hw + offset..2 * hw + offset + len].convert_from_f32_slice(&b_buf[..len]);
        offset += len;
    }

    let padded = pad_f16_nchw(&nchw, h, w);
    *buf = Some(nchw);
    Ok((padded, h, w))
}

/// Convert NCHW `[1,3,H,W]` float32 → interleaved RGB u8, clamping to 0–255.
fn nchw_to_cpu_rgb(arr: &Array4<f32>, out_h: usize, out_w: usize) -> Result<Vec<u8>> {
    let owned_contig;
    let slice = if let Some(s) = arr.as_slice() {
        s
    } else {
        owned_contig = arr.as_standard_layout().into_owned();
        owned_contig.as_slice().unwrap()
    };
    let hw = out_h * out_w;

    let mut rgb = vec![0u8; hw * 3];
    for i in 0..hw {
        let r = slice[i].clamp(0.0, 255.0) as u8;
        let g = slice[hw + i].clamp(0.0, 255.0) as u8;
        let b = slice[2 * hw + i].clamp(0.0, 255.0) as u8;
        rgb[i * 3] = r;
        rgb[i * 3 + 1] = g;
        rgb[i * 3 + 2] = b;
    }
    Ok(rgb)
}

/// Convert f16 NCHW `[1,3,H,W]` (0–1 range) → interleaved RGB u8 with *255 + clamp.
fn f16_nchw_to_cpu_rgb(arr: &ndarray::ArrayD<f16>, out_h: usize, out_w: usize) -> Result<Vec<u8>> {
    let owned_contig;
    let slice = if let Some(s) = arr.as_slice() {
        s
    } else {
        owned_contig = arr.as_standard_layout().into_owned();
        owned_contig.as_slice().unwrap()
    };
    let hw = out_h * out_w;

    const CHUNK: usize = 4096;
    let mut r_buf = [0.0f32; CHUNK];
    let mut g_buf = [0.0f32; CHUNK];
    let mut b_buf = [0.0f32; CHUNK];

    let r_chan = &slice[..hw];
    let g_chan = &slice[hw..2 * hw];
    let b_chan = &slice[2 * hw..3 * hw];

    let mut rgb = vec![0u8; hw * 3];
    let mut offset = 0;
    while offset < hw {
        let len = CHUNK.min(hw - offset);
        r_chan[offset..offset + len].convert_to_f32_slice(&mut r_buf[..len]);
        g_chan[offset..offset + len].convert_to_f32_slice(&mut g_buf[..len]);
        b_chan[offset..offset + len].convert_to_f32_slice(&mut b_buf[..len]);
        for j in 0..len {
            let dst = (offset + j) * 3;
            rgb[dst] = (r_buf[j] * 255.0).clamp(0.0, 255.0) as u8;
            rgb[dst + 1] = (g_buf[j] * 255.0).clamp(0.0, 255.0) as u8;
            rgb[dst + 2] = (b_buf[j] * 255.0).clamp(0.0, 255.0) as u8;
        }
        offset += len;
    }
    Ok(rgb)
}

fn run_single_inference(
    session_arc: &Arc<Mutex<Session>>,
    input: &Array4<f32>,
    orig_h: usize,
    orig_w: usize,
    scale: usize,
    use_iobinding: bool,
    input_name: &str,
    output_name: &str,
    is_fp16: bool,
) -> Result<Array4<f32>> {
    let output_owned = {
        let mut session = session_arc.lock().unwrap();
        if is_fp16 {
            run_fp16_inference(&mut session, input, input_name, output_name)?
        } else if use_iobinding {
            let input_tensor = Tensor::from_array(input.clone())?;
            run_with_iobinding(&mut session, input_name, &input_tensor, output_name)?
        } else {
            let input_tensor = Tensor::from_array(input.clone())?;
            let outputs = session.run(ort::inputs![input_name => &input_tensor])?;
            let output_view = outputs[output_name].try_extract_array::<f32>()?;
            output_view.to_owned()
        }
    };

    let padded_h = input.shape()[2];
    let padded_w = input.shape()[3];
    let pad_h = padded_h - orig_h;
    let pad_w = padded_w - orig_w;

    let out_h = orig_h * scale;
    let out_w = orig_w * scale;

    if pad_h > 0 || pad_w > 0 {
        let cropped = output_owned
            .slice(s![.., .., ..out_h, ..out_w])
            .to_owned()
            .into_dimensionality::<ndarray::Ix4>()?;
        Ok(cropped)
    } else {
        Ok(output_owned.into_dimensionality::<ndarray::Ix4>()?)
    }
}

fn run_with_iobinding(
    session: &mut Session,
    input_name: &str,
    input_tensor: &Tensor<f32>,
    output_name: &str,
) -> Result<ndarray::ArrayBase<ndarray::OwnedRepr<f32>, ndarray::IxDyn>> {
    let mut binding = session.create_binding()?;
    binding.bind_input(input_name, input_tensor)?;
    binding.bind_output_to_device(output_name, &session.allocator().memory_info())?;
    let outputs = session.run_binding(&binding)?;
    let output_view = outputs[output_name].try_extract_array::<f32>()?;
    Ok(output_view.to_owned())
}

fn run_fp16_inference(
    session: &mut Session,
    input: &Array4<f32>,
    input_name: &str,
    output_name: &str,
) -> Result<ndarray::ArrayBase<ndarray::OwnedRepr<f32>, ndarray::IxDyn>> {
    let f32_slice = input
        .as_slice()
        .expect("input must be contiguous for SIMD f16 conversion");
    let mut fp16_data = vec![f16::ZERO; f32_slice.len()];
    fp16_data.convert_from_f32_slice(f32_slice);

    let shape: Vec<usize> = input.shape().to_vec();
    let fp16_array = ndarray::ArrayD::from_shape_vec(shape, fp16_data)?;
    let input_tensor = Tensor::from_array(fp16_array)?;
    let outputs = session.run(ort::inputs![input_name => &input_tensor])?;
    let output_view = outputs[output_name].try_extract_array::<f16>()?;

    let fp16_owned;
    let fp16_slice = if let Some(s) = output_view.as_slice() {
        s
    } else {
        fp16_owned = output_view.as_standard_layout().into_owned();
        fp16_owned.as_slice().unwrap()
    };
    let mut f32_data = vec![0.0f32; fp16_slice.len()];
    fp16_slice.convert_to_f32_slice(&mut f32_data);

    let f32_array = ndarray::ArrayD::from_shape_vec(output_view.shape().to_vec(), f32_data)?;
    Ok(f32_array)
}

fn run_direct_fp16_inference(
    session: &mut Session,
    input: &ndarray::ArrayD<f16>,
    input_name: &str,
    output_name: &str,
) -> Result<ndarray::ArrayD<f16>> {
    let input_tensor = Tensor::from_array(input.clone())?;
    let outputs = session.run(ort::inputs![input_name => &input_tensor])?;
    let output_view = outputs[output_name].try_extract_array::<f16>()?;
    Ok(output_view.to_owned())
}

fn run_tiled_inference(
    session_arc: &Arc<Mutex<Session>>,
    input: &Array4<f32>,
    orig_h: usize,
    orig_w: usize,
    tile_size: usize,
    scale: usize,
    use_iobinding: bool,
    input_name: &str,
    output_name: &str,
    is_fp16: bool,
) -> Result<Array4<f32>> {
    let out_h = orig_h * scale;
    let out_w = orig_w * scale;
    let mut output = Array4::<f32>::zeros((1, 3, out_h, out_w));

    let overlap = DEFAULT_TILE_OVERLAP;
    let step = tile_size.saturating_sub(overlap * 2);
    if step == 0 {
        bail!("tile_size ({tile_size}) is too small for overlap ({overlap})");
    }

    let padded_h = input.shape()[2];
    let padded_w = input.shape()[3];

    debug!(
        tile_size,
        overlap, step, padded_h, padded_w, "Starting tiled inference"
    );

    let mut y = 0usize;
    while y < orig_h {
        let mut x = 0usize;
        while x < orig_w {
            let in_y0 = y.saturating_sub(overlap);
            let in_x0 = x.saturating_sub(overlap);
            let in_y1 = (y + tile_size).min(padded_h);
            let in_x1 = (x + tile_size).min(padded_w);

            let tile_h = in_y1 - in_y0;
            let tile_w = in_x1 - in_x0;

            let tile_pad_h = pad_amount(tile_h);
            let tile_pad_w = pad_amount(tile_w);

            let tile_input = if tile_pad_h > 0 || tile_pad_w > 0 {
                let raw_tile = input
                    .slice(s![.., .., in_y0..in_y1, in_x0..in_x1])
                    .to_owned();
                pad_nchw(
                    &raw_tile.into_dimensionality::<ndarray::Ix4>()?,
                    tile_h,
                    tile_w,
                )
            } else {
                input
                    .slice(s![.., .., in_y0..in_y1, in_x0..in_x1])
                    .to_owned()
                    .into_dimensionality::<ndarray::Ix4>()?
            };

            let tile_output_owned = {
                let mut session = session_arc.lock().unwrap();
                if is_fp16 {
                    run_fp16_inference(&mut session, &tile_input, input_name, output_name)?
                } else if use_iobinding {
                    let input_tensor = Tensor::from_array(tile_input)?;
                    run_with_iobinding(&mut session, input_name, &input_tensor, output_name)?
                } else {
                    let input_tensor = Tensor::from_array(tile_input)?;
                    let outputs = session.run(ort::inputs![input_name => &input_tensor])?;
                    let output_view = outputs[output_name].try_extract_array::<f32>()?;
                    output_view.to_owned()
                }
            };

            let out_y0 = y * scale;
            let out_x0 = x * scale;
            let crop_y0 = (y - in_y0) * scale;
            let crop_x0 = (x - in_x0) * scale;

            let usable_h = (tile_h - (y - in_y0)).min(orig_h - y);
            let usable_w = (tile_w - (x - in_x0)).min(orig_w - x);
            let out_tile_h = usable_h * scale;
            let out_tile_w = usable_w * scale;

            let end_y = (out_y0 + out_tile_h).min(out_h);
            let end_x = (out_x0 + out_tile_w).min(out_w);
            let actual_h = end_y - out_y0;
            let actual_w = end_x - out_x0;

            output
                .slice_mut(s![.., .., out_y0..end_y, out_x0..end_x])
                .assign(&tile_output_owned.slice(s![
                    ..,
                    ..,
                    crop_y0..crop_y0 + actual_h,
                    crop_x0..crop_x0 + actual_w
                ]));

            x += step;
        }
        y += step;
    }

    Ok(output)
}

fn run_single_f16_inference(
    session_arc: &Arc<Mutex<Session>>,
    input: &ndarray::ArrayD<f16>,
    orig_h: usize,
    orig_w: usize,
    scale: usize,
    input_name: &str,
    output_name: &str,
) -> Result<ndarray::ArrayD<f16>> {
    let output_owned = {
        let mut session = session_arc.lock().unwrap();
        run_direct_fp16_inference(&mut session, input, input_name, output_name)?
    };

    let padded_h = input.shape()[2];
    let padded_w = input.shape()[3];
    let pad_h = padded_h - orig_h;
    let pad_w = padded_w - orig_w;

    let out_h = orig_h * scale;
    let out_w = orig_w * scale;

    if pad_h > 0 || pad_w > 0 {
        Ok(output_owned
            .slice(s![.., .., ..out_h, ..out_w])
            .to_owned()
            .into_dyn())
    } else {
        Ok(output_owned)
    }
}

fn run_tiled_f16_inference(
    session_arc: &Arc<Mutex<Session>>,
    input: &ndarray::ArrayD<f16>,
    orig_h: usize,
    orig_w: usize,
    tile_size: usize,
    scale: usize,
    input_name: &str,
    output_name: &str,
) -> Result<ndarray::ArrayD<f16>> {
    let out_h = orig_h * scale;
    let out_w = orig_w * scale;
    let mut output = ndarray::ArrayD::from_elem(ndarray::IxDyn(&[1, 3, out_h, out_w]), f16::ZERO);

    let overlap = DEFAULT_TILE_OVERLAP;
    let step = tile_size.saturating_sub(overlap * 2);
    if step == 0 {
        bail!("tile_size ({tile_size}) is too small for overlap ({overlap})");
    }

    let padded_h = input.shape()[2];
    let padded_w = input.shape()[3];

    debug!(
        tile_size,
        overlap, step, padded_h, padded_w, "Starting tiled f16 inference"
    );

    let mut y = 0usize;
    while y < orig_h {
        let mut x = 0usize;
        while x < orig_w {
            let in_y0 = y.saturating_sub(overlap);
            let in_x0 = x.saturating_sub(overlap);
            let in_y1 = (y + tile_size).min(padded_h);
            let in_x1 = (x + tile_size).min(padded_w);

            let tile_h = in_y1 - in_y0;
            let tile_w = in_x1 - in_x0;

            let tile_pad_h = pad_amount(tile_h);
            let tile_pad_w = pad_amount(tile_w);

            let raw_tile = input
                .slice(s![.., .., in_y0..in_y1, in_x0..in_x1])
                .to_owned()
                .into_dyn();

            let tile_input = if tile_pad_h > 0 || tile_pad_w > 0 {
                pad_f16_nchw(&raw_tile, tile_h, tile_w)
            } else {
                raw_tile
            };

            let tile_output_owned = {
                let mut session = session_arc.lock().unwrap();
                run_direct_fp16_inference(&mut session, &tile_input, input_name, output_name)?
            };

            let out_y0 = y * scale;
            let out_x0 = x * scale;
            let crop_y0 = (y - in_y0) * scale;
            let crop_x0 = (x - in_x0) * scale;

            let usable_h = (tile_h - (y - in_y0)).min(orig_h - y);
            let usable_w = (tile_w - (x - in_x0)).min(orig_w - x);
            let out_tile_h = usable_h * scale;
            let out_tile_w = usable_w * scale;

            let end_y = (out_y0 + out_tile_h).min(out_h);
            let end_x = (out_x0 + out_tile_w).min(out_w);
            let actual_h = end_y - out_y0;
            let actual_w = end_x - out_x0;

            output
                .slice_mut(s![.., .., out_y0..end_y, out_x0..end_x])
                .assign(&tile_output_owned.slice(s![
                    ..,
                    ..,
                    crop_y0..crop_y0 + actual_h,
                    crop_x0..crop_x0 + actual_w
                ]));

            x += step;
        }
        y += step;
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_rgb_to_nchw_basic() {
        let data = vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 128, 128, 128];
        let (arr, h, w) = cpu_rgb_to_nchw(&data, 2, 2, 8).unwrap();
        assert_eq!(h, 2);
        assert_eq!(w, 2);
        assert_eq!(arr.shape(), &[1, 3, 4, 4]);
        assert_eq!(arr[[0, 0, 0, 0]], 255.0);
        assert_eq!(arr[[0, 1, 0, 0]], 0.0);
        assert_eq!(arr[[0, 2, 0, 0]], 0.0);
        assert_eq!(arr[[0, 0, 0, 1]], 0.0);
        assert_eq!(arr[[0, 1, 0, 1]], 255.0);
    }

    #[test]
    fn test_cpu_rgb_to_nchw_aligned() {
        let data = vec![100u8; 4 * 4 * 3];
        let (arr, h, w) = cpu_rgb_to_nchw(&data, 4, 4, 8).unwrap();
        assert_eq!(h, 4);
        assert_eq!(w, 4);
        assert_eq!(arr.shape(), &[1, 3, 4, 4]);
    }

    #[test]
    fn test_cpu_rgb_to_nchw_16bit() {
        let mut data = Vec::new();
        for _ in 0..(2 * 2 * 3) {
            data.extend_from_slice(&65535u16.to_le_bytes());
        }
        let (arr, h, w) = cpu_rgb_to_nchw(&data, 2, 2, 16).unwrap();
        assert_eq!(h, 2);
        assert_eq!(w, 2);
        assert!((arr[[0, 0, 0, 0]] - 255.0).abs() < 0.01);
    }

    #[test]
    fn test_cpu_rgb_to_nchw_10bit_native_range_quantizes_to_8bit() {
        let mut data = Vec::new();
        for _ in 0..(2 * 2) {
            data.extend_from_slice(&0u16.to_le_bytes());
            data.extend_from_slice(&512u16.to_le_bytes());
            data.extend_from_slice(&1023u16.to_le_bytes());
        }

        let (arr, h, w) = cpu_rgb_to_nchw(&data, 2, 2, 10).unwrap();
        assert_eq!(h, 2);
        assert_eq!(w, 2);

        assert!((arr[[0, 0, 0, 0]] - 0.0).abs() < 0.01);
        assert!((arr[[0, 1, 0, 0]] - 128.0).abs() < 0.01);
        assert!((arr[[0, 2, 0, 0]] - 255.0).abs() < 0.01);
    }

    #[test]
    fn test_cpu_rgb_to_nchw_10bit_wide_range_quantizes_to_8bit() {
        let mut data = Vec::new();
        for _ in 0..(2 * 2) {
            data.extend_from_slice(&0u16.to_le_bytes());
            data.extend_from_slice(&32768u16.to_le_bytes());
            data.extend_from_slice(&65535u16.to_le_bytes());
        }

        let (arr, h, w) = cpu_rgb_to_nchw(&data, 2, 2, 10).unwrap();
        assert_eq!(h, 2);
        assert_eq!(w, 2);

        assert!((arr[[0, 0, 0, 0]] - 0.0).abs() < 0.01);
        assert!((arr[[0, 1, 0, 0]] - 128.0).abs() < 0.01);
        assert!((arr[[0, 2, 0, 0]] - 255.0).abs() < 0.01);
    }

    #[test]
    fn test_pad_nchw_no_padding() {
        let arr = Array4::<f32>::ones((1, 3, 8, 8));
        let padded = pad_nchw(&arr, 8, 8);
        assert_eq!(padded.shape(), &[1, 3, 8, 8]);
    }

    #[test]
    fn test_pad_nchw_needs_padding() {
        let arr = Array4::<f32>::ones((1, 3, 5, 6));
        let padded = pad_nchw(&arr, 5, 6);
        assert_eq!(padded.shape(), &[1, 3, 8, 8]);
        assert_eq!(padded[[0, 0, 0, 0]], 1.0);
        assert_eq!(padded[[0, 0, 4, 5]], 1.0);
        assert_eq!(padded[[0, 0, 5, 0]], padded[[0, 0, 4, 0]]);
        assert_eq!(padded[[0, 0, 6, 0]], padded[[0, 0, 3, 0]]);
        assert_eq!(padded[[0, 0, 7, 0]], padded[[0, 0, 2, 0]]);
    }

    #[test]
    fn test_nchw_to_cpu_rgb_basic() {
        let mut arr = Array4::<f32>::zeros((1, 3, 2, 2));
        arr[[0, 0, 0, 0]] = 255.0;
        arr[[0, 1, 0, 1]] = 128.0;
        arr[[0, 2, 1, 0]] = 64.0;

        let rgb = nchw_to_cpu_rgb(&arr, 2, 2).unwrap();
        assert_eq!(rgb.len(), 12);
        assert_eq!(rgb[0], 255);
        assert_eq!(rgb[1], 0);
        assert_eq!(rgb[2], 0);
        assert_eq!(rgb[3], 0);
        assert_eq!(rgb[4], 128);
        assert_eq!(rgb[5], 0);
        assert_eq!(rgb[6], 0);
        assert_eq!(rgb[7], 0);
        assert_eq!(rgb[8], 64);
    }

    #[test]
    fn test_nchw_to_cpu_rgb_clamping() {
        let mut arr = Array4::<f32>::zeros((1, 3, 1, 1));
        arr[[0, 0, 0, 0]] = 300.0;
        arr[[0, 1, 0, 0]] = -10.0;
        arr[[0, 2, 0, 0]] = 128.5;

        let rgb = nchw_to_cpu_rgb(&arr, 1, 1).unwrap();
        assert_eq!(rgb[0], 255);
        assert_eq!(rgb[1], 0);
        assert_eq!(rgb[2], 128);
    }

    #[test]
    fn test_pad_amount() {
        assert_eq!(pad_amount(4), 0);
        assert_eq!(pad_amount(5), 3);
        assert_eq!(pad_amount(6), 2);
        assert_eq!(pad_amount(7), 1);
        assert_eq!(pad_amount(8), 0);
        assert_eq!(pad_amount(1080), 0);
        assert_eq!(pad_amount(720), 0);
    }

    #[test]
    fn test_roundtrip_conversion() {
        let mut data = vec![0u8; 4 * 4 * 3];
        for i in 0..48 {
            data[i] = (i * 5) as u8;
        }
        let (arr, h, w) = cpu_rgb_to_nchw(&data, 4, 4, 8).unwrap();
        assert_eq!(h, 4);
        assert_eq!(w, 4);
        assert_eq!(arr.shape(), &[1, 3, 4, 4]);

        let restored = nchw_to_cpu_rgb(&arr, 4, 4).unwrap();
        assert_eq!(data, restored);
    }

    #[test]
    fn test_super_res_node_ports() {
        let node = SuperResNode::new();
        assert_eq!(node.node_type(), "SuperResolution");

        let inputs = node.input_ports();
        assert_eq!(inputs.len(), 4);
        assert_eq!(inputs[0].name, "model_path");
        assert_eq!(inputs[0].port_type, PortType::Path);
        assert!(inputs[0].required);

        assert_eq!(inputs[1].name, "scale");
        assert_eq!(inputs[1].port_type, PortType::Int);
        assert!(!inputs[1].required);

        assert_eq!(inputs[2].name, "tile_size");
        assert_eq!(inputs[2].port_type, PortType::Int);
        assert!(!inputs[2].required);

        assert_eq!(inputs[3].name, "backend");
        assert_eq!(inputs[3].port_type, PortType::Str);
        assert!(!inputs[3].required);

        let outputs = node.output_ports();
        assert!(outputs.is_empty());
    }

    #[test]
    fn test_super_res_node_default_backend() {
        let node = SuperResNode::new();
        assert_eq!(node.backend, InferenceBackend::Cuda);
        assert!(node.use_iobinding);
        assert!(node.trt_cache_dir.is_none());
    }

    #[test]
    fn test_execute_missing_model_path() {
        let mut node = SuperResNode::new();
        let ctx = ExecutionContext::default();
        let inputs = HashMap::new();
        let result = node.execute(&inputs, &ctx);
        let err = result.err().expect("should fail");
        assert!(err.to_string().contains("model_path is required"));
    }

    #[test]
    fn test_process_frame_without_session() {
        let mut node = SuperResNode::new();
        let ctx = ExecutionContext::default();
        let frame = Frame::CpuRgb {
            data: vec![0u8; 4 * 4 * 3],
            width: 4,
            height: 4,
            bit_depth: 8,
        };
        let result = node.process_frame(frame, &ctx);
        let err = result.err().expect("should fail");
        assert!(err.to_string().contains("Model not loaded"));
    }

    /// Requires GPU + model file. Run: `cargo test -p videnoa-core -- --ignored`
    #[test]
    #[ignore]
    fn test_full_inference_small_frame() {
        let mut node = SuperResNode::new();
        let ctx = ExecutionContext::default();

        let mut inputs = HashMap::new();
        inputs.insert(
            "model_path".to_string(),
            PortData::Path(std::path::PathBuf::from(
                "models/RealESRGAN_x4plus_anime_6B.onnx",
            )),
        );
        inputs.insert("scale".to_string(), PortData::Int(4));

        node.execute(&inputs, &ctx).expect("execute should succeed");

        let frame = Frame::CpuRgb {
            data: vec![128u8; 8 * 8 * 3],
            width: 8,
            height: 8,
            bit_depth: 8,
        };

        let result = node
            .process_frame(frame, &ctx)
            .expect("inference should succeed");
        match result {
            Frame::CpuRgb {
                width,
                height,
                bit_depth,
                data,
            } => {
                assert_eq!(width, 32);
                assert_eq!(height, 32);
                assert_eq!(bit_depth, 8);
                assert_eq!(data.len(), 32 * 32 * 3);
            }
            _ => panic!("Expected CpuRgb frame"),
        }
    }

    /// Requires GPU + model file. Tests tile-based inference path.
    #[test]
    #[ignore]
    fn test_tiled_inference() {
        let mut node = SuperResNode::new();
        let ctx = ExecutionContext::default();

        let mut inputs = HashMap::new();
        inputs.insert(
            "model_path".to_string(),
            PortData::Path(std::path::PathBuf::from(
                "models/RealESRGAN_x4plus_anime_6B.onnx",
            )),
        );
        inputs.insert("scale".to_string(), PortData::Int(4));
        inputs.insert("tile_size".to_string(), PortData::Int(64));

        node.execute(&inputs, &ctx).expect("execute should succeed");

        let frame = Frame::CpuRgb {
            data: vec![100u8; 64 * 64 * 3],
            width: 64,
            height: 64,
            bit_depth: 8,
        };

        let result = node
            .process_frame(frame, &ctx)
            .expect("tiled inference should succeed");
        match result {
            Frame::CpuRgb {
                width,
                height,
                bit_depth,
                ..
            } => {
                assert_eq!(width, 256);
                assert_eq!(height, 256);
                assert_eq!(bit_depth, 8);
            }
            _ => panic!("Expected CpuRgb frame"),
        }
    }

    #[test]
    fn test_cpu_rgb_to_f16_nchw_basic() {
        let data = vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 128, 128, 128];
        let (arr, h, w) = cpu_rgb_to_f16_nchw(&data, 2, 2).unwrap();
        assert_eq!(h, 2);
        assert_eq!(w, 2);
        assert_eq!(arr.shape(), &[1, 3, 4, 4]);

        let r00 = arr[[0, 0, 0, 0]].to_f32();
        let g00 = arr[[0, 1, 0, 0]].to_f32();
        let b00 = arr[[0, 2, 0, 0]].to_f32();
        assert!((r00 - 1.0).abs() < 0.01, "R(0,0) should be ~1.0, got {r00}");
        assert!((g00 - 0.0).abs() < 0.01, "G(0,0) should be ~0.0, got {g00}");
        assert!((b00 - 0.0).abs() < 0.01, "B(0,0) should be ~0.0, got {b00}");

        let g01 = arr[[0, 1, 0, 1]].to_f32();
        assert!((g01 - 1.0).abs() < 0.01, "G(0,1) should be ~1.0, got {g01}");

        let r11 = arr[[0, 0, 1, 1]].to_f32();
        assert!(
            (r11 - 128.0 / 255.0).abs() < 0.01,
            "R(1,1) should be ~0.502, got {r11}"
        );
    }

    #[test]
    fn test_cpu_rgb_to_f16_nchw_10bit_native_range_quantizes_to_8bit() {
        let mut data = Vec::new();
        for _ in 0..(2 * 2) {
            data.extend_from_slice(&0u16.to_le_bytes());
            data.extend_from_slice(&512u16.to_le_bytes());
            data.extend_from_slice(&1023u16.to_le_bytes());
        }

        let (arr, h, w) = cpu_rgb_to_f16_nchw_into(&data, 2, 2, 10, &mut None).unwrap();
        assert_eq!(h, 2);
        assert_eq!(w, 2);
        assert_eq!(arr.shape(), &[1, 3, 4, 4]);

        let r00 = arr[[0, 0, 0, 0]].to_f32();
        let g00 = arr[[0, 1, 0, 0]].to_f32();
        let b00 = arr[[0, 2, 0, 0]].to_f32();
        assert!((r00 - 0.0).abs() < 0.01);
        assert!((g00 - 128.0 / 255.0).abs() < 0.02);
        assert!((b00 - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_f16_nchw_to_cpu_rgb_basic() {
        let mut arr = ndarray::ArrayD::from_elem(ndarray::IxDyn(&[1, 3, 2, 2]), f16::ZERO);
        let hw = 4usize;
        let slice = arr.as_slice_mut().unwrap();
        slice[0] = f16::from_f32(1.0);
        slice[hw + 1] = f16::from_f32(0.5);
        slice[2 * hw + 2] = f16::from_f32(0.25);

        let rgb = f16_nchw_to_cpu_rgb(&arr, 2, 2).unwrap();
        assert_eq!(rgb.len(), 12);
        assert_eq!(rgb[0], 255);
        assert_eq!(rgb[1], 0);
        assert_eq!(rgb[2], 0);
        assert_eq!(rgb[3], 0);
        assert_eq!(rgb[4], 127); // f16(0.5) * 255.0 = 127.5 → 127
        assert_eq!(rgb[5], 0);
        assert_eq!(rgb[6], 0);
        assert_eq!(rgb[7], 0);
        assert_eq!(rgb[8], 63); // f16(0.25) * 255.0 = 63.75 → 63
    }

    #[test]
    fn test_preprocess_converts_rgb_to_nchw_f16() {
        let data = vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 128, 128, 128];
        let frame = Frame::CpuRgb {
            data,
            width: 2,
            height: 2,
            bit_depth: 8,
        };
        let ctx = ExecutionContext::default();
        let mut stage = SuperResPreprocess { f16_nchw_buf: None };
        let result = stage.process_frame(frame, &ctx).unwrap();

        match result {
            Frame::NchwF16 {
                data,
                height,
                width,
            } => {
                assert_eq!(height, 2);
                assert_eq!(width, 2);
                assert_eq!(data.len(), 3 * 2 * 2);
                let r00 = f16::from_bits(data[0]).to_f32();
                assert!((r00 - 1.0).abs() < 0.01, "R(0,0) should be ~1.0, got {r00}");
                let g01 = f16::from_bits(data[4 + 1]).to_f32();
                assert!((g01 - 1.0).abs() < 0.01, "G(0,1) should be ~1.0, got {g01}");
            }
            _ => panic!("Expected NchwF16"),
        }
    }

    #[test]
    fn test_preprocess_converts_10bit_rgb_to_nchw_f16() {
        let mut data = Vec::new();
        for _ in 0..(2 * 2) {
            data.extend_from_slice(&0u16.to_le_bytes());
            data.extend_from_slice(&512u16.to_le_bytes());
            data.extend_from_slice(&1023u16.to_le_bytes());
        }

        let frame = Frame::CpuRgb {
            data,
            width: 2,
            height: 2,
            bit_depth: 10,
        };
        let ctx = ExecutionContext::default();
        let mut stage = SuperResPreprocess { f16_nchw_buf: None };
        let result = stage.process_frame(frame, &ctx).unwrap();

        match result {
            Frame::NchwF16 {
                data,
                height,
                width,
            } => {
                assert_eq!(height, 2);
                assert_eq!(width, 2);
                assert_eq!(data.len(), 3 * 2 * 2);

                let r00 = f16::from_bits(data[0]).to_f32();
                let g00 = f16::from_bits(data[4]).to_f32();
                let b00 = f16::from_bits(data[8]).to_f32();
                assert!((r00 - 0.0).abs() < 0.01);
                assert!((g00 - 128.0 / 255.0).abs() < 0.02);
                assert!((b00 - 1.0).abs() < 0.01);
            }
            _ => panic!("Expected NchwF16"),
        }
    }

    #[test]
    fn test_postprocess_converts_nchw_f16_to_rgb() {
        let hw = 2 * 2;
        let mut f16_data = vec![f16::ZERO; 3 * hw];
        f16_data[0] = f16::from_f32(1.0);
        f16_data[hw + 1] = f16::from_f32(0.5);
        f16_data[2 * hw + 2] = f16::from_f32(0.25);
        let data: Vec<u16> = f16_data.iter().map(|v| v.to_bits()).collect();

        let frame = Frame::NchwF16 {
            data,
            height: 2,
            width: 2,
        };
        let ctx = ExecutionContext::default();
        let mut stage = SuperResPostprocess;
        let result = stage.process_frame(frame, &ctx).unwrap();

        match result {
            Frame::CpuRgb {
                data,
                width,
                height,
                bit_depth,
            } => {
                assert_eq!(width, 2);
                assert_eq!(height, 2);
                assert_eq!(bit_depth, 8);
                assert_eq!(data.len(), 12);
                assert_eq!(data[0], 255);
                assert_eq!(data[1], 0);
                assert_eq!(data[4], 127);
                assert_eq!(data[8], 63);
            }
            _ => panic!("Expected CpuRgb"),
        }
    }

    #[test]
    fn test_preprocess_postprocess_roundtrip() {
        let mut input_data = vec![0u8; 4 * 4 * 3];
        for i in 0..48 {
            input_data[i] = (i * 5) as u8;
        }
        let frame = Frame::CpuRgb {
            data: input_data.clone(),
            width: 4,
            height: 4,
            bit_depth: 8,
        };
        let ctx = ExecutionContext::default();

        let mut pre = SuperResPreprocess { f16_nchw_buf: None };
        let tensor = pre.process_frame(frame, &ctx).unwrap();
        assert!(matches!(tensor, Frame::NchwF16 { .. }));

        let mut post = SuperResPostprocess;
        let result = post.process_frame(tensor, &ctx).unwrap();
        match result {
            Frame::CpuRgb { data, .. } => {
                for (i, (&orig, &rt)) in input_data.iter().zip(data.iter()).enumerate() {
                    let diff = (orig as i16 - rt as i16).unsigned_abs();
                    assert!(
                        diff <= 1,
                        "Pixel {i}: original={orig}, roundtripped={rt}, diff={diff}"
                    );
                }
            }
            _ => panic!("Expected CpuRgb"),
        }
    }

    #[test]
    fn test_preprocess_rejects_non_rgb() {
        let frame = Frame::NchwF16 {
            data: vec![0; 12],
            height: 2,
            width: 2,
        };
        let ctx = ExecutionContext::default();
        let mut stage = SuperResPreprocess { f16_nchw_buf: None };
        assert!(stage.process_frame(frame, &ctx).is_err());
    }

    #[test]
    fn test_postprocess_rejects_non_f16() {
        let frame = Frame::CpuRgb {
            data: vec![0; 12],
            width: 2,
            height: 2,
            bit_depth: 8,
        };
        let ctx = ExecutionContext::default();
        let mut stage = SuperResPostprocess;
        assert!(stage.process_frame(frame, &ctx).is_err());
    }

    #[test]
    fn test_inference_rejects_non_f16() {
        let node = SuperResNode::new();
        assert!(node.into_micro_stages().is_none());
    }

    #[test]
    fn test_into_micro_stages_returns_none_for_fp32() {
        let node = SuperResNode::new();
        assert!(!node.is_fp16());
        assert!(node.into_micro_stages().is_none());
    }

    #[test]
    fn test_preprocess_buffer_reuse() {
        let ctx = ExecutionContext::default();
        let mut stage = SuperResPreprocess { f16_nchw_buf: None };

        for _ in 0..3 {
            let frame = Frame::CpuRgb {
                data: vec![128u8; 4 * 4 * 3],
                width: 4,
                height: 4,
                bit_depth: 8,
            };
            let result = stage.process_frame(frame, &ctx).unwrap();
            assert!(matches!(result, Frame::NchwF16 { .. }));
        }
        assert!(stage.f16_nchw_buf.is_some());
    }

    #[test]
    fn test_emit_tensor_flag() {
        let mut node = SuperResNode::new();
        assert!(!node.emit_tensor, "emit_tensor should default to false");
        node.set_emit_tensor(true);
        assert!(node.emit_tensor, "emit_tensor should be true after set");
        node.set_emit_tensor(false);
        assert!(!node.emit_tensor, "emit_tensor should be false after unset");
    }

    #[test]
    fn test_f16_roundtrip() {
        let mut data = vec![0u8; 4 * 4 * 3];
        for i in 0..48 {
            data[i] = (i * 5) as u8;
        }
        let (arr, h, w) = cpu_rgb_to_f16_nchw(&data, 4, 4).unwrap();
        assert_eq!(h, 4);
        assert_eq!(w, 4);
        assert_eq!(arr.shape(), &[1, 3, 4, 4]);

        let restored = f16_nchw_to_cpu_rgb(&arr, 4, 4).unwrap();
        assert_eq!(restored.len(), data.len());
        for (i, (&orig, &roundtripped)) in data.iter().zip(restored.iter()).enumerate() {
            let diff = (orig as i16 - roundtripped as i16).unsigned_abs();
            assert!(
                diff <= 1,
                "Pixel {i}: original={orig}, roundtripped={roundtripped}, diff={diff} (max allowed: 1)"
            );
        }
    }
}
