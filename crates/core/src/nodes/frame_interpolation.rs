//! Frame interpolation node: sliding-window pair-based inference via `ort::Session` + CUDA/TensorRT EP.
//!
//! Unlike [`SuperResNode`](super::super_res::SuperResNode) which implements `FrameProcessor`
//! (one frame in → one frame out), frame interpolation operates on *pairs* of consecutive frames
//! and produces N-1 interpolated frames between each pair (where N is the multiplier: any integer ≥ 2).
//!
//! Currently uses RIFE models. Supports two ONNX model formats:
//! - **Three-input** (RIFE v4.6 and earlier): separate `img0`, `img1`, `timestep` tensors
//! - **Concatenated** (RIFE v4.22+): single `input` tensor of shape `[batch, 7, H, W]`
//!   where channels are `[img0_rgb(3) + img1_rgb(3) + timestep_broadcast(1)]`

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use half::f16;
use half::slice::HalfFloatSliceExt;
use ndarray::{s, Array4};
use ort::{
    session::Session,
    value::{Tensor, TensorRef},
};
use tracing::debug;

use crate::node::{ExecutionContext, FrameProcessor, Node, PortDefinition};
use crate::streaming_executor::FrameInterpolator;
use crate::types::{Frame, PortData, PortType};

use crate::nodes::backend::{build_session, InferenceBackend, SessionConfig};

const PAD_ALIGN: usize = 32;

const INPUT_IMG0: &str = "img0";
const INPUT_IMG1: &str = "img1";
const INPUT_TIMESTEP: &str = "timestep";
/// Single concatenated input name for v4.22+ models
const INPUT_CONCAT: &str = "input";
const OUTPUT_NAME: &str = "output";

/// ONNX model input format detected at load time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFormat {
    /// Separate img0, img1, timestep inputs (RIFE v4.6 and earlier)
    ThreeInput,
    /// Single concatenated input of shape [batch, 7, H, W] (RIFE v4.22+)
    Concatenated,
}

/// Frame interpolation node (currently uses RIFE models).
///
/// Processes *pairs* of frames via [`process_frame_pair`](FrameInterpolationNode::process_frame_pair),
/// producing interpolated frames at requested timesteps.
///
/// # Multiplier behaviour
/// - **N×**: inserts N-1 intermediate frames between each pair → multiplies fps by N
/// - e.g. 2× → t=0.5; 3× → t=0.333, 0.667; 4× → t=0.25, 0.5, 0.75
pub struct FrameInterpolationNode {
    session: Option<Arc<Mutex<Session>>>,
    multiplier: u32,
    backend: InferenceBackend,
    use_iobinding: bool,
    trt_cache_dir: Option<PathBuf>,
    model_format: ModelFormat,
    /// Reusable [1,7,padded_h,padded_w] buffer for Concatenated format — avoids ~27ms ndarray::concatenate per inference.
    concat_buf: Option<Array4<f32>>,
    /// Reusable [1,3,padded_h,padded_w] buffer for img0 CpuRgb preprocessing — eliminates ~25MB alloc per call.
    nchw_buf_0: Option<Array4<f32>>,
    /// Reusable [1,3,padded_h,padded_w] buffer for img1 CpuRgb preprocessing — eliminates ~25MB alloc per call.
    nchw_buf_1: Option<Array4<f32>>,
    /// Cached padded NCHW tensor from the previous pair's img1.
    /// In consecutive pairs (frame_N, frame_N+1) → (frame_N+1, frame_N+2),
    /// frame_N+1's NCHW is identical — cache it to skip ~22ms of preprocessing.
    cached_img1_nchw: Option<(Array4<f32>, usize, usize)>, // (padded_array, orig_h, orig_w)
    /// When true, emit Frame::NchwF32 instead of CpuRgb.
    /// Set by compile_graph when downstream node can accept tensor input.
    pub emit_tensor: bool,
}

impl FrameInterpolationNode {
    pub fn new() -> Self {
        Self {
            session: None,
            multiplier: 2,
            backend: InferenceBackend::default(),
            use_iobinding: true,
            trt_cache_dir: None,
            model_format: ModelFormat::ThreeInput,
            concat_buf: None,
            nchw_buf_0: None,
            nchw_buf_1: None,
            cached_img1_nchw: None,
            emit_tensor: false,
        }
    }

    pub fn set_emit_tensor(&mut self, emit: bool) {
        self.emit_tensor = emit;
    }

    pub fn set_trt_cache_dir(&mut self, dir: PathBuf) {
        self.trt_cache_dir = Some(dir);
    }

    pub fn timesteps(&self) -> Vec<f32> {
        timesteps_for_multiplier(self.multiplier)
    }

    /// Returns the detected ONNX model input format.
    pub fn model_format(&self) -> ModelFormat {
        self.model_format
    }

    /// Consume self and split into three micro-stages for pipeline parallelism.
    ///
    /// Returns `Some(FrameInterpolationMicroStages)` for `Concatenated` format models (v4.22+),
    /// or `None` for `ThreeInput` format (which doesn't benefit from this split).
    pub fn into_micro_stages(self) -> Option<FrameInterpolationMicroStages> {
        if self.model_format != ModelFormat::Concatenated {
            return None;
        }
        let session = self.session?;

        Some(FrameInterpolationMicroStages {
            preprocess: FrameInterpolationPreprocess { nchw_buf: None },
            inference: FrameInterpolationInference {
                session,
                use_iobinding: self.use_iobinding,
                concat_buf: self.concat_buf,
                multiplier: self.multiplier,
            },
            postprocess: FrameInterpolationPostprocess { emit_tensor: false },
        })
    }

    fn preprocess_pair(
        &mut self,
        frame0: &Frame,
        frame1: &Frame,
    ) -> Result<(Array4<f32>, Array4<f32>, usize, usize, bool)> {
        match (frame0, frame1) {
            (
                Frame::CpuRgb {
                    data: data0,
                    width: w0,
                    height: h0,
                    bit_depth: bd0,
                },
                Frame::CpuRgb {
                    data: data1,
                    width: w1,
                    height: h1,
                    bit_depth: bd1,
                },
            ) => {
                let orig_h = *h0 as usize;
                let orig_w = *w0 as usize;

                let img0 = match self.cached_img1_nchw.take() {
                    Some((cached_arr, cached_h, cached_w))
                        if cached_h == orig_h && cached_w == orig_w =>
                    {
                        debug!("RIFE cache hit: reusing cached img1 as img0");
                        cached_arr
                    }
                    stale => {
                        if stale.is_some() {
                            debug!(
                                "RIFE cache miss: dimension mismatch, computing img0 from scratch"
                            );
                        } else {
                            debug!("RIFE cache miss: computing img0 from scratch");
                        }
                        cpu_rgb_to_nchw_buffered(data0, *w0, *h0, *bd0, self.nchw_buf_0.take())?
                    }
                };

                let img1 = cpu_rgb_to_nchw_buffered(data1, *w1, *h1, *bd1, self.nchw_buf_1.take())?;
                Ok((img0, img1, orig_h, orig_w, true))
            }
            _ => {
                let (img0, h, w) = frame_to_nchw(frame0)?;
                let (img1, _, _) = frame_to_nchw(frame1)?;
                Ok((img0, img1, h, w, false))
            }
        }
    }

    /// Process a pair of consecutive frames, producing interpolated frames.
    ///
    /// If `scene_change` is `true`, interpolation is skipped and the first frame
    /// is duplicated to fill the intermediate positions instead.
    ///
    /// Returns a `Vec<Frame>` of length `multiplier - 1`.
    pub fn process_frame_pair(
        &mut self,
        frame0: &Frame,
        frame1: &Frame,
        scene_change: bool,
    ) -> Result<Vec<Frame>> {
        let steps = self.timesteps();

        if scene_change {
            if let Frame::CpuRgb {
                data,
                width,
                height,
                bit_depth,
            } = frame1
            {
                let img1_nchw = cpu_rgb_to_nchw_buffered(
                    data,
                    *width,
                    *height,
                    *bit_depth,
                    self.nchw_buf_1.take(),
                )?;
                self.cached_img1_nchw = Some((img1_nchw, *height as usize, *width as usize));
            }
            return duplicate_first_frame(frame0, steps.len());
        }

        let session_arc = self
            .session
            .as_ref()
            .context("Model not loaded — call execute() first")?
            .clone();

        let t_pre = std::time::Instant::now();
        let (img0, img1, orig_h, orig_w, reuse_buffers) = self.preprocess_pair(frame0, frame1)?;
        let preprocess_ms = t_pre.elapsed().as_secs_f64() * 1000.0;

        let use_iobinding = self.use_iobinding;
        let model_format = self.model_format;

        let mut results = Vec::with_capacity(steps.len());
        let mut inference_ms_total = 0.0;
        let mut postprocess_ms_total = 0.0;

        match model_format {
            ModelFormat::Concatenated => {
                let padded_h = img0.shape()[2];
                let padded_w = img0.shape()[3];
                let target_shape = [1, 7, padded_h, padded_w];

                let mut concat = match self.concat_buf.take() {
                    Some(arr) if arr.shape() == target_shape => arr,
                    _ => Array4::<f32>::zeros(target_shape),
                };

                concat.slice_mut(s![.., 0..3, .., ..]).assign(&img0);
                concat.slice_mut(s![.., 3..6, .., ..]).assign(&img1);

                for &t in &steps {
                    concat.slice_mut(s![.., 6..7, .., ..]).fill(t);

                    let t_inf = std::time::Instant::now();
                    let output_raw = run_concatenated(&session_arc, &concat, use_iobinding)?;
                    let output = crop_output(output_raw, &img0, orig_h, orig_w)?;
                    inference_ms_total += t_inf.elapsed().as_secs_f64() * 1000.0;

                    if self.emit_tensor {
                        let cropped_data = output.as_slice().unwrap().to_vec();
                        results.push(Frame::NchwF32 {
                            data: cropped_data,
                            height: orig_h as u32,
                            width: orig_w as u32,
                        });
                    } else {
                        let t_post = std::time::Instant::now();
                        let data = nchw_to_cpu_rgb(&output, orig_h, orig_w)?;
                        postprocess_ms_total += t_post.elapsed().as_secs_f64() * 1000.0;

                        results.push(Frame::CpuRgb {
                            data,
                            width: orig_w as u32,
                            height: orig_h as u32,
                            bit_depth: 8,
                        });
                    }
                }

                self.concat_buf = Some(concat);
            }
            ModelFormat::ThreeInput => {
                for &t in &steps {
                    let t_inf = std::time::Instant::now();
                    let output = run_interpolation(
                        &session_arc,
                        &img0,
                        &img1,
                        t,
                        orig_h,
                        orig_w,
                        use_iobinding,
                        model_format,
                    )?;
                    inference_ms_total += t_inf.elapsed().as_secs_f64() * 1000.0;

                    if self.emit_tensor {
                        let cropped_data = output.as_slice().unwrap().to_vec();
                        results.push(Frame::NchwF32 {
                            data: cropped_data,
                            height: orig_h as u32,
                            width: orig_w as u32,
                        });
                    } else {
                        let t_post = std::time::Instant::now();
                        let data = nchw_to_cpu_rgb(&output, orig_h, orig_w)?;
                        postprocess_ms_total += t_post.elapsed().as_secs_f64() * 1000.0;

                        results.push(Frame::CpuRgb {
                            data,
                            width: orig_w as u32,
                            height: orig_h as u32,
                            bit_depth: 8,
                        });
                    }
                }
            }
        }

        if reuse_buffers {
            self.nchw_buf_0 = Some(img0);
            self.cached_img1_nchw = Some((img1, orig_h, orig_w));
        }

        let total_ms = t_pre.elapsed().as_secs_f64() * 1000.0;
        debug!(
            preprocess_ms = format!("{preprocess_ms:.1}"),
            inference_ms = format!("{inference_ms_total:.1}"),
            postprocess_ms = format!("{postprocess_ms_total:.1}"),
            total_ms = format!("{total_ms:.1}"),
            timesteps = steps.len(),
            "RIFE pair timing"
        );

        Ok(results)
    }
}

impl Default for FrameInterpolationNode {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameInterpolator for FrameInterpolationNode {
    fn stage_name(&self) -> &str {
        "FrameInterpolation"
    }

    fn interpolate(
        &mut self,
        previous: &Frame,
        current: &Frame,
        is_scene_change: bool,
        _ctx: &ExecutionContext,
    ) -> Result<Vec<Frame>> {
        self.process_frame_pair(previous, current, is_scene_change)
    }
}

impl Node for FrameInterpolationNode {
    fn node_type(&self) -> &str {
        "FrameInterpolation"
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
                name: "multiplier".to_string(),
                port_type: PortType::Int,
                required: false,
                default_value: Some(serde_json::json!(2)),
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

        if let Some(PortData::Int(m)) = inputs.get("multiplier") {
            let m = *m as u32;
            if m < 2 {
                bail!("multiplier must be >= 2, got {m}");
            }
            self.multiplier = m;
        }

        if let Some(PortData::Str(b)) = inputs.get("backend") {
            self.backend = InferenceBackend::from_str_lossy(b);
        }

        debug!(
            model = %model_path.display(),
            multiplier = self.multiplier,
            backend = %self.backend,
            use_iobinding = self.use_iobinding,
            "Loading ONNX RIFE model"
        );

        let config = SessionConfig {
            model_path: &model_path,
            backend: &self.backend,
            trt_cache_dir: self.trt_cache_dir.as_deref(),
        };

        let session = build_session(&config)?;

        self.model_format = detect_model_format(&session);
        debug!(
            format = ?self.model_format,
            "Detected RIFE model format"
        );

        self.session = Some(Arc::new(Mutex::new(session)));
        debug!("RIFE model loaded successfully");

        Ok(HashMap::new())
    }
}

// ---------------------------------------------------------------------------
// Micro-stage structs for pipeline parallelism (Concatenated format only)
// ---------------------------------------------------------------------------

pub struct FrameInterpolationMicroStages {
    pub preprocess: FrameInterpolationPreprocess,
    pub inference: FrameInterpolationInference,
    pub postprocess: FrameInterpolationPostprocess,
}

// ---------------------------------------------------------------------------
// Micro-stage 1: Preprocess — CpuRgb → NchwF32 (padded, normalized)
// ---------------------------------------------------------------------------

pub struct FrameInterpolationPreprocess {
    nchw_buf: Option<Array4<f32>>,
}

impl Node for FrameInterpolationPreprocess {
    fn node_type(&self) -> &str {
        "FrameInterpolationPreprocess"
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

impl FrameProcessor for FrameInterpolationPreprocess {
    fn process_frame(&mut self, frame: Frame, _ctx: &ExecutionContext) -> Result<Frame> {
        match frame {
            Frame::CpuRgb {
                data,
                width,
                height,
                bit_depth,
            } => {
                let nchw = cpu_rgb_to_nchw_buffered(
                    &data,
                    width,
                    height,
                    bit_depth,
                    self.nchw_buf.take(),
                )?;
                let padded_data = nchw.as_slice().unwrap().to_vec();
                self.nchw_buf = Some(nchw);

                Ok(Frame::NchwF32 {
                    data: padded_data,
                    height,
                    width,
                })
            }
            Frame::NchwF16 {
                data,
                height,
                width,
            } => {
                // Tensor pass-through from SuperRes: convert f16→f32 and pad
                let (padded, _h, _w) = nchw_f16_to_array4(&data, height as usize, width as usize)?;
                let padded_data = padded.as_slice().unwrap().to_vec();
                Ok(Frame::NchwF32 {
                    data: padded_data,
                    height,
                    width,
                })
            }
            _ => {
                bail!("FrameInterpolationPreprocess: expected CpuRgb or NchwF16, got other variant")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Micro-stage 2: Inference — two NchwF32 → Vec<NchwF32>
// ---------------------------------------------------------------------------

pub struct FrameInterpolationInference {
    session: Arc<Mutex<Session>>,
    use_iobinding: bool,
    concat_buf: Option<Array4<f32>>,
    multiplier: u32,
}

impl Node for FrameInterpolationInference {
    fn node_type(&self) -> &str {
        "FrameInterpolationInference"
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

impl FrameInterpolator for FrameInterpolationInference {
    fn stage_name(&self) -> &str {
        "FrameInterpolationInference"
    }

    fn interpolate(
        &mut self,
        previous: &Frame,
        current: &Frame,
        is_scene_change: bool,
        _ctx: &ExecutionContext,
    ) -> Result<Vec<Frame>> {
        let steps = timesteps_for_multiplier(self.multiplier);

        if is_scene_change {
            return duplicate_first_frame(previous, steps.len());
        }

        let (prev_data, orig_h, orig_w) = extract_nchw_f32(previous, "previous")?;
        let (curr_data, _, _) = extract_nchw_f32(current, "current")?;

        let padded_h = orig_h + pad_amount(orig_h);
        let padded_w = orig_w + pad_amount(orig_w);

        let img0 = Array4::from_shape_vec((1, 3, padded_h, padded_w), prev_data)
            .context("FrameInterpolationInference: failed to reshape previous frame")?;
        let img1 = Array4::from_shape_vec((1, 3, padded_h, padded_w), curr_data)
            .context("FrameInterpolationInference: failed to reshape current frame")?;

        let target_shape = [1, 7, padded_h, padded_w];
        let mut concat = match self.concat_buf.take() {
            Some(arr) if arr.shape() == target_shape => arr,
            _ => Array4::<f32>::zeros(target_shape),
        };

        concat.slice_mut(s![.., 0..3, .., ..]).assign(&img0);
        concat.slice_mut(s![.., 3..6, .., ..]).assign(&img1);

        let mut results = Vec::with_capacity(steps.len());
        for &t in &steps {
            concat.slice_mut(s![.., 6..7, .., ..]).fill(t);

            let output_raw = run_concatenated(&self.session, &concat, self.use_iobinding)?;
            let output = crop_output(output_raw, &img0, orig_h, orig_w)?;

            let cropped_data = output.as_slice().unwrap().to_vec();
            results.push(Frame::NchwF32 {
                data: cropped_data,
                height: orig_h as u32,
                width: orig_w as u32,
            });
        }

        self.concat_buf = Some(concat);
        Ok(results)
    }
}

fn extract_nchw_f32(frame: &Frame, label: &str) -> Result<(Vec<f32>, usize, usize)> {
    let Frame::NchwF32 {
        data,
        height,
        width,
    } = frame
    else {
        bail!("FrameInterpolationInference: expected NchwF32 for {label}, got other variant");
    };
    Ok((data.clone(), *height as usize, *width as usize))
}

// ---------------------------------------------------------------------------
// Micro-stage 3: Postprocess — NchwF32 → CpuRgb
// ---------------------------------------------------------------------------

pub struct FrameInterpolationPostprocess {
    pub emit_tensor: bool,
}

impl Node for FrameInterpolationPostprocess {
    fn node_type(&self) -> &str {
        "FrameInterpolationPostprocess"
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

impl FrameProcessor for FrameInterpolationPostprocess {
    fn process_frame(&mut self, frame: Frame, _ctx: &ExecutionContext) -> Result<Frame> {
        let Frame::NchwF32 {
            data,
            height,
            width,
        } = frame
        else {
            bail!("FrameInterpolationPostprocess: expected NchwF32, got other variant");
        };

        let h = height as usize;
        let w = width as usize;
        let expected_len = 3 * h * w;

        if self.emit_tensor {
            if data.len() == expected_len {
                return Ok(Frame::NchwF32 {
                    data,
                    height,
                    width,
                });
            }
            let padded_h = h + pad_amount(h);
            let padded_w = w + pad_amount(w);
            let padded_expected = 3 * padded_h * padded_w;
            anyhow::ensure!(
                data.len() == padded_expected,
                "FrameInterpolationPostprocess: data length {} doesn't match cropped ({}) or padded ({}) expectations for {}x{}",
                data.len(), expected_len, padded_expected, w, h
            );
            let padded = Array4::from_shape_vec((1, 3, padded_h, padded_w), data)
                .context("FrameInterpolationPostprocess: failed to reshape padded NchwF32 data")?;
            let cropped = padded.slice(ndarray::s![.., .., ..h, ..w]).to_owned();
            let cropped_data = cropped.as_slice().unwrap().to_vec();
            return Ok(Frame::NchwF32 {
                data: cropped_data,
                height,
                width,
            });
        }

        let arr = if data.len() == expected_len {
            Array4::from_shape_vec((1, 3, h, w), data)
                .context("FrameInterpolationPostprocess: failed to reshape cropped NchwF32 data")?
        } else {
            let padded_h = h + pad_amount(h);
            let padded_w = w + pad_amount(w);
            let padded_expected = 3 * padded_h * padded_w;
            anyhow::ensure!(
                data.len() == padded_expected,
                "FrameInterpolationPostprocess: data length {} doesn't match cropped ({}) or padded ({}) expectations for {}x{}",
                data.len(), expected_len, padded_expected, w, h
            );
            let padded = Array4::from_shape_vec((1, 3, padded_h, padded_w), data)
                .context("FrameInterpolationPostprocess: failed to reshape padded NchwF32 data")?;
            padded.slice(ndarray::s![.., .., ..h, ..w]).to_owned()
        };

        let rgb = nchw_to_cpu_rgb(&arr, h, w)?;
        Ok(Frame::CpuRgb {
            data: rgb,
            width: width,
            height: height,
            bit_depth: 8,
        })
    }
}

fn timesteps_for_multiplier(multiplier: u32) -> Vec<f32> {
    let n = multiplier as usize;
    (1..n).map(|i| i as f32 / n as f32).collect()
}

fn frame_to_nchw(frame: &Frame) -> Result<(Array4<f32>, usize, usize)> {
    match frame {
        Frame::CpuRgb {
            data,
            width,
            height,
            bit_depth,
        } => cpu_rgb_to_nchw(data, *width, *height, *bit_depth),
        Frame::NchwF32 {
            data,
            height,
            width,
        } => nchw_f32_to_array4(data, *height as usize, *width as usize),
        Frame::NchwF16 {
            data,
            height,
            width,
        } => nchw_f16_to_array4(data, *height as usize, *width as usize),
        _ => bail!("FrameInterpolationNode does not support this Frame variant"),
    }
}

fn cpu_rgb_to_nchw(
    data: &[u8],
    width: u32,
    height: u32,
    bit_depth: u8,
) -> Result<(Array4<f32>, usize, usize)> {
    let arr = cpu_rgb_to_nchw_buffered(data, width, height, bit_depth, None)?;
    let h = height as usize;
    let w = width as usize;
    Ok((arr, h, w))
}

/// Scatter CpuRgb data directly into a padded-size NCHW buffer, then apply reflection padding
/// in-place. Reuses `buf` if its shape matches `[1, 3, padded_h, padded_w]`, otherwise allocates.
/// Returns the owned buffer so the caller can store it for the next call.
fn cpu_rgb_to_nchw_buffered(
    data: &[u8],
    width: u32,
    height: u32,
    bit_depth: u8,
    buf: Option<Array4<f32>>,
) -> Result<Array4<f32>> {
    let h = height as usize;
    let w = width as usize;
    let padded_h = h + pad_amount(h);
    let padded_w = w + pad_amount(w);
    let target_shape = [1, 3, padded_h, padded_w];

    let mut nchw = match buf {
        Some(arr) if arr.shape() == target_shape => arr,
        _ => Array4::<f32>::zeros(target_shape),
    };

    let padded_hw = padded_h * padded_w;

    // Planes laid out as [R: 0..padded_hw, G: padded_hw..2*padded_hw, B: ...]; row stride = padded_w.
    let nchw_slice = nchw.as_slice_mut().unwrap();
    match bit_depth {
        8 => {
            let expected = h * w * 3;
            if data.len() != expected {
                bail!(
                    "Data length mismatch: expected {} ({}x{}x3), got {}",
                    expected,
                    h,
                    w,
                    data.len()
                );
            }
            for y in 0..h {
                let row_src = y * w * 3;
                let row_dst = y * padded_w;
                for x in 0..w {
                    let src = row_src + x * 3;
                    nchw_slice[row_dst + x] = data[src] as f32 / 255.0;
                    nchw_slice[padded_hw + row_dst + x] = data[src + 1] as f32 / 255.0;
                    nchw_slice[2 * padded_hw + row_dst + x] = data[src + 2] as f32 / 255.0;
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
            for y in 0..h {
                let row_src = y * w * 6;
                let row_dst = y * padded_w;
                for x in 0..w {
                    let src = row_src + x * 6;
                    let r = u16::from_le_bytes([data[src], data[src + 1]]) as f32 / 65535.0;
                    let g = u16::from_le_bytes([data[src + 2], data[src + 3]]) as f32 / 65535.0;
                    let b = u16::from_le_bytes([data[src + 4], data[src + 5]]) as f32 / 65535.0;
                    nchw_slice[row_dst + x] = r;
                    nchw_slice[padded_hw + row_dst + x] = g;
                    nchw_slice[2 * padded_hw + row_dst + x] = b;
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
                let row_src = y * w * 6;
                let row_dst = y * padded_w;
                for x in 0..w {
                    let src = row_src + x * 6;
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
                    nchw_slice[row_dst + x] = r as f32 / 255.0;
                    nchw_slice[padded_hw + row_dst + x] = g as f32 / 255.0;
                    nchw_slice[2 * padded_hw + row_dst + x] = b as f32 / 255.0;
                }
            }
        }
        _ => bail!("Unsupported bit depth: {bit_depth} (expected 8..=16)"),
    }

    // Apply reflection padding in-place (same logic as pad_nchw but operates on the buffer directly)
    let pad_h = padded_h - h;
    let pad_w = padded_w - w;

    if pad_h > 0 || pad_w > 0 {
        // Bottom reflection: copy row (h-1-y) → row (h+y) for the [..w] columns
        for y in 0..pad_h {
            let src_y = h - 1 - y;
            for c in 0..3usize {
                let plane = c * padded_hw;
                let src_off = plane + src_y * padded_w;
                let dst_off = plane + (h + y) * padded_w;
                for x in 0..w {
                    nchw_slice[dst_off + x] = nchw_slice[src_off + x];
                }
            }
        }

        // Right reflection: copy column (w-1-x) → column (w+x) for all rows [..padded_h]
        // (padded_h rows because bottom padding is already filled above)
        for x in 0..pad_w {
            let src_x = w - 1 - x;
            let total_rows = h + pad_h;
            for c in 0..3usize {
                let plane = c * padded_hw;
                for row in 0..total_rows {
                    let row_off = plane + row * padded_w;
                    nchw_slice[row_off + w + x] = nchw_slice[row_off + src_x];
                }
            }
        }
    }

    Ok(nchw)
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

/// Convert NchwF32 data (already in [0,1] range) to Array4<f32> and pad.
/// Skips ÷255 normalization since tensor data is already normalized.
fn nchw_f32_to_array4(data: &[f32], h: usize, w: usize) -> Result<(Array4<f32>, usize, usize)> {
    let expected = 3 * h * w;
    if data.len() != expected {
        bail!(
            "NchwF32 data length mismatch: expected {expected} (3×{h}×{w}), got {}",
            data.len()
        );
    }

    let array = Array4::from_shape_vec((1, 3, h, w), data.to_vec())
        .context("failed to reshape NchwF32 data to [1,3,H,W]")?;

    let padded = pad_nchw(&array, h, w);
    Ok((padded, h, w))
}

/// Convert NchwF16 data (raw u16 bits, already in [0,1] range) to Array4<f32> and pad.
fn nchw_f16_to_array4(data: &[u16], h: usize, w: usize) -> Result<(Array4<f32>, usize, usize)> {
    let expected = 3 * h * w;
    if data.len() != expected {
        bail!(
            "NchwF16 data length mismatch: expected {expected} (3×{h}×{w}), got {}",
            data.len()
        );
    }

    // Convert u16 bits → f16 → f32 using SIMD batch conversion.
    // Safety: f16 is #[repr(transparent)] over u16 in the half crate.
    let f16_slice: &[f16] =
        unsafe { std::slice::from_raw_parts(data.as_ptr() as *const f16, data.len()) };
    let mut f32_data = vec![0.0f32; data.len()];
    f16_slice.convert_to_f32_slice(&mut f32_data);

    nchw_f32_to_array4(&f32_data, h, w)
}

fn pad_nchw(arr: &Array4<f32>, h: usize, w: usize) -> Array4<f32> {
    let pad_h = pad_amount(h);
    let pad_w = pad_amount(w);

    if pad_h == 0 && pad_w == 0 {
        return arr.clone();
    }

    let new_h = h + pad_h;
    let new_w = w + pad_w;
    let mut padded = Array4::<f32>::zeros((1, 3, new_h, new_w));

    padded
        .slice_mut(s![.., .., ..h, ..w])
        .assign(&arr.slice(s![.., .., ..h, ..w]));

    // Bottom reflection padding: row-wise slice assign
    for y in 0..pad_h {
        let src_y = h - 1 - y;
        for c in 0..3usize {
            padded
                .slice_mut(s![0, c, h + y, ..w])
                .assign(&arr.slice(s![0, c, src_y, ..w]));
        }
    }

    // Right reflection padding: column-wise slice assign
    for x in 0..pad_w {
        let src_x = w - 1 - x;
        for c in 0..3usize {
            padded
                .slice_mut(s![0, c, ..h, w + x])
                .assign(&arr.slice(s![0, c, ..h, src_x]));
            if pad_h > 0 {
                padded
                    .slice_mut(s![0, c, h..new_h, w + x])
                    .assign(&arr.slice(s![0, c, (h - pad_h)..h;-1, src_x]));
            }
        }
    }

    padded
}

fn pad_amount(dim: usize) -> usize {
    (PAD_ALIGN - (dim % PAD_ALIGN)) % PAD_ALIGN
}

fn nchw_to_cpu_rgb(arr: &Array4<f32>, out_h: usize, out_w: usize) -> Result<Vec<u8>> {
    let hw = out_h * out_w;
    let mut rgb = vec![0u8; hw * 3];

    // Contiguous-slice gather: read from channel planes [R: 0..hw, G: hw..2hw, B: 2hw..3hw]
    let contiguous = arr.as_standard_layout();
    let arr_slice = contiguous
        .as_slice()
        .expect("standard layout must be contiguous");

    let r_plane = &arr_slice[..hw];
    let g_plane = &arr_slice[hw..2 * hw];
    let b_plane = &arr_slice[2 * hw..3 * hw];

    const CHUNK: usize = 4096;

    let mut offset = 0;
    while offset < hw {
        let len = CHUNK.min(hw - offset);
        let r_chunk = &r_plane[offset..offset + len];
        let g_chunk = &g_plane[offset..offset + len];
        let b_chunk = &b_plane[offset..offset + len];
        let dst = &mut rgb[offset * 3..(offset + len) * 3];

        for j in 0..len {
            let r = (r_chunk[j] * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            let g = (g_chunk[j] * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            let b = (b_chunk[j] * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            dst[j * 3] = r;
            dst[j * 3 + 1] = g;
            dst[j * 3 + 2] = b;
        }

        offset += len;
    }
    Ok(rgb)
}

#[cfg(test)]
fn nchw_to_cpu_rgb_scalar(arr: &Array4<f32>, out_h: usize, out_w: usize) -> Result<Vec<u8>> {
    let hw = out_h * out_w;
    let mut rgb = vec![0u8; hw * 3];
    let contiguous = arr.as_standard_layout();
    let arr_slice = contiguous
        .as_slice()
        .expect("standard layout must be contiguous");

    for i in 0..hw {
        let dst = i * 3;
        rgb[dst] = (arr_slice[i] * 255.0).clamp(0.0, 255.0) as u8;
        rgb[dst + 1] = (arr_slice[hw + i] * 255.0).clamp(0.0, 255.0) as u8;
        rgb[dst + 2] = (arr_slice[2 * hw + i] * 255.0).clamp(0.0, 255.0) as u8;
    }
    Ok(rgb)
}

fn detect_model_format(session: &Session) -> ModelFormat {
    let inputs = session.inputs();
    if inputs.len() == 1 && inputs[0].name() == INPUT_CONCAT {
        ModelFormat::Concatenated
    } else {
        ModelFormat::ThreeInput
    }
}

fn run_interpolation(
    session_arc: &Arc<Mutex<Session>>,
    img0: &Array4<f32>,
    img1: &Array4<f32>,
    timestep: f32,
    orig_h: usize,
    orig_w: usize,
    use_iobinding: bool,
    _model_format: ModelFormat,
) -> Result<Array4<f32>> {
    let t0 = std::time::Instant::now();
    let output_owned = run_three_input(session_arc, img0, img1, timestep, use_iobinding)?;
    let session_run_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let t1 = std::time::Instant::now();
    let result = crop_output(output_owned, img0, orig_h, orig_w)?;
    let crop_ms = t1.elapsed().as_secs_f64() * 1000.0;

    debug!(
        session_run_ms = format!("{session_run_ms:.1}"),
        crop_ms = format!("{crop_ms:.1}"),
        timestep,
        "RIFE inference detail"
    );

    Ok(result)
}

fn run_three_input(
    session_arc: &Arc<Mutex<Session>>,
    img0: &Array4<f32>,
    img1: &Array4<f32>,
    timestep: f32,
    use_iobinding: bool,
) -> Result<ndarray::ArrayD<f32>> {
    let tensor0 = Tensor::from_array(img0.clone())?;
    let tensor1 = Tensor::from_array(img1.clone())?;
    let ts_array = ndarray::Array4::<f32>::from_elem((1, 1, 1, 1), timestep);
    let ts_tensor = Tensor::from_array(ts_array)?;

    let mut session = session_arc.lock().unwrap();
    if use_iobinding {
        let mut binding = session.create_binding()?;
        binding.bind_input(INPUT_IMG0, &tensor0)?;
        binding.bind_input(INPUT_IMG1, &tensor1)?;
        binding.bind_input(INPUT_TIMESTEP, &ts_tensor)?;
        binding.bind_output_to_device(OUTPUT_NAME, &session.allocator().memory_info())?;
        let outputs = session.run_binding(&binding)?;
        let output_view = outputs[OUTPUT_NAME].try_extract_array::<f32>()?;
        Ok(output_view.to_owned())
    } else {
        let outputs = session.run(
            ort::inputs![INPUT_IMG0 => &tensor0, INPUT_IMG1 => &tensor1, INPUT_TIMESTEP => &ts_tensor]
        )?;
        let output_view = outputs[OUTPUT_NAME].try_extract_array::<f32>()?;
        Ok(output_view.to_owned())
    }
}

fn run_concatenated(
    session_arc: &Arc<Mutex<Session>>,
    concat: &Array4<f32>,
    use_iobinding: bool,
) -> Result<ndarray::ArrayD<f32>> {
    let t_tensor = std::time::Instant::now();
    let tensor = TensorRef::from_array_view(concat.view())?;
    let tensor_ms = t_tensor.elapsed().as_secs_f64() * 1000.0;

    let t_lock = std::time::Instant::now();
    let mut session = session_arc.lock().unwrap();
    let lock_ms = t_lock.elapsed().as_secs_f64() * 1000.0;

    let t_run = std::time::Instant::now();
    let result = if use_iobinding {
        let mut binding = session.create_binding()?;
        binding.bind_input(INPUT_CONCAT, &tensor)?;
        binding.bind_output_to_device(OUTPUT_NAME, &session.allocator().memory_info())?;
        let outputs = session.run_binding(&binding)?;
        let output_view = outputs[OUTPUT_NAME].try_extract_array::<f32>()?;
        output_view.to_owned()
    } else {
        let outputs = session.run(ort::inputs![INPUT_CONCAT => tensor])?;
        let output_view = outputs[OUTPUT_NAME].try_extract_array::<f32>()?;
        output_view.to_owned()
    };
    let run_ms = t_run.elapsed().as_secs_f64() * 1000.0;

    debug!(
        tensor_copy_ms = format!("{tensor_ms:.1}"),
        lock_ms = format!("{lock_ms:.1}"),
        session_run_ms = format!("{run_ms:.1}"),
        "RIFE run_concatenated detail"
    );

    Ok(result)
}

fn crop_output(
    output_owned: ndarray::ArrayD<f32>,
    img0: &Array4<f32>,
    orig_h: usize,
    orig_w: usize,
) -> Result<Array4<f32>> {
    let output_4d = output_owned.into_dimensionality::<ndarray::Ix4>()?;

    let padded_h = img0.shape()[2];
    let padded_w = img0.shape()[3];
    let pad_h = padded_h - orig_h;
    let pad_w = padded_w - orig_w;

    if pad_h > 0 || pad_w > 0 {
        Ok(output_4d
            .slice(s![.., .., ..orig_h, ..orig_w])
            .to_owned()
            .into_dimensionality::<ndarray::Ix4>()?)
    } else {
        Ok(output_4d)
    }
}

fn duplicate_first_frame(frame0: &Frame, count: usize) -> Result<Vec<Frame>> {
    match frame0 {
        Frame::CpuRgb {
            data,
            width,
            height,
            bit_depth,
        } => {
            let mut frames = Vec::with_capacity(count);
            for _ in 0..count {
                frames.push(Frame::CpuRgb {
                    data: data.clone(),
                    width: *width,
                    height: *height,
                    bit_depth: *bit_depth,
                });
            }
            Ok(frames)
        }
        Frame::NchwF32 {
            data,
            height,
            width,
        } => {
            let mut frames = Vec::with_capacity(count);
            for _ in 0..count {
                frames.push(Frame::NchwF32 {
                    data: data.clone(),
                    height: *height,
                    width: *width,
                });
            }
            Ok(frames)
        }
        Frame::NchwF16 {
            data,
            height,
            width,
        } => {
            let mut frames = Vec::with_capacity(count);
            for _ in 0..count {
                frames.push(Frame::NchwF16 {
                    data: data.clone(),
                    height: *height,
                    width: *width,
                });
            }
            Ok(frames)
        }
        _ => bail!("FrameInterpolationNode does not support this Frame variant"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timesteps_2x() {
        let steps = timesteps_for_multiplier(2);
        assert_eq!(steps, vec![0.5]);
    }

    #[test]
    fn test_timesteps_3x() {
        let steps = timesteps_for_multiplier(3);
        assert_eq!(steps.len(), 2);
        assert!((steps[0] - 1.0 / 3.0).abs() < 1e-6);
        assert!((steps[1] - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn test_timesteps_4x() {
        let steps = timesteps_for_multiplier(4);
        assert_eq!(steps, vec![0.25, 0.5, 0.75]);
    }

    #[test]
    fn test_timesteps_via_node() {
        let mut node = FrameInterpolationNode::new();
        assert_eq!(node.timesteps(), vec![0.5]);
        node.multiplier = 3;
        assert_eq!(node.timesteps().len(), 2);
        node.multiplier = 4;
        assert_eq!(node.timesteps(), vec![0.25, 0.5, 0.75]);
    }

    #[test]
    fn test_pad_amount_aligned() {
        assert_eq!(pad_amount(32), 0);
        assert_eq!(pad_amount(64), 0);
        assert_eq!(pad_amount(128), 0);
        assert_eq!(pad_amount(1920), 0);
    }

    #[test]
    fn test_pad_amount_unaligned() {
        assert_eq!(pad_amount(1080), 8);
        assert_eq!(pad_amount(720), 16);
        assert_eq!(pad_amount(1), 31);
        assert_eq!(pad_amount(33), 31);
    }

    #[test]
    fn test_pad_nchw_no_padding_needed() {
        let arr = Array4::<f32>::ones((1, 3, 32, 64));
        let padded = pad_nchw(&arr, 32, 64);
        assert_eq!(padded.shape(), &[1, 3, 32, 64]);
    }

    #[test]
    fn test_pad_nchw_needs_padding() {
        let arr = Array4::<f32>::ones((1, 3, 30, 50));
        let padded = pad_nchw(&arr, 30, 50);
        assert_eq!(padded.shape(), &[1, 3, 32, 64]);
        assert_eq!(padded[[0, 0, 0, 0]], 1.0);
        assert_eq!(padded[[0, 0, 29, 49]], 1.0);
        assert_eq!(padded[[0, 0, 30, 0]], padded[[0, 0, 29, 0]]);
        assert_eq!(padded[[0, 0, 31, 0]], padded[[0, 0, 28, 0]]);
    }

    #[test]
    fn test_pad_nchw_1080p() {
        let arr = Array4::<f32>::zeros((1, 3, 1080, 1920));
        let padded = pad_nchw(&arr, 1080, 1920);
        assert_eq!(padded.shape(), &[1, 3, 1088, 1920]);
    }

    #[test]
    fn test_cpu_rgb_to_nchw_normalised() {
        let data = vec![255u8; 32 * 32 * 3];
        let (arr, h, w) = cpu_rgb_to_nchw(&data, 32, 32, 8).unwrap();
        assert_eq!(h, 32);
        assert_eq!(w, 32);
        assert_eq!(arr.shape(), &[1, 3, 32, 32]);
        assert!((arr[[0, 0, 0, 0]] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_cpu_rgb_to_nchw_black() {
        let data = vec![0u8; 32 * 32 * 3];
        let (arr, _, _) = cpu_rgb_to_nchw(&data, 32, 32, 8).unwrap();
        assert!((arr[[0, 0, 0, 0]]).abs() < 1e-5);
    }

    #[test]
    fn test_cpu_rgb_to_nchw_unaligned() {
        let data = vec![128u8; 30 * 50 * 3];
        let (arr, h, w) = cpu_rgb_to_nchw(&data, 50, 30, 8).unwrap();
        assert_eq!(h, 30);
        assert_eq!(w, 50);
        assert_eq!(arr.shape(), &[1, 3, 32, 64]);
        assert!((arr[[0, 0, 0, 0]] - 128.0 / 255.0).abs() < 1e-5);
    }

    #[test]
    fn test_cpu_rgb_to_nchw_16bit() {
        let mut data = Vec::new();
        for _ in 0..(32 * 32 * 3) {
            data.extend_from_slice(&65535u16.to_le_bytes());
        }
        let (arr, h, w) = cpu_rgb_to_nchw(&data, 32, 32, 16).unwrap();
        assert_eq!(h, 32);
        assert_eq!(w, 32);
        assert!((arr[[0, 0, 0, 0]] - 1.0).abs() < 1e-3);
    }

    #[test]
    fn test_cpu_rgb_to_nchw_10bit_native_range_quantizes_to_8bit() {
        let mut data = Vec::new();
        for _ in 0..(32 * 32) {
            data.extend_from_slice(&0u16.to_le_bytes());
            data.extend_from_slice(&512u16.to_le_bytes());
            data.extend_from_slice(&1023u16.to_le_bytes());
        }

        let (arr, h, w) = cpu_rgb_to_nchw(&data, 32, 32, 10).unwrap();
        assert_eq!(h, 32);
        assert_eq!(w, 32);

        assert!((arr[[0, 0, 0, 0]] - 0.0).abs() < 1e-6);
        assert!((arr[[0, 1, 0, 0]] - 128.0 / 255.0).abs() < 1e-3);
        assert!((arr[[0, 2, 0, 0]] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cpu_rgb_to_nchw_10bit_wide_range_quantizes_to_8bit() {
        let mut data = Vec::new();
        for _ in 0..(32 * 32) {
            data.extend_from_slice(&0u16.to_le_bytes());
            data.extend_from_slice(&32768u16.to_le_bytes());
            data.extend_from_slice(&65535u16.to_le_bytes());
        }

        let (arr, h, w) = cpu_rgb_to_nchw(&data, 32, 32, 10).unwrap();
        assert_eq!(h, 32);
        assert_eq!(w, 32);

        assert!((arr[[0, 0, 0, 0]] - 0.0).abs() < 1e-6);
        assert!((arr[[0, 1, 0, 0]] - 128.0 / 255.0).abs() < 1e-3);
        assert!((arr[[0, 2, 0, 0]] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cpu_rgb_to_nchw_data_mismatch() {
        let data = vec![0u8; 10];
        let result = cpu_rgb_to_nchw(&data, 32, 32, 8);
        let err = result.err().expect("should fail");
        assert!(err.to_string().contains("Data length mismatch"));
    }

    #[test]
    fn test_nchw_to_cpu_rgb_normalised() {
        let mut arr = Array4::<f32>::zeros((1, 3, 2, 2));
        arr[[0, 0, 0, 0]] = 1.0;
        arr[[0, 1, 0, 1]] = 0.5;
        arr[[0, 2, 1, 0]] = 0.25;

        let rgb = nchw_to_cpu_rgb(&arr, 2, 2).unwrap();
        assert_eq!(rgb.len(), 12);
        assert_eq!(rgb[0], 255);
        assert_eq!(rgb[1], 0);
        assert_eq!(rgb[2], 0);
        assert_eq!(rgb[3], 0);
        assert!((rgb[4] as i32 - 127).abs() <= 1);
        assert_eq!(rgb[5], 0);
        assert_eq!(rgb[6], 0);
        assert_eq!(rgb[7], 0);
        assert!((rgb[8] as i32 - 64).abs() <= 1);
    }

    #[test]
    fn test_nchw_to_cpu_rgb_clamping() {
        let mut arr = Array4::<f32>::zeros((1, 3, 1, 1));
        arr[[0, 0, 0, 0]] = 1.5;
        arr[[0, 1, 0, 0]] = -0.5;
        arr[[0, 2, 0, 0]] = 0.5;

        let rgb = nchw_to_cpu_rgb(&arr, 1, 1).unwrap();
        assert_eq!(rgb[0], 255);
        assert_eq!(rgb[1], 0);
        assert!((rgb[2] as i32 - 127).abs() <= 1);
    }

    #[test]
    fn test_nchw_to_cpu_rgb_simd_vs_scalar() {
        let h = 256;
        let w = 256;
        let mut arr = Array4::<f32>::zeros((1, 3, h, w));
        for c in 0..3 {
            for y in 0..h {
                for x in 0..w {
                    arr[[0, c, y, x]] = ((c * 1000 + y * w + x) % 256) as f32 / 255.0;
                }
            }
        }

        let optimized = nchw_to_cpu_rgb(&arr, h, w).unwrap();
        let scalar = nchw_to_cpu_rgb_scalar(&arr, h, w).unwrap();
        assert_eq!(optimized.len(), scalar.len());

        for (i, (a, b)) in optimized.iter().zip(scalar.iter()).enumerate() {
            assert!(
                (*a as i32 - *b as i32).abs() <= 1,
                "mismatch at index {i}: optimized={a}, scalar={b}"
            );
        }
    }

    #[test]
    fn test_roundtrip_conversion_aligned() {
        let mut data = vec![0u8; 32 * 32 * 3];
        for i in 0..data.len() {
            data[i] = (i % 256) as u8;
        }
        let (arr, h, w) = cpu_rgb_to_nchw(&data, 32, 32, 8).unwrap();
        let restored = nchw_to_cpu_rgb(&arr, h, w).unwrap();
        for (a, b) in data.iter().zip(restored.iter()) {
            assert!((*a as i32 - *b as i32).abs() <= 1, "mismatch: {a} vs {b}");
        }
    }

    #[test]
    fn test_duplicate_first_frame() {
        let frame = Frame::CpuRgb {
            data: vec![42u8; 32 * 32 * 3],
            width: 32,
            height: 32,
            bit_depth: 8,
        };
        let dupes = duplicate_first_frame(&frame, 3).unwrap();
        assert_eq!(dupes.len(), 3);
        for d in &dupes {
            match d {
                Frame::CpuRgb {
                    data,
                    width,
                    height,
                    bit_depth,
                } => {
                    assert_eq!(*width, 32);
                    assert_eq!(*height, 32);
                    assert_eq!(*bit_depth, 8);
                    assert_eq!(data.len(), 32 * 32 * 3);
                    assert!(data.iter().all(|&b| b == 42));
                }
                _ => panic!("Expected CpuRgb"),
            }
        }
    }

    #[test]
    fn test_scene_change_skips_interpolation() {
        let mut node = FrameInterpolationNode::new();
        let frame0 = Frame::CpuRgb {
            data: vec![100u8; 32 * 32 * 3],
            width: 32,
            height: 32,
            bit_depth: 8,
        };
        let frame1 = Frame::CpuRgb {
            data: vec![200u8; 32 * 32 * 3],
            width: 32,
            height: 32,
            bit_depth: 8,
        };
        let result = node.process_frame_pair(&frame0, &frame1, true).unwrap();
        assert_eq!(result.len(), 1);
        match &result[0] {
            Frame::CpuRgb { data, .. } => {
                assert!(data.iter().all(|&b| b == 100));
            }
            _ => panic!("Expected CpuRgb"),
        }
    }

    #[test]
    fn test_scene_change_4x() {
        let mut node = FrameInterpolationNode::new();
        node.multiplier = 4;
        let frame0 = Frame::CpuRgb {
            data: vec![50u8; 32 * 32 * 3],
            width: 32,
            height: 32,
            bit_depth: 8,
        };
        let frame1 = Frame::CpuRgb {
            data: vec![200u8; 32 * 32 * 3],
            width: 32,
            height: 32,
            bit_depth: 8,
        };
        let result = node.process_frame_pair(&frame0, &frame1, true).unwrap();
        assert_eq!(result.len(), 3);
        for f in &result {
            match f {
                Frame::CpuRgb { data, .. } => assert!(data.iter().all(|&b| b == 50)),
                _ => panic!("Expected CpuRgb"),
            }
        }
    }

    #[test]
    fn test_scene_change_preserves_cache() {
        let mut node = FrameInterpolationNode::new();
        let frame0 = Frame::CpuRgb {
            data: vec![128u8; 64 * 48 * 3],
            width: 64,
            height: 48,
            bit_depth: 8,
        };
        let frame1 = Frame::CpuRgb {
            data: vec![200u8; 64 * 48 * 3],
            width: 64,
            height: 48,
            bit_depth: 8,
        };

        let result = node.process_frame_pair(&frame0, &frame1, true).unwrap();
        assert_eq!(result.len(), 1);

        assert!(
            node.cached_img1_nchw.is_some(),
            "img1 should be cached after scene_change"
        );
        let (cached, h, w) = node.cached_img1_nchw.as_ref().unwrap();
        assert_eq!(*h, 48);
        assert_eq!(*w, 64);
        assert_eq!(cached.shape(), &[1, 3, 64, 64]);
    }

    #[test]
    fn test_fi_node_ports() {
        let node = FrameInterpolationNode::new();
        assert_eq!(node.node_type(), "FrameInterpolation");

        let inputs = node.input_ports();
        assert_eq!(inputs.len(), 3);
        assert_eq!(inputs[0].name, "model_path");
        assert_eq!(inputs[0].port_type, PortType::Path);
        assert!(inputs[0].required);

        assert_eq!(inputs[1].name, "multiplier");
        assert_eq!(inputs[1].port_type, PortType::Int);
        assert!(!inputs[1].required);
        assert_eq!(inputs[1].default_value, Some(serde_json::json!(2)));

        assert_eq!(inputs[2].name, "backend");
        assert_eq!(inputs[2].port_type, PortType::Str);
        assert!(!inputs[2].required);

        let outputs = node.output_ports();
        assert!(outputs.is_empty());
    }

    #[test]
    fn test_fi_node_default_backend() {
        let node = FrameInterpolationNode::new();
        assert_eq!(node.backend, InferenceBackend::Cuda);
        assert!(node.use_iobinding);
        assert!(node.trt_cache_dir.is_none());
    }

    #[test]
    fn test_frame_interpolator_stage_name() {
        let node = FrameInterpolationNode::new();
        assert_eq!(node.stage_name(), "FrameInterpolation");
    }

    #[test]
    fn test_frame_interpolator_delegates_scene_change_path() {
        let mut node = FrameInterpolationNode::new();
        let ctx = ExecutionContext::default();
        let frame0 = Frame::CpuRgb {
            data: vec![123u8; 32 * 32 * 3],
            width: 32,
            height: 32,
            bit_depth: 8,
        };
        let frame1 = Frame::CpuRgb {
            data: vec![200u8; 32 * 32 * 3],
            width: 32,
            height: 32,
            bit_depth: 8,
        };

        let result = node
            .interpolate(&frame0, &frame1, true, &ctx)
            .expect("interpolate should delegate to scene-change duplicate path");
        assert_eq!(result.len(), 1);
        match &result[0] {
            Frame::CpuRgb { data, .. } => assert!(data.iter().all(|&b| b == 123)),
            _ => panic!("Expected CpuRgb"),
        }
    }

    #[test]
    #[ignore]
    fn test_frame_interpolator_inference_path_compiles() {
        let mut node = FrameInterpolationNode::new();
        let ctx = ExecutionContext::default();
        let frame0 = Frame::CpuRgb {
            data: vec![0u8; 32 * 32 * 3],
            width: 32,
            height: 32,
            bit_depth: 8,
        };
        let frame1 = Frame::CpuRgb {
            data: vec![255u8; 32 * 32 * 3],
            width: 32,
            height: 32,
            bit_depth: 8,
        };

        let _ = node.interpolate(&frame0, &frame1, false, &ctx);
    }

    #[test]
    fn test_execute_missing_model_path() {
        let mut node = FrameInterpolationNode::new();
        let ctx = ExecutionContext::default();
        let inputs = HashMap::new();
        let result = node.execute(&inputs, &ctx);
        let err = result.err().expect("should fail");
        assert!(err.to_string().contains("model_path is required"));
    }

    #[test]
    fn test_execute_multiplier_below_minimum() {
        let mut node = FrameInterpolationNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert(
            "model_path".to_string(),
            PortData::Path(std::env::temp_dir().join("model.onnx")),
        );

        inputs.insert("multiplier".to_string(), PortData::Int(1));
        let result = node.execute(&inputs, &ctx);
        let err = result.err().expect("multiplier=1 should fail");
        assert!(err.to_string().contains("multiplier must be >= 2"));

        inputs.insert("multiplier".to_string(), PortData::Int(0));
        let result = node.execute(&inputs, &ctx);
        let err = result.err().expect("multiplier=0 should fail");
        assert!(err.to_string().contains("multiplier must be >= 2"));
    }

    #[test]
    fn test_process_frame_pair_without_session() {
        let mut node = FrameInterpolationNode::new();
        let frame0 = Frame::CpuRgb {
            data: vec![0u8; 32 * 32 * 3],
            width: 32,
            height: 32,
            bit_depth: 8,
        };
        let frame1 = Frame::CpuRgb {
            data: vec![255u8; 32 * 32 * 3],
            width: 32,
            height: 32,
            bit_depth: 8,
        };
        let result = node.process_frame_pair(&frame0, &frame1, false);
        let err = result.err().expect("should fail");
        assert!(err.to_string().contains("Model not loaded"));
    }

    #[test]
    #[ignore]
    fn test_fi_full_inference() {
        let mut node = FrameInterpolationNode::new();
        let ctx = ExecutionContext::default();

        let mut inputs = HashMap::new();
        inputs.insert(
            "model_path".to_string(),
            PortData::Path(std::path::PathBuf::from("models/rife-v4.6.onnx")),
        );
        inputs.insert("multiplier".to_string(), PortData::Int(2));

        node.execute(&inputs, &ctx).expect("execute should succeed");

        let frame0 = Frame::CpuRgb {
            data: vec![100u8; 64 * 64 * 3],
            width: 64,
            height: 64,
            bit_depth: 8,
        };
        let frame1 = Frame::CpuRgb {
            data: vec![200u8; 64 * 64 * 3],
            width: 64,
            height: 64,
            bit_depth: 8,
        };

        let result = node
            .process_frame_pair(&frame0, &frame1, false)
            .expect("interpolation should succeed");
        assert_eq!(result.len(), 1);
        match &result[0] {
            Frame::CpuRgb {
                width,
                height,
                bit_depth,
                data,
            } => {
                assert_eq!(*width, 64);
                assert_eq!(*height, 64);
                assert_eq!(*bit_depth, 8);
                assert_eq!(data.len(), 64 * 64 * 3);
            }
            _ => panic!("Expected CpuRgb frame"),
        }
    }

    #[test]
    fn test_frame_to_nchw_from_nchw_f32_aligned() {
        let h = 32usize;
        let w = 32usize;
        let data: Vec<f32> = (0..3 * h * w)
            .map(|i| i as f32 / (3 * h * w) as f32)
            .collect();
        let frame = Frame::NchwF32 {
            data: data.clone(),
            height: h as u32,
            width: w as u32,
        };

        let (array, orig_h, orig_w) = frame_to_nchw(&frame).unwrap();
        assert_eq!(orig_h, h);
        assert_eq!(orig_w, w);
        assert_eq!(array.shape(), &[1, 3, h, w]);
        assert!((array[[0, 0, 0, 0]] - data[0]).abs() < 1e-6);
        assert!((array[[0, 0, 1, 0]] - data[w]).abs() < 1e-6);
    }

    #[test]
    fn test_frame_to_nchw_from_nchw_f32_unaligned() {
        let h = 30usize;
        let w = 50usize;
        let data: Vec<f32> = (0..3 * h * w).map(|i| i as f32 / 1000.0).collect();
        let frame = Frame::NchwF32 {
            data: data.clone(),
            height: h as u32,
            width: w as u32,
        };

        let (array, orig_h, orig_w) = frame_to_nchw(&frame).unwrap();
        assert_eq!(orig_h, 30);
        assert_eq!(orig_w, 50);
        assert_eq!(array.shape(), &[1, 3, 32, 64]);
        assert!((array[[0, 0, 0, 0]] - data[0]).abs() < 1e-6);
    }

    #[test]
    fn test_frame_to_nchw_from_nchw_f32_length_mismatch() {
        let frame = Frame::NchwF32 {
            data: vec![0.0f32; 10],
            height: 32,
            width: 32,
        };
        let err = frame_to_nchw(&frame).unwrap_err();
        assert!(err.to_string().contains("NchwF32 data length mismatch"));
    }

    #[test]
    fn test_frame_to_nchw_from_nchw_f16() {
        let h = 32usize;
        let w = 32usize;
        let f32_data: Vec<f32> = (0..3 * h * w)
            .map(|i| i as f32 / (3 * h * w) as f32)
            .collect();
        let u16_data: Vec<u16> = f32_data
            .iter()
            .map(|&v| half::f16::from_f32(v).to_bits())
            .collect();
        let frame = Frame::NchwF16 {
            data: u16_data,
            height: h as u32,
            width: w as u32,
        };

        let (array, orig_h, orig_w) = frame_to_nchw(&frame).unwrap();
        assert_eq!(orig_h, h);
        assert_eq!(orig_w, w);
        assert_eq!(array.shape(), &[1, 3, h, w]);
        assert!((array[[0, 0, 0, 0]] - f32_data[0]).abs() < 0.01);
    }

    #[test]
    fn test_frame_to_nchw_from_nchw_f16_unaligned() {
        let h = 30usize;
        let w = 50usize;
        let f32_data: Vec<f32> = (0..3 * h * w).map(|i| i as f32 / 1000.0).collect();
        let u16_data: Vec<u16> = f32_data
            .iter()
            .map(|&v| half::f16::from_f32(v).to_bits())
            .collect();
        let frame = Frame::NchwF16 {
            data: u16_data,
            height: h as u32,
            width: w as u32,
        };

        let (array, orig_h, orig_w) = frame_to_nchw(&frame).unwrap();
        assert_eq!(orig_h, 30);
        assert_eq!(orig_w, 50);
        assert_eq!(array.shape(), &[1, 3, 32, 64]);
    }

    #[test]
    fn test_frame_to_nchw_from_nchw_f16_length_mismatch() {
        let frame = Frame::NchwF16 {
            data: vec![0u16; 10],
            height: 32,
            width: 32,
        };
        let err = frame_to_nchw(&frame).unwrap_err();
        assert!(err.to_string().contains("NchwF16 data length mismatch"));
    }

    #[test]
    fn test_duplicate_first_frame_nchw_f32() {
        let frame = Frame::NchwF32 {
            data: vec![0.5f32; 3 * 4 * 4],
            height: 4,
            width: 4,
        };
        let dupes = duplicate_first_frame(&frame, 3).unwrap();
        assert_eq!(dupes.len(), 3);
        for f in &dupes {
            match f {
                Frame::NchwF32 {
                    data,
                    height,
                    width,
                } => {
                    assert_eq!(data.len(), 3 * 4 * 4);
                    assert_eq!(*height, 4);
                    assert_eq!(*width, 4);
                    assert!(data.iter().all(|&v| (v - 0.5).abs() < 1e-6));
                }
                _ => panic!("expected NchwF32"),
            }
        }
    }

    #[test]
    fn test_duplicate_first_frame_nchw_f16() {
        let frame = Frame::NchwF16 {
            data: vec![15360u16; 3 * 4 * 4], // f16 bits for 1.0
            height: 4,
            width: 4,
        };
        let dupes = duplicate_first_frame(&frame, 2).unwrap();
        assert_eq!(dupes.len(), 2);
        for f in &dupes {
            match f {
                Frame::NchwF16 {
                    data,
                    height,
                    width,
                } => {
                    assert_eq!(data.len(), 3 * 4 * 4);
                    assert_eq!(*height, 4);
                    assert_eq!(*width, 4);
                    assert!(data.iter().all(|&v| v == 15360));
                }
                _ => panic!("expected NchwF16"),
            }
        }
    }

    #[test]
    fn test_roundtrip_conversion_1080p() {
        let h = 1080usize;
        let w = 1920usize;
        let mut data = vec![0u8; h * w * 3];
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = ((i * 7 + 13) % 256) as u8;
        }

        let (padded_arr, orig_h, orig_w) = cpu_rgb_to_nchw(&data, w as u32, h as u32, 8).unwrap();
        assert_eq!(orig_h, h);
        assert_eq!(orig_w, w);
        assert_eq!(padded_arr.shape(), &[1, 3, 1088, 1920]);

        let cropped = padded_arr
            .slice(s![.., .., ..h, ..w])
            .to_owned()
            .into_dimensionality::<ndarray::Ix4>()
            .unwrap();

        let restored = nchw_to_cpu_rgb(&cropped, h, w).unwrap();
        assert_eq!(restored.len(), data.len());
        for (i, (&a, &b)) in data.iter().zip(restored.iter()).enumerate() {
            assert!(
                (a as i32 - b as i32).abs() <= 1,
                "mismatch at byte {i}: original={a}, restored={b}"
            );
        }
    }

    #[test]
    fn test_optimized_nchw_matches_original() {
        let h = 32usize;
        let w = 32usize;
        let data: Vec<u8> = (0..h * w * 3).map(|i| (i % 256) as u8).collect();

        let (padded, _, _) = cpu_rgb_to_nchw(&data, w as u32, h as u32, 8).unwrap();
        assert_eq!(padded.shape(), &[1, 3, h, w]);
        let arr = padded.slice(s![0, .., ..h, ..w]);

        for y in 0..h {
            for x in 0..w {
                let src = (y * w + x) * 3;
                let r = data[src] as f32 / 255.0;
                let g = data[src + 1] as f32 / 255.0;
                let b = data[src + 2] as f32 / 255.0;
                assert!(
                    (arr[[0, y, x]] - r).abs() < 1e-6,
                    "R mismatch at ({y},{x}): expected {r}, got {}",
                    arr[[0, y, x]]
                );
                assert!(
                    (arr[[1, y, x]] - g).abs() < 1e-6,
                    "G mismatch at ({y},{x}): expected {g}, got {}",
                    arr[[1, y, x]]
                );
                assert!(
                    (arr[[2, y, x]] - b).abs() < 1e-6,
                    "B mismatch at ({y},{x}): expected {b}, got {}",
                    arr[[2, y, x]]
                );
            }
        }
    }

    #[test]
    fn test_pad_nchw_reflection_correctness() {
        let h = 30usize;
        let w = 50usize;
        let mut arr = Array4::<f32>::zeros((1, 3, h, w));
        for c in 0..3usize {
            for y in 0..h {
                for x in 0..w {
                    arr[[0, c, y, x]] = (c * 10000 + y * 100 + x) as f32;
                }
            }
        }

        let padded = pad_nchw(&arr, h, w);
        let pad_h = pad_amount(h);
        let pad_w = pad_amount(w);
        assert_eq!(pad_h, 2);
        assert_eq!(pad_w, 14);
        assert_eq!(padded.shape(), &[1, 3, 32, 64]);

        for c in 0..3usize {
            for y in 0..h {
                for x in 0..w {
                    assert_eq!(
                        padded[[0, c, y, x]],
                        arr[[0, c, y, x]],
                        "interior mismatch at c={c}, y={y}, x={x}"
                    );
                }
            }
        }

        for c in 0..3usize {
            for y in 0..pad_h {
                let src_y = h - 1 - y;
                for x in 0..w {
                    assert_eq!(
                        padded[[0, c, h + y, x]],
                        arr[[0, c, src_y, x]],
                        "bottom reflection mismatch at c={c}, y={y}, x={x}"
                    );
                }
            }
        }

        for c in 0..3usize {
            for x in 0..pad_w {
                let src_x = w - 1 - x;
                for y in 0..h {
                    assert_eq!(
                        padded[[0, c, y, w + x]],
                        arr[[0, c, y, src_x]],
                        "right reflection mismatch at c={c}, y={y}, x={x}"
                    );
                }
                for y in 0..pad_h {
                    let src_y = h - 1 - y;
                    assert_eq!(
                        padded[[0, c, h + y, w + x]],
                        arr[[0, c, src_y, src_x]],
                        "corner reflection mismatch at c={c}, y={y}, x={x}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_scene_change_nchw_f32() {
        let mut node = FrameInterpolationNode::new();
        let frame0 = Frame::NchwF32 {
            data: vec![0.5f32; 3 * 32 * 32],
            height: 32,
            width: 32,
        };
        let frame1 = Frame::NchwF32 {
            data: vec![0.9f32; 3 * 32 * 32],
            height: 32,
            width: 32,
        };
        let result = node.process_frame_pair(&frame0, &frame1, true).unwrap();
        assert_eq!(result.len(), 1);
        match &result[0] {
            Frame::NchwF32 { data, .. } => {
                assert!(data.iter().all(|&v| (v - 0.5).abs() < 1e-6));
            }
            _ => panic!("expected NchwF32"),
        }
    }

    #[test]
    fn test_concat_buffer_shape_and_values() {
        let h = 32usize;
        let w = 32usize;
        let img0 = Array4::from_shape_fn((1, 3, h, w), |(_, c, y, x)| {
            (c * 1000 + y * 10 + x) as f32 / 10000.0
        });
        let img1 = Array4::from_shape_fn((1, 3, h, w), |(_, c, y, x)| {
            1.0 - (c * 1000 + y * 10 + x) as f32 / 10000.0
        });
        let timestep = 0.5f32;

        let mut concat = Array4::<f32>::zeros([1, 7, h, w]);
        concat.slice_mut(s![.., 0..3, .., ..]).assign(&img0);
        concat.slice_mut(s![.., 3..6, .., ..]).assign(&img1);
        concat.slice_mut(s![.., 6..7, .., ..]).fill(timestep);

        assert_eq!(concat.shape(), &[1, 7, h, w]);

        for c in 0..3 {
            for y in 0..h {
                for x in 0..w {
                    assert_eq!(
                        concat[[0, c, y, x]],
                        img0[[0, c, y, x]],
                        "img0 channel {c} mismatch at ({y},{x})"
                    );
                    assert_eq!(
                        concat[[0, 3 + c, y, x]],
                        img1[[0, c, y, x]],
                        "img1 channel {c} mismatch at ({y},{x})"
                    );
                }
            }
        }

        for y in 0..h {
            for x in 0..w {
                assert!(
                    (concat[[0, 6, y, x]] - timestep).abs() < 1e-7,
                    "timestep channel mismatch at ({y},{x})"
                );
            }
        }
    }

    #[test]
    fn test_buffer_reuse_across_pairs() {
        let mut node = FrameInterpolationNode::new();
        assert!(node.concat_buf.is_none());

        let frame0 = Frame::CpuRgb {
            data: vec![100u8; 32 * 32 * 3],
            width: 32,
            height: 32,
            bit_depth: 8,
        };
        let frame1 = Frame::CpuRgb {
            data: vec![200u8; 32 * 32 * 3],
            width: 32,
            height: 32,
            bit_depth: 8,
        };

        let result = node.process_frame_pair(&frame0, &frame1, true).unwrap();
        assert_eq!(result.len(), 1);
        assert!(
            node.concat_buf.is_none(),
            "scene_change path should not populate concat_buf"
        );

        node.multiplier = 4;
        let result = node.process_frame_pair(&frame0, &frame1, true).unwrap();
        assert_eq!(result.len(), 3);
        assert!(
            node.concat_buf.is_none(),
            "scene_change path should not populate concat_buf"
        );

        node.model_format = ModelFormat::Concatenated;
        let result = node.process_frame_pair(&frame0, &frame1, true).unwrap();
        assert_eq!(result.len(), 3);
        assert!(
            node.concat_buf.is_none(),
            "scene_change path should not populate concat_buf even in Concatenated mode"
        );
    }

    #[test]
    fn test_fi_preprocess() {
        let ctx = ExecutionContext::default();
        let mut pre = FrameInterpolationPreprocess { nchw_buf: None };

        let frame = Frame::CpuRgb {
            data: vec![128u8; 32 * 32 * 3],
            width: 32,
            height: 32,
            bit_depth: 8,
        };

        let result = pre.process_frame(frame, &ctx).unwrap();
        match result {
            Frame::NchwF32 {
                data,
                height,
                width,
            } => {
                assert_eq!(height, 32);
                assert_eq!(width, 32);
                let padded_h = 32 + pad_amount(32);
                let padded_w = 32 + pad_amount(32);
                assert_eq!(data.len(), 3 * padded_h * padded_w);
                let expected = 128.0 / 255.0;
                assert!(
                    (data[0] - expected).abs() < 1e-5,
                    "first pixel should be ~{expected}, got {}",
                    data[0]
                );
            }
            _ => panic!("expected NchwF32"),
        }

        assert!(
            pre.nchw_buf.is_some(),
            "buffer should be retained for reuse"
        );
    }

    #[test]
    fn test_fi_postprocess() {
        let ctx = ExecutionContext::default();
        let mut post = FrameInterpolationPostprocess { emit_tensor: false };

        let h = 2usize;
        let w = 2usize;
        let mut nchw_data = vec![0.0f32; 3 * h * w];
        nchw_data[0] = 1.0; // R[0,0]
        nchw_data[h * w] = 0.5; // G[0,0]
        nchw_data[2 * h * w] = 0.25; // B[0,0]

        let frame = Frame::NchwF32 {
            data: nchw_data,
            height: h as u32,
            width: w as u32,
        };

        let result = post.process_frame(frame, &ctx).unwrap();
        match result {
            Frame::CpuRgb {
                data,
                width,
                height,
                bit_depth,
            } => {
                assert_eq!(width, w as u32);
                assert_eq!(height, h as u32);
                assert_eq!(bit_depth, 8);
                assert_eq!(data.len(), h * w * 3);
                assert_eq!(data[0], 255); // R
                assert!((data[1] as i32 - 127).abs() <= 1); // G
                assert!((data[2] as i32 - 64).abs() <= 1); // B
            }
            _ => panic!("expected CpuRgb"),
        }
    }

    #[test]
    fn test_into_micro_stages_three_input_returns_none() {
        let mut node = FrameInterpolationNode::new();
        node.model_format = ModelFormat::ThreeInput;
        assert!(node.into_micro_stages().is_none());
    }

    #[test]
    fn test_into_micro_stages_no_session_returns_none() {
        let mut node = FrameInterpolationNode::new();
        node.model_format = ModelFormat::Concatenated;
        assert!(node.into_micro_stages().is_none());
    }

    #[test]
    fn test_model_format_getter() {
        let mut node = FrameInterpolationNode::new();
        assert_eq!(node.model_format(), ModelFormat::ThreeInput);
        node.model_format = ModelFormat::Concatenated;
        assert_eq!(node.model_format(), ModelFormat::Concatenated);
    }

    #[test]
    fn test_fi_postprocess_padded_input() {
        let orig_h: usize = 50;
        let orig_w: usize = 50;
        let padded_h = orig_h + pad_amount(orig_h);
        let padded_w = orig_w + pad_amount(orig_w);

        let mut padded_data = vec![0.0f32; 3 * padded_h * padded_w];
        for c in 0..3 {
            for y in 0..orig_h {
                for x in 0..orig_w {
                    padded_data[c * padded_h * padded_w + y * padded_w + x] = 0.5;
                }
            }
        }

        let frame = Frame::NchwF32 {
            data: padded_data,
            height: orig_h as u32,
            width: orig_w as u32,
        };

        let ctx = ExecutionContext::default();
        let mut postprocess = FrameInterpolationPostprocess { emit_tensor: false };
        let result = postprocess.process_frame(frame, &ctx).unwrap();

        match result {
            Frame::CpuRgb {
                data,
                width,
                height,
                bit_depth,
            } => {
                assert_eq!(height, orig_h as u32);
                assert_eq!(width, orig_w as u32);
                assert_eq!(bit_depth, 8);
                assert_eq!(data.len(), orig_h * orig_w * 3);
                let expected_val = (0.5f32 * 255.0).round() as u8;
                for (i, &b) in data.iter().enumerate() {
                    assert!(
                        (b as i32 - expected_val as i32).abs() <= 1,
                        "pixel mismatch at byte {i}: got {b}, expected ~{expected_val}"
                    );
                }
            }
            _ => panic!("Expected CpuRgb"),
        }
    }

    #[test]
    fn test_fi_postprocess_cropped_input() {
        let h: usize = 50;
        let w: usize = 50;
        let data: Vec<f32> = (0..(3 * h * w))
            .map(|i| (i as f32) / (3 * h * w) as f32)
            .collect();

        let frame = Frame::NchwF32 {
            data: data.clone(),
            height: h as u32,
            width: w as u32,
        };

        let ctx = ExecutionContext::default();
        let mut postprocess = FrameInterpolationPostprocess { emit_tensor: false };
        let result = postprocess.process_frame(frame, &ctx).unwrap();

        match result {
            Frame::CpuRgb {
                data: rgb,
                width,
                height,
                bit_depth,
            } => {
                assert_eq!(height, h as u32);
                assert_eq!(width, w as u32);
                assert_eq!(bit_depth, 8);
                assert_eq!(rgb.len(), h * w * 3);
                let hw = h * w;
                let r0 = (data[0] * 255.0).clamp(0.0, 255.0) as u8;
                let g0 = (data[hw] * 255.0).clamp(0.0, 255.0) as u8;
                let b0 = (data[2 * hw] * 255.0).clamp(0.0, 255.0) as u8;
                assert!((rgb[0] as i32 - r0 as i32).abs() <= 1);
                assert!((rgb[1] as i32 - g0 as i32).abs() <= 1);
                assert!((rgb[2] as i32 - b0 as i32).abs() <= 1);
            }
            _ => panic!("Expected CpuRgb"),
        }
    }
}
