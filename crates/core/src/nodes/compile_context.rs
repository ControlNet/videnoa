use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};

use crate::compile::{CompileContext, DecoderResult};
use crate::frame_pool::FramePool;
use crate::graph::PipelineGraph;
use crate::node::{ExecutionContext, FrameProcessor, Node, PortDefinition};
use crate::streaming_executor::{FrameInterpolator, FrameSink, PipelineStage};
use crate::types::{Frame, PortData};

use crate::nodes::backend::{
    model_trt_cache_dir, InferenceBackend, TrtCacheIdentity, TrtTileIdentity,
};
use crate::nodes::frame_interpolation::{
    FrameInterpolationNode, FrameInterpolationPostprocess, ModelFormat,
};
use crate::nodes::rescale::rescaled_dimensions;
use crate::nodes::resize::ResizeAlgorithm;
use crate::nodes::super_res::{SuperResNode, SuperResOutputMode, SuperResPostprocess};
use crate::nodes::video_input::{extract_metadata, run_ffprobe, VideoDecoder};
use crate::nodes::video_output::{
    compatible_pixel_format, default_pixel_format_for_codec, optional_positive_dimension,
    EncoderConfig, OutputScale, VideoEncoder,
};

/// Processing node types the video job pipeline can compile.
const SUPPORTED_VIDEO_PROCESSING_NODES: [&str; 4] =
    ["SuperResolution", "FrameInterpolation", "Resize", "Rescale"];

fn is_output_scale_node(node_type: &str) -> bool {
    matches!(node_type, "Resize" | "Rescale")
}

/// Check the processing nodes between VideoInput and VideoOutput, in order.
///
/// Resize/Rescale are compiled into the encoder's FFmpeg scale filter, so they
/// can only appear after every frame-processing stage.
pub fn validate_video_processing_chain(node_types: &[&str]) -> Result<()> {
    let mut output_scale_node: Option<&str> = None;
    for &node_type in node_types {
        if !SUPPORTED_VIDEO_PROCESSING_NODES.contains(&node_type) {
            bail!(
                "processing node '{node_type}' is not supported in video jobs; \
                 use SuperResolution, FrameInterpolation, Resize or Rescale"
            );
        }
        if is_output_scale_node(node_type) {
            output_scale_node = Some(node_type);
        } else if let Some(scale_node) = output_scale_node {
            bail!(
                "'{node_type}' cannot follow '{scale_node}': Resize/Rescale must be the last \
                 processing nodes before VideoOutput"
            );
        }
    }
    Ok(())
}

/// Reject video job graphs whose processing chain the pipeline cannot compile,
/// before any job is queued.
pub fn validate_video_workflow(graph: &PipelineGraph) -> Result<()> {
    let node_types = crate::compile::video_processing_node_types(graph)?;
    let node_types: Vec<&str> = node_types.iter().map(String::as_str).collect();
    validate_video_processing_chain(&node_types)
}

/// Raw pipe bit depth for frames decoded from a source of `source_bit_depth`.
fn pipe_bit_depth_for_source(source_bit_depth: u8) -> u8 {
    if source_bit_depth > 8 {
        16
    } else {
        8
    }
}

fn is_420_pixel_format(pixel_format: &str) -> bool {
    pixel_format.contains("420") || matches!(pixel_format, "p010le" | "nv12")
}

#[cfg(all(test, target_os = "linux", target_env = "gnu"))]
#[path = "compile_context_memory_tests.rs"]
mod memory_tests;

pub struct VideoCompileContext {
    output_width: Cell<u32>,
    output_height: Cell<u32>,
    output_fps_num: Cell<u32>,
    output_fps_den: Cell<u32>,
    total_output_frames: Cell<Option<u64>>,
    previous_node_type: RefCell<Option<String>>,
    accumulated_stages: RefCell<Vec<PipelineStage>>,
    source_path: RefCell<Option<PathBuf>>,
    pending_superres_emit_tensor: RefCell<Option<Arc<AtomicBool>>>,
    pending_superres_direct_rgb: RefCell<Option<Arc<AtomicBool>>>,
    previous_superres_fp16: Cell<bool>,
    previous_superres_tile_size: Cell<u32>,
    pending_fi_emit_tensor: RefCell<Option<Arc<AtomicBool>>>,
    /// Trailing Resize/Rescale nodes, applied by the encoder after `output_width/height`.
    output_scales: RefCell<Vec<OutputScale>>,
    /// Bit depth of the frames reaching the encoder (8 or 16).
    pipe_bit_depth: Cell<u8>,
    trt_cache_dir: PathBuf,
    /// Frame buffers shared by the decoder, pooled stages and encoder of a job.
    frame_pool: Arc<FramePool>,
}

impl Drop for VideoCompileContext {
    fn drop(&mut self) {
        // Compilation can fail while the context still owns initialized stages.
        // Drop their sessions and buffers before asking the allocator to reclaim pages.
        self.accumulated_stages.get_mut().clear();
        self.frame_pool.clear();

        // Repeated inference jobs leave GiB of freed buffers in glibc's thread
        // arenas. Return those pages at the job boundary, outside the frame loop.
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        // SAFETY: malloc_trim is thread-safe and only releases unused allocator pages.
        unsafe {
            libc::malloc_trim(0);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SuperResDimensions {
    input_width: u32,
    input_height: u32,
    output_width: u32,
    output_height: u32,
}

impl SuperResDimensions {
    fn new(input_width: u32, input_height: u32, scale: u32) -> Self {
        Self {
            input_width,
            input_height,
            output_width: input_width.saturating_mul(scale),
            output_height: input_height.saturating_mul(scale),
        }
    }
}

impl VideoCompileContext {
    fn encoder_config(
        &self,
        inputs: &HashMap<String, PortData>,
        outputs: &HashMap<String, PortData>,
    ) -> Result<EncoderConfig> {
        let source_path = self
            .source_path
            .borrow()
            .clone()
            .ok_or_else(|| anyhow!("source path is unavailable in compile context"))?;

        let output_path = match outputs.get("output_path") {
            Some(PortData::Path(path)) => path.clone(),
            Some(_) => bail!("VideoOutput output 'output_path' must be Path"),
            None => bail!("VideoOutput output 'output_path' is missing"),
        };

        let codec = match inputs.get("codec") {
            Some(PortData::Str(value)) => value.clone(),
            _ => "libx265".to_string(),
        };
        let crf = match inputs.get("crf") {
            Some(PortData::Int(value)) => *value,
            _ => 18,
        };
        let requested_pixel_format = match inputs.get("pixel_format") {
            Some(PortData::Str(value)) => value.as_str(),
            _ => default_pixel_format_for_codec(&codec),
        };
        let pixel_format = compatible_pixel_format(&codec, requested_pixel_format).to_string();
        let cq_value = match inputs.get("cq_value") {
            Some(PortData::Int(value)) => Some(*value),
            _ => None,
        };
        let nvenc_preset = match inputs.get("nvenc_preset") {
            Some(PortData::Str(value)) => Some(value.clone()),
            _ => None,
        };
        let x265_preset = match inputs.get("x265_preset") {
            Some(PortData::Str(value)) => Some(value.clone()),
            _ => None,
        };

        let width = self.output_width.get();
        let height = self.output_height.get();
        if width == 0 || height == 0 {
            bail!("output resolution is not initialized");
        }

        let mut output_scales = self.output_scales.borrow().clone();
        match (
            optional_positive_dimension(inputs, "width")?,
            optional_positive_dimension(inputs, "height")?,
        ) {
            (Some(output_width), Some(output_height)) => {
                if (output_width, output_height) != self.scaled_output_dimensions() {
                    output_scales.push(OutputScale {
                        width: output_width,
                        height: output_height,
                        algorithm: ResizeAlgorithm::Lanczos,
                    });
                }
            }
            (None, None) => {}
            _ => bail!("VideoOutput: set both width and height, or neither"),
        }
        if let Some(scale) = output_scales.last() {
            if is_420_pixel_format(&pixel_format) && (scale.width % 2 != 0 || scale.height % 2 != 0)
            {
                bail!(
                    "output size {}x{} must have even width and height for pixel format {}",
                    scale.width,
                    scale.height,
                    pixel_format
                );
            }
        }

        Ok(EncoderConfig {
            source_path,
            output_path,
            codec,
            crf,
            pixel_format,
            width,
            height,
            fps: self.output_fps_string(),
            bit_depth: self.pipe_bit_depth.get(),
            cq_value,
            nvenc_preset,
            x265_preset,
            output_scales,
        })
    }

    pub fn new(trt_cache_dir: PathBuf) -> Self {
        Self {
            output_width: Cell::new(0),
            output_height: Cell::new(0),
            output_fps_num: Cell::new(24000),
            output_fps_den: Cell::new(1001),
            total_output_frames: Cell::new(None),
            previous_node_type: RefCell::new(None),
            accumulated_stages: RefCell::new(Vec::new()),
            source_path: RefCell::new(None),
            pending_superres_emit_tensor: RefCell::new(None),
            pending_superres_direct_rgb: RefCell::new(None),
            previous_superres_fp16: Cell::new(false),
            previous_superres_tile_size: Cell::new(0),
            pending_fi_emit_tensor: RefCell::new(None),
            output_scales: RefCell::new(Vec::new()),
            pipe_bit_depth: Cell::new(8),
            trt_cache_dir,
            frame_pool: FramePool::shared(),
        }
    }

    /// Resolution after the trailing Resize/Rescale nodes compiled so far.
    fn scaled_output_dimensions(&self) -> (u32, u32) {
        self.output_scales.borrow().last().map_or(
            (self.output_width.get(), self.output_height.get()),
            |scale| (scale.width, scale.height),
        )
    }

    fn push_output_scale(&self, node_type: &str, inputs: &HashMap<String, PortData>) -> Result<()> {
        let (current_width, current_height) = self.scaled_output_dimensions();
        if current_width == 0 || current_height == 0 {
            bail!("output resolution is not initialized");
        }

        let (width, height) = if node_type == "Resize" {
            (
                read_required_positive_u32(inputs, "width")?,
                read_required_positive_u32(inputs, "height")?,
            )
        } else {
            let scale_factor = match inputs.get("scale_factor") {
                Some(PortData::Float(value)) if *value > 0.0 => *value,
                Some(PortData::Float(value)) => {
                    bail!("scale_factor must be positive, got {value}")
                }
                Some(_) => bail!("scale_factor must be a Float"),
                None => bail!("scale_factor is required"),
            };
            rescaled_dimensions(current_width, current_height, scale_factor)?
        };
        let algorithm = match inputs.get("algorithm") {
            Some(PortData::Str(value)) => ResizeAlgorithm::from_str_lossy(value),
            _ => ResizeAlgorithm::Lanczos,
        };

        self.output_scales.borrow_mut().push(OutputScale {
            width,
            height,
            algorithm,
        });
        Ok(())
    }

    pub(crate) fn create_preview_superres(
        &self,
        inputs: &HashMap<String, PortData>,
        width: u32,
        height: u32,
    ) -> Result<SuperResNode> {
        let scale = read_positive_u32(inputs, "scale", 4)?;
        self.create_superres_node(inputs, SuperResDimensions::new(width, height, scale))
    }

    fn create_superres_node(
        &self,
        inputs: &HashMap<String, PortData>,
        dimensions: SuperResDimensions,
    ) -> Result<SuperResNode> {
        let mut node = SuperResNode::new();
        if let Some(cache_dir) = self.resolve_trt_cache_dir_for_dimensions(
            inputs,
            4,
            dimensions.input_width,
            dimensions.input_height,
        )? {
            node.set_trt_cache_dir(cache_dir);
        }
        node.execute(inputs, &ExecutionContext::default())
            .context("failed to initialize SuperResolution node")?;
        Ok(node)
    }

    fn create_fi_node(&self, inputs: &HashMap<String, PortData>) -> Result<FrameInterpolationNode> {
        let mut node = FrameInterpolationNode::new();
        if let Some(cache_dir) = self.resolve_trt_cache_dir(inputs, 32)? {
            node.set_trt_cache_dir(cache_dir);
        }
        node.execute(inputs, &ExecutionContext::default())
            .context("failed to initialize FrameInterpolation node")?;
        Ok(node)
    }

    fn resolve_trt_cache_dir(
        &self,
        inputs: &HashMap<String, PortData>,
        alignment: usize,
    ) -> Result<Option<PathBuf>> {
        self.resolve_trt_cache_dir_for_dimensions(
            inputs,
            alignment,
            self.output_width.get(),
            self.output_height.get(),
        )
    }

    fn resolve_trt_cache_dir_for_dimensions(
        &self,
        inputs: &HashMap<String, PortData>,
        alignment: usize,
        input_width: u32,
        input_height: u32,
    ) -> Result<Option<PathBuf>> {
        let backend = match inputs.get("backend") {
            Some(PortData::Str(value)) => InferenceBackend::from_str_lossy(value),
            _ => InferenceBackend::Cuda,
        };
        if backend != InferenceBackend::Tensorrt {
            return Ok(None);
        }

        let model_path = match inputs.get("model_path") {
            Some(PortData::Path(path)) => path,
            Some(_) => bail!("model_path must be a Path"),
            None => bail!("model_path is required"),
        };
        let original_h = usize::try_from(input_height).context("video height exceeds usize")?;
        let original_w = usize::try_from(input_width).context("video width exceeds usize")?;
        let input_h = aligned_dimension(input_height, alignment)?;
        let input_w = aligned_dimension(input_width, alignment)?;
        let tile = match inputs.get("tile_size") {
            Some(PortData::Int(value)) => {
                let value = usize::try_from(*value).context("tile_size must be non-negative")?;
                (value > 0).then_some(TrtTileIdentity {
                    tile_size: value,
                    original_h,
                    original_w,
                })
            }
            Some(_) => bail!("tile_size must be an Int"),
            None => None,
        };
        let identity = TrtCacheIdentity {
            device_id: 0,
            input_h,
            input_w,
            tile,
        };
        model_trt_cache_dir(&self.trt_cache_dir, model_path, identity).map(Some)
    }

    fn create_superres_stages(
        &self,
        inputs: &HashMap<String, PortData>,
    ) -> Result<Vec<PipelineStage>> {
        let scale = read_positive_u32(inputs, "scale", 4)?;
        let dimensions =
            SuperResDimensions::new(self.output_width.get(), self.output_height.get(), scale);
        let node = self.create_superres_node(inputs, dimensions)?;
        let num_workers = node.num_workers();
        let is_fp16_model = node.is_fp16();
        let tile_size = node.tile_size();

        let fi_to_sr =
            should_enable_fi_to_sr_passthrough(self.previous_node_type.borrow().as_deref());
        if fi_to_sr {
            if let Some(emit_tensor) = self.pending_fi_emit_tensor.borrow().as_ref() {
                emit_tensor.store(true, Ordering::Relaxed);
            }
        }

        let emit_tensor = Arc::new(AtomicBool::new(false));
        let direct_rgb = Arc::new(AtomicBool::new(false));
        self.pending_superres_emit_tensor
            .replace(Some(Arc::clone(&emit_tensor)));
        self.pending_superres_direct_rgb
            .replace(Some(Arc::clone(&direct_rgb)));
        self.previous_superres_fp16.set(is_fp16_model);
        self.previous_superres_tile_size.set(tile_size);
        self.pending_fi_emit_tensor.replace(None);

        if should_use_superres_micro_stages(is_fp16_model, tile_size) {
            let mut micro = node
                .into_micro_stages()
                .ok_or_else(|| anyhow!("failed to build SuperResolution micro-stages"))?;
            micro.set_frame_pool(&self.frame_pool);
            let mut inference_lanes: Vec<Box<dyn FrameProcessor>> = Vec::new();
            inference_lanes
                .try_reserve_exact(num_workers)
                .context("failed to reserve SuperResolution inference lanes")?;
            micro.inference.set_direct_rgb_flag(Arc::clone(&direct_rgb));
            inference_lanes.push(Box::new(micro.inference));
            for lane_index in 1..num_workers {
                let worker = self
                    .create_superres_node(inputs, dimensions)
                    .with_context(|| {
                        format!(
                            "failed to initialize SuperResolution inference lane {}",
                            lane_index + 1
                        )
                    })?;
                if worker.is_fp16() != is_fp16_model || worker.tile_size() != tile_size {
                    bail!(
                        "SuperResolution inference lane {} detected incompatible model settings",
                        lane_index + 1
                    );
                }
                let mut worker_micro = worker.into_micro_stages().ok_or_else(|| {
                    anyhow!(
                        "failed to build SuperResolution inference lane {}",
                        lane_index + 1
                    )
                })?;
                worker_micro
                    .inference
                    .set_direct_rgb_flag(Arc::clone(&direct_rgb));
                worker_micro
                    .inference
                    .set_frame_pool(Arc::clone(&self.frame_pool));
                inference_lanes.push(Box::new(worker_micro.inference));
            }
            self.accumulated_stages
                .borrow_mut()
                .push(PipelineStage::Processor(Box::new(micro.preprocess)));
            self.accumulated_stages
                .borrow_mut()
                .push(ordered_processor_stage(inference_lanes)?);
            self.accumulated_stages
                .borrow_mut()
                .push(PipelineStage::Processor(Box::new(
                    SuperResPostprocessStage {
                        inner: micro.postprocess,
                        emit_tensor,
                    },
                )));
        } else {
            let mut worker_lanes: Vec<Box<dyn FrameProcessor>> = Vec::new();
            worker_lanes
                .try_reserve_exact(num_workers)
                .context("failed to reserve SuperResolution worker lanes")?;
            worker_lanes.push(Box::new(SuperResSingleStage {
                inner: node,
                emit_tensor: Arc::clone(&emit_tensor),
            }));
            for lane_index in 1..num_workers {
                let worker = self
                    .create_superres_node(inputs, dimensions)
                    .with_context(|| {
                        format!(
                            "failed to initialize SuperResolution worker lane {}",
                            lane_index + 1
                        )
                    })?;
                if worker.is_fp16() != is_fp16_model || worker.tile_size() != tile_size {
                    bail!(
                        "SuperResolution worker lane {} detected incompatible model settings",
                        lane_index + 1
                    );
                }
                worker_lanes.push(Box::new(SuperResSingleStage {
                    inner: worker,
                    emit_tensor: Arc::clone(&emit_tensor),
                }));
            }
            self.accumulated_stages
                .borrow_mut()
                .push(ordered_processor_stage(worker_lanes)?);
        }

        self.output_width.set(dimensions.output_width);
        self.output_height.set(dimensions.output_height);
        self.pipe_bit_depth.set(8);

        self.previous_node_type
            .replace(Some("SuperResolution".to_string()));
        Ok(take_stages(&self.accumulated_stages))
    }

    fn create_fi_stages(&self, inputs: &HashMap<String, PortData>) -> Result<Vec<PipelineStage>> {
        let node = self.create_fi_node(inputs)?;
        let model_format = node.model_format();
        let num_workers = node.num_workers();
        let use_parallel_inference = should_use_parallel_fi(num_workers, model_format);

        let multiplier = read_positive_u32(inputs, "multiplier", 2)?;
        self.output_fps_num
            .set(self.output_fps_num.get().saturating_mul(multiplier));
        if let Some(total) = self.total_output_frames.get() {
            self.total_output_frames
                .set(Some(total.saturating_mul(multiplier as u64)));
        }

        let sr_to_fi = should_enable_sr_to_fi_passthrough(
            self.previous_node_type.borrow().as_deref(),
            self.previous_superres_fp16.get(),
        );

        match superres_output_mode(
            self.previous_superres_fp16.get(),
            self.previous_superres_tile_size.get(),
            sr_to_fi,
        ) {
            SuperResOutputMode::TensorF16 => {
                if let Some(emit_tensor) = self.pending_superres_emit_tensor.borrow().as_ref() {
                    emit_tensor.store(true, Ordering::Relaxed);
                }
            }
            SuperResOutputMode::PostprocessRgb | SuperResOutputMode::DirectRgb => {}
        }

        let fi_emit_tensor = Arc::new(AtomicBool::new(false));
        self.pending_fi_emit_tensor
            .replace(Some(Arc::clone(&fi_emit_tensor)));

        if should_use_fi_micro_stages(model_format, sr_to_fi) {
            let mut micro = node
                .into_micro_stages()
                .ok_or_else(|| anyhow!("failed to build FrameInterpolation micro-stages"))?;
            micro.set_frame_pool(&self.frame_pool);
            self.accumulated_stages
                .borrow_mut()
                .push(PipelineStage::Processor(Box::new(micro.preprocess)));
            let mut inference_lanes: Vec<Box<dyn FrameInterpolator>> = Vec::new();
            inference_lanes
                .try_reserve_exact(num_workers)
                .context("failed to reserve FrameInterpolation inference lanes")?;
            inference_lanes.push(Box::new(micro.inference));
            for lane_index in 1..num_workers {
                let worker = self.create_fi_node(inputs).with_context(|| {
                    format!(
                        "failed to initialize FrameInterpolation inference lane {}",
                        lane_index + 1
                    )
                })?;
                if worker.model_format() != model_format {
                    bail!(
                        "FrameInterpolation inference lane {} detected a different model format",
                        lane_index + 1
                    );
                }
                let mut worker_micro = worker.into_micro_stages().ok_or_else(|| {
                    anyhow!(
                        "failed to build FrameInterpolation inference lane {}",
                        lane_index + 1
                    )
                })?;
                worker_micro
                    .inference
                    .set_frame_pool(Arc::clone(&self.frame_pool));
                inference_lanes.push(Box::new(worker_micro.inference));
            }
            self.accumulated_stages
                .borrow_mut()
                .push(ordered_interpolator_stage(inference_lanes)?);
            micro.postprocess.emit_tensor = fi_emit_tensor.load(Ordering::Relaxed);
            self.accumulated_stages
                .borrow_mut()
                .push(PipelineStage::Processor(Box::new(FIPostprocessStage {
                    inner: micro.postprocess,
                    emit_tensor: fi_emit_tensor,
                })));
        } else {
            let mut worker_lanes: Vec<Box<dyn FrameInterpolator>> = Vec::new();
            worker_lanes
                .try_reserve_exact(num_workers)
                .context("failed to reserve FrameInterpolation worker lanes")?;
            let mut primary = node;
            if use_parallel_inference {
                primary.disable_pair_cache();
            }
            worker_lanes.push(Box::new(FISingleStage {
                inner: primary,
                emit_tensor: Arc::clone(&fi_emit_tensor),
            }));
            for lane_index in 1..num_workers {
                let mut worker = self.create_fi_node(inputs).with_context(|| {
                    format!(
                        "failed to initialize FrameInterpolation worker lane {}",
                        lane_index + 1
                    )
                })?;
                if worker.model_format() != model_format {
                    bail!(
                        "FrameInterpolation worker lane {} detected a different model format",
                        lane_index + 1
                    );
                }
                worker.disable_pair_cache();
                worker_lanes.push(Box::new(FISingleStage {
                    inner: worker,
                    emit_tensor: Arc::clone(&fi_emit_tensor),
                }));
            }
            self.accumulated_stages
                .borrow_mut()
                .push(ordered_interpolator_stage(worker_lanes)?);
        }

        self.pending_superres_emit_tensor.replace(None);
        self.pending_superres_direct_rgb.replace(None);
        self.previous_superres_fp16.set(false);
        self.previous_superres_tile_size.set(0);
        self.pipe_bit_depth.set(8);
        self.previous_node_type
            .replace(Some("FrameInterpolation".to_string()));
        Ok(take_stages(&self.accumulated_stages))
    }

    fn output_fps_string(&self) -> String {
        let num = self.output_fps_num.get().max(1);
        let den = self.output_fps_den.get().max(1);
        format!("{num}/{den}")
    }
}

impl Default for VideoCompileContext {
    fn default() -> Self {
        Self::new(PathBuf::from("trt_cache"))
    }
}

impl CompileContext for VideoCompileContext {
    fn create_decoder(
        &self,
        node: &mut dyn Node,
        outputs: &HashMap<String, PortData>,
    ) -> DecoderResult {
        if node.node_type() != "video_input" && node.node_type() != "VideoInput" {
            bail!(
                "expected VideoInput source node, got '{}'",
                node.node_type()
            );
        }

        let source_path = match outputs.get("source_path") {
            Some(PortData::Path(path)) => path.clone(),
            Some(_) => bail!("VideoInput output 'source_path' must be Path"),
            None => bail!("VideoInput output 'source_path' is missing"),
        };

        let probe = run_ffprobe(&source_path).context("failed to probe input video")?;
        let (video_info, _metadata) =
            extract_metadata(&probe, &source_path).context("failed to parse input metadata")?;

        let (fps_num, fps_den) = fps_to_rational(video_info.fps);
        let total_frames = estimate_total_frames(&source_path, video_info.fps);

        let mut decoder = VideoDecoder::new(&source_path, &video_info, Some("none"))
            .context("failed to create video decoder")?;
        decoder.set_frame_pool(Arc::clone(&self.frame_pool));

        self.source_path.replace(Some(source_path));
        self.output_width.set(video_info.width);
        self.output_height.set(video_info.height);
        self.output_fps_num.set(fps_num);
        self.output_fps_den.set(fps_den);
        self.total_output_frames.set(total_frames);
        self.previous_node_type.replace(None);
        self.pending_superres_emit_tensor.replace(None);
        self.pending_superres_direct_rgb.replace(None);
        self.previous_superres_fp16.set(false);
        self.previous_superres_tile_size.set(0);
        self.pending_fi_emit_tensor.replace(None);
        self.output_scales.borrow_mut().clear();
        self.pipe_bit_depth
            .set(pipe_bit_depth_for_source(video_info.bit_depth));

        Ok((Box::new(decoder), total_frames))
    }

    fn create_encoder(
        &self,
        node: &mut dyn Node,
        inputs: &HashMap<String, PortData>,
        outputs: &HashMap<String, PortData>,
    ) -> Result<Box<dyn FrameSink>> {
        if node.node_type() != "video_output" && node.node_type() != "VideoOutput" {
            bail!("expected VideoOutput sink node, got '{}'", node.node_type());
        }

        match superres_output_mode(
            self.previous_superres_fp16.get(),
            self.previous_superres_tile_size.get(),
            false,
        ) {
            SuperResOutputMode::DirectRgb => {
                if let Some(direct_rgb) = self.pending_superres_direct_rgb.borrow().as_ref() {
                    direct_rgb.store(true, Ordering::Relaxed);
                }
            }
            SuperResOutputMode::PostprocessRgb | SuperResOutputMode::TensorF16 => {}
        }

        let config = self.encoder_config(inputs, outputs)?;

        let mut encoder = VideoEncoder::new(&config).context("failed to create video encoder")?;
        encoder.set_frame_pool(Arc::clone(&self.frame_pool));
        Ok(Box::new(encoder))
    }

    fn create_processor(
        &self,
        node: Box<dyn Node>,
        inputs: &HashMap<String, PortData>,
    ) -> Result<Box<dyn FrameProcessor>> {
        if node.node_type() != "SuperResolution" {
            bail!(
                "unsupported processor node '{}' in VideoCompileContext",
                node.node_type()
            );
        }

        let scale = read_positive_u32(inputs, "scale", 4)?;
        let dimensions =
            SuperResDimensions::new(self.output_width.get(), self.output_height.get(), scale);
        let node = self.create_superres_node(inputs, dimensions)?;
        self.output_width.set(dimensions.output_width);
        self.output_height.set(dimensions.output_height);
        self.pipe_bit_depth.set(8);

        let fi_to_sr =
            should_enable_fi_to_sr_passthrough(self.previous_node_type.borrow().as_deref());
        if fi_to_sr {
            if let Some(emit_tensor) = self.pending_fi_emit_tensor.borrow().as_ref() {
                emit_tensor.store(true, Ordering::Relaxed);
            }
        }

        let emit_tensor = Arc::new(AtomicBool::new(false));
        self.pending_superres_emit_tensor
            .replace(Some(Arc::clone(&emit_tensor)));
        self.previous_superres_fp16.set(node.is_fp16());
        self.previous_superres_tile_size.set(node.tile_size());
        self.pending_fi_emit_tensor.replace(None);
        self.previous_node_type
            .replace(Some("SuperResolution".to_string()));

        Ok(Box::new(SuperResSingleStage {
            inner: node,
            emit_tensor,
        }))
    }

    fn create_interpolator(
        &self,
        node: Box<dyn Node>,
        inputs: &HashMap<String, PortData>,
    ) -> Result<Box<dyn FrameInterpolator>> {
        if !self.is_interpolator_type(node.node_type()) {
            bail!(
                "unsupported interpolator node '{}' in VideoCompileContext",
                node.node_type()
            );
        }

        let multiplier = read_positive_u32(inputs, "multiplier", 2)?;
        self.output_fps_num
            .set(self.output_fps_num.get().saturating_mul(multiplier));
        if let Some(total) = self.total_output_frames.get() {
            self.total_output_frames
                .set(Some(total.saturating_mul(multiplier as u64)));
        }
        self.pipe_bit_depth.set(8);

        let sr_to_fi = should_enable_sr_to_fi_passthrough(
            self.previous_node_type.borrow().as_deref(),
            self.previous_superres_fp16.get(),
        );
        if sr_to_fi {
            if let Some(emit_tensor) = self.pending_superres_emit_tensor.borrow().as_ref() {
                emit_tensor.store(true, Ordering::Relaxed);
            }
        }

        let fi_node = self.create_fi_node(inputs)?;

        let fi_emit_tensor = Arc::new(AtomicBool::new(false));
        self.pending_fi_emit_tensor
            .replace(Some(Arc::clone(&fi_emit_tensor)));

        self.pending_superres_emit_tensor.replace(None);
        self.pending_superres_direct_rgb.replace(None);
        self.previous_superres_fp16.set(false);
        self.previous_superres_tile_size.set(0);
        self.previous_node_type
            .replace(Some("FrameInterpolation".to_string()));

        Ok(Box::new(FISingleStage {
            inner: fi_node,
            emit_tensor: fi_emit_tensor,
        }))
    }

    fn is_interpolator_type(&self, node_type: &str) -> bool {
        node_type == "FrameInterpolation"
    }

    fn execute_processing_node(&self, node_type: &str) -> bool {
        node_type != "SuperResolution" && node_type != "FrameInterpolation"
    }

    fn total_output_frames(&self) -> Option<u64> {
        self.total_output_frames.get()
    }

    fn create_stages(
        &self,
        node: Box<dyn Node>,
        inputs: &HashMap<String, PortData>,
        is_interpolator: bool,
    ) -> Result<Vec<PipelineStage>> {
        self.accumulated_stages.borrow_mut().clear();

        let node_type = node.node_type();
        if is_output_scale_node(node_type) {
            self.push_output_scale(node_type, inputs)?;
            return Ok(Vec::new());
        }
        if !self.output_scales.borrow().is_empty() {
            bail!(
                "'{node_type}' cannot follow Resize/Rescale: Resize/Rescale must be the last \
                 processing nodes before VideoOutput"
            );
        }

        if is_interpolator {
            if self.is_interpolator_type(node.node_type()) {
                return self.create_fi_stages(inputs);
            }
            return Ok(vec![PipelineStage::Interpolator(
                self.create_interpolator(node, inputs)?,
            )]);
        }

        if node.node_type() == "SuperResolution" {
            return self.create_superres_stages(inputs);
        }

        Ok(vec![PipelineStage::Processor(
            self.create_processor(node, inputs)?,
        )])
    }
}

struct SuperResSingleStage {
    inner: SuperResNode,
    emit_tensor: Arc<AtomicBool>,
}

impl Node for SuperResSingleStage {
    fn node_type(&self) -> &str {
        "SuperResolution"
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

impl FrameProcessor for SuperResSingleStage {
    fn process_frame(&mut self, frame: Frame, ctx: &ExecutionContext) -> Result<Frame> {
        self.inner
            .set_emit_tensor(self.emit_tensor.load(Ordering::Relaxed));
        self.inner.process_frame(frame, ctx)
    }
}

struct SuperResPostprocessStage {
    inner: SuperResPostprocess,
    emit_tensor: Arc<AtomicBool>,
}

impl Node for SuperResPostprocessStage {
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

impl FrameProcessor for SuperResPostprocessStage {
    fn process_frame(&mut self, frame: Frame, ctx: &ExecutionContext) -> Result<Frame> {
        if matches!(frame, Frame::CpuRgb { .. }) {
            return Ok(frame);
        }
        if self.emit_tensor.load(Ordering::Relaxed) && matches!(frame, Frame::NchwF16 { .. }) {
            return Ok(frame);
        }
        self.inner.process_frame(frame, ctx)
    }
}

struct FISingleStage {
    inner: FrameInterpolationNode,
    emit_tensor: Arc<AtomicBool>,
}

impl Node for FISingleStage {
    fn node_type(&self) -> &str {
        "FrameInterpolation"
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

impl FrameInterpolator for FISingleStage {
    fn stage_name(&self) -> &str {
        "FrameInterpolation"
    }

    fn interpolate(
        &mut self,
        previous: &Frame,
        current: &Frame,
        is_scene_change: bool,
        ctx: &ExecutionContext,
    ) -> Result<Vec<Frame>> {
        self.inner
            .set_emit_tensor(self.emit_tensor.load(Ordering::Relaxed));
        self.inner
            .interpolate(previous, current, is_scene_change, ctx)
    }
}

struct FIPostprocessStage {
    inner: FrameInterpolationPostprocess,
    emit_tensor: Arc<AtomicBool>,
}

impl Node for FIPostprocessStage {
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

impl FrameProcessor for FIPostprocessStage {
    fn process_frame(&mut self, frame: Frame, ctx: &ExecutionContext) -> Result<Frame> {
        self.inner.emit_tensor = self.emit_tensor.load(Ordering::Relaxed);
        self.inner.process_frame(frame, ctx)
    }
}

fn ordered_processor_stage(mut workers: Vec<Box<dyn FrameProcessor>>) -> Result<PipelineStage> {
    match workers.len() {
        0 => bail!("ordered processor stage requires at least one worker"),
        1 => Ok(PipelineStage::Processor(
            workers
                .pop()
                .context("ordered processor stage lost its only worker")?,
        )),
        _ => Ok(PipelineStage::ParallelProcessor(workers)),
    }
}

fn ordered_interpolator_stage(
    mut workers: Vec<Box<dyn FrameInterpolator>>,
) -> Result<PipelineStage> {
    match workers.len() {
        0 => bail!("ordered interpolator stage requires at least one worker"),
        1 => {
            Ok(PipelineStage::Interpolator(workers.pop().context(
                "ordered interpolator stage lost its only worker",
            )?))
        }
        _ => Ok(PipelineStage::ParallelInterpolator(workers)),
    }
}

fn read_positive_u32(inputs: &HashMap<String, PortData>, key: &str, default: u32) -> Result<u32> {
    match inputs.get(key) {
        Some(PortData::Int(value)) => {
            if *value <= 0 {
                bail!("{key} must be positive, got {value}");
            }
            Ok(*value as u32)
        }
        Some(_) => bail!("{key} must be Int"),
        None => Ok(default),
    }
}

fn read_required_positive_u32(inputs: &HashMap<String, PortData>, key: &str) -> Result<u32> {
    match inputs.get(key) {
        Some(PortData::Int(value)) => {
            if *value <= 0 {
                bail!("{key} must be positive, got {value}");
            }
            u32::try_from(*value).with_context(|| format!("{key} is too large: {value}"))
        }
        Some(_) => bail!("{key} must be Int"),
        None => bail!("{key} is required"),
    }
}

fn should_use_superres_micro_stages(is_fp16_model: bool, tile_size: u32) -> bool {
    is_fp16_model && tile_size == 0
}

fn superres_output_mode(
    is_fp16_model: bool,
    tile_size: u32,
    has_frame_interpolation_downstream: bool,
) -> SuperResOutputMode {
    if !should_use_superres_micro_stages(is_fp16_model, tile_size) {
        return SuperResOutputMode::PostprocessRgb;
    }
    if has_frame_interpolation_downstream {
        SuperResOutputMode::TensorF16
    } else {
        SuperResOutputMode::DirectRgb
    }
}

fn should_enable_sr_to_fi_passthrough(
    previous_node_type: Option<&str>,
    previous_superres_fp16: bool,
) -> bool {
    previous_node_type == Some("SuperResolution") && previous_superres_fp16
}

fn should_enable_fi_to_sr_passthrough(previous_node_type: Option<&str>) -> bool {
    previous_node_type == Some("FrameInterpolation")
}

fn should_use_fi_micro_stages(model_format: ModelFormat, _tensor_passthrough: bool) -> bool {
    model_format == ModelFormat::Concatenated
}

const fn should_use_parallel_fi(num_workers: usize, _model_format: ModelFormat) -> bool {
    num_workers > 1
}

fn fps_to_rational(fps: f64) -> (u32, u32) {
    if !fps.is_finite() || fps <= 0.0 {
        return (24000, 1001);
    }

    let mut best_num = fps.round() as u32;
    let mut best_den = 1_u32;
    let mut best_error = (fps - f64::from(best_num)).abs();

    for den in 2_u32..=1001 {
        let num = (fps * f64::from(den)).round() as u32;
        let error = (fps - f64::from(num) / f64::from(den)).abs();
        if error < best_error {
            best_num = num;
            best_den = den;
            best_error = error;
        }
    }

    if best_num == 0 {
        return (24000, 1001);
    }

    let divisor = gcd(best_num, best_den).max(1);
    (best_num / divisor, best_den / divisor)
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let next = a % b;
        a = b;
        b = next;
    }
    a
}

fn estimate_total_frames(input: &Path, fps: f64) -> Option<u64> {
    let output = crate::runtime::command_for("ffprobe")
        .args([
            "-v",
            "quiet",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=nb_frames,duration",
            "-show_entries",
            "format=duration",
            "-print_format",
            "json",
        ])
        .arg(input)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .ok()?;

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;

    // Try stream-level nb_frames first (MP4/AVI usually have this).
    if let Some(stream) = json
        .get("streams")
        .and_then(|s| s.as_array())
        .and_then(|a| a.first())
    {
        if let Some(nb_frames) = stream.get("nb_frames").and_then(|v| v.as_str()) {
            if let Ok(value) = nb_frames.parse::<u64>() {
                if value > 0 {
                    return Some(value);
                }
            }
        }

        // Try stream-level duration × fps.
        if let Some(duration) = stream.get("duration").and_then(|v| v.as_str()) {
            if let Ok(seconds) = duration.parse::<f64>() {
                if seconds > 0.0 && fps > 0.0 {
                    return Some((seconds * fps).round() as u64);
                }
            }
        }
    }

    // Fallback: format-level duration × fps (MKV stores duration at format level).
    if let Some(duration) = json
        .get("format")
        .and_then(|f| f.get("duration"))
        .and_then(|v| v.as_str())
    {
        if let Ok(seconds) = duration.parse::<f64>() {
            if seconds > 0.0 && fps > 0.0 {
                return Some((seconds * fps).round() as u64);
            }
        }
    }

    None
}

fn take_stages(stages: &RefCell<Vec<PipelineStage>>) -> Vec<PipelineStage> {
    std::mem::take(&mut *stages.borrow_mut())
}

fn aligned_dimension(value: u32, alignment: usize) -> Result<usize> {
    let value = usize::try_from(value).context("video dimension exceeds usize")?;
    if value == 0 {
        bail!("video resolution is not initialized");
    }
    Ok(value.div_ceil(alignment) * alignment)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::rescale::RescaleNode;
    use crate::nodes::resize::{ResizeAlgorithm, ResizeNode};

    fn superres_stage_count(is_fp16_model: bool, tile_size: u32) -> usize {
        if should_use_superres_micro_stages(is_fp16_model, tile_size) {
            3
        } else {
            1
        }
    }

    fn fi_stage_count(model_format: ModelFormat, tensor_passthrough: bool) -> usize {
        if should_use_fi_micro_stages(model_format, tensor_passthrough) {
            3
        } else {
            1
        }
    }

    #[test]
    fn test_video_compile_context_sr_only() {
        assert_eq!(superres_stage_count(false, 0), 1);
        assert_eq!(superres_stage_count(true, 64), 1);
        assert_eq!(superres_stage_count(true, 0), 3);
    }

    #[test]
    fn terminal_fp16_superres_selects_direct_rgb_output() {
        assert_eq!(
            superres_output_mode(true, 0, false),
            SuperResOutputMode::DirectRgb
        );
    }

    #[test]
    fn fp16_superres_before_frame_interpolation_selects_tensor_output() {
        assert_eq!(
            superres_output_mode(true, 0, true),
            SuperResOutputMode::TensorF16
        );
    }

    #[test]
    fn tiled_or_fp32_superres_keeps_postprocess_output() {
        assert_eq!(
            superres_output_mode(true, 64, false),
            SuperResOutputMode::PostprocessRgb
        );
        assert_eq!(
            superres_output_mode(false, 0, false),
            SuperResOutputMode::PostprocessRgb
        );
    }

    #[test]
    fn test_video_compile_context_fi_only() {
        assert_eq!(fi_stage_count(ModelFormat::ThreeInput, false), 1);
        assert_eq!(fi_stage_count(ModelFormat::Concatenated, false), 3);
    }

    #[test]
    fn ntsc_frame_rate_keeps_its_exact_rational() {
        // Given: the floating-point value parsed from 24000/1001.
        let fps = 24_000.0 / 1_001.0;

        // When: the compile context converts it back to an encoder rate.
        let rational = fps_to_rational(fps);

        // Then: frame interpolation can multiply the standard rational exactly.
        assert_eq!(rational, (24_000, 1_001));
    }

    #[test]
    fn video_compile_context_preserves_non_default_encoder_settings() {
        // Given: a compile context and user-selected encoder settings.
        let context = VideoCompileContext::default();
        context
            .source_path
            .replace(Some(PathBuf::from("input.mkv")));
        context.output_width.set(3840);
        context.output_height.set(2160);
        let inputs = HashMap::from([
            ("codec".to_string(), PortData::Str("hevc_nvenc".to_string())),
            ("crf".to_string(), PortData::Int(23)),
            (
                "pixel_format".to_string(),
                PortData::Str("p010le".to_string()),
            ),
            ("cq_value".to_string(), PortData::Int(17)),
            ("nvenc_preset".to_string(), PortData::Str("p6".to_string())),
            ("x265_preset".to_string(), PortData::Str("slow".to_string())),
        ]);
        let outputs = HashMap::from([(
            "output_path".to_string(),
            PortData::Path(PathBuf::from("output.mkv")),
        )]);

        // When: the production compile path assembles its encoder configuration.
        let config = context
            .encoder_config(&inputs, &outputs)
            .expect("encoder settings should produce a configuration");

        // Then: every non-default quality and preset setting reaches the encoder.
        assert_eq!(config.cq_value, Some(17));
        assert_eq!(config.nvenc_preset.as_deref(), Some("p6"));
        assert_eq!(config.x265_preset.as_deref(), Some("slow"));
    }

    #[test]
    fn video_compile_context_uses_codec_compatible_pixel_format() {
        // Given: a workflow that selects HEVC NVENC without overriding pixel format.
        let context = VideoCompileContext::default();
        context
            .source_path
            .replace(Some(PathBuf::from("input.mkv")));
        context.output_width.set(3840);
        context.output_height.set(2160);
        let inputs =
            HashMap::from([("codec".to_string(), PortData::Str("hevc_nvenc".to_string()))]);
        let outputs = HashMap::from([(
            "output_path".to_string(),
            PortData::Path(PathBuf::from("output.mkv")),
        )]);

        // When: the production compile path assembles the encoder configuration.
        let config = context
            .encoder_config(&inputs, &outputs)
            .expect("encoder settings should produce a configuration");

        // Then: the NVENC-compatible 10-bit surface format is selected.
        assert_eq!(config.pixel_format, "p010le");
    }

    fn context_with_output(width: u32, height: u32) -> VideoCompileContext {
        let context = VideoCompileContext::default();
        context
            .source_path
            .replace(Some(PathBuf::from("input.mkv")));
        context.output_width.set(width);
        context.output_height.set(height);
        context
    }

    fn output_path_outputs() -> HashMap<String, PortData> {
        HashMap::from([(
            "output_path".to_string(),
            PortData::Path(PathBuf::from("output.mkv")),
        )])
    }

    fn rescale_inputs(scale_factor: f64, algorithm: &str) -> HashMap<String, PortData> {
        HashMap::from([
            ("scale_factor".to_string(), PortData::Float(scale_factor)),
            (
                "algorithm".to_string(),
                PortData::Str(algorithm.to_string()),
            ),
        ])
    }

    #[test]
    fn trailing_rescale_becomes_an_encoder_scale() {
        // Given: a 4x super-resolution output of a 720p source.
        let context = context_with_output(5120, 2880);

        // When: a trailing Rescale(0.75) is compiled.
        let stages = context
            .create_stages(
                Box::new(RescaleNode::new()),
                &rescale_inputs(0.75, "lanczos"),
                false,
            )
            .expect("trailing Rescale should compile");
        let config = context
            .encoder_config(&HashMap::new(), &output_path_outputs())
            .expect("encoder config should build");

        // Then: no frame stage is added; FFmpeg scales the piped 5120x2880 frames to 4K.
        assert!(stages.is_empty());
        assert_eq!((config.width, config.height), (5120, 2880));
        assert_eq!(config.output_dimensions(), (3840, 2160));
        assert_eq!(
            config.output_scales,
            vec![OutputScale {
                width: 3840,
                height: 2160,
                algorithm: ResizeAlgorithm::Lanczos,
            }]
        );
    }

    #[test]
    fn chained_output_resizes_keep_their_order() {
        let context = context_with_output(1920, 1080);
        let resize_inputs = HashMap::from([
            ("width".to_string(), PortData::Int(1280)),
            ("height".to_string(), PortData::Int(720)),
            (
                "algorithm".to_string(),
                PortData::Str("bilinear".to_string()),
            ),
        ]);

        context
            .create_stages(Box::new(ResizeNode::new()), &resize_inputs, false)
            .expect("Resize should compile");
        context
            .create_stages(
                Box::new(RescaleNode::new()),
                &rescale_inputs(0.5, "nearest"),
                false,
            )
            .expect("Rescale after Resize should compile");
        let config = context
            .encoder_config(&HashMap::new(), &output_path_outputs())
            .unwrap();

        assert_eq!(
            config.output_scales,
            vec![
                OutputScale {
                    width: 1280,
                    height: 720,
                    algorithm: ResizeAlgorithm::Bilinear,
                },
                OutputScale {
                    width: 640,
                    height: 360,
                    algorithm: ResizeAlgorithm::Nearest,
                },
            ]
        );
    }

    #[test]
    fn processing_after_output_resize_is_rejected_before_model_loading() {
        let context = context_with_output(1280, 720);
        context
            .create_stages(
                Box::new(RescaleNode::new()),
                &rescale_inputs(0.5, "lanczos"),
                false,
            )
            .unwrap();

        let error = context
            .create_stages(Box::new(SuperResNode::new()), &HashMap::new(), false)
            .err()
            .expect("SuperResolution after Rescale must be rejected");

        let message = format!("{error:#}");
        assert!(
            message.contains("Resize/Rescale must be the last processing nodes"),
            "{message}"
        );
    }

    #[test]
    fn video_output_dimensions_scale_the_encoded_output() {
        let context = context_with_output(5120, 2880);
        let inputs = HashMap::from([
            ("width".to_string(), PortData::Int(3840)),
            ("height".to_string(), PortData::Int(2160)),
        ]);

        let config = context
            .encoder_config(&inputs, &output_path_outputs())
            .unwrap();

        assert_eq!((config.width, config.height), (5120, 2880));
        assert_eq!(
            config.output_scales,
            vec![OutputScale {
                width: 3840,
                height: 2160,
                algorithm: ResizeAlgorithm::Lanczos,
            }]
        );
    }

    #[test]
    fn video_output_dimensions_matching_the_pipeline_add_no_scale() {
        let context = context_with_output(1280, 720);
        context
            .create_stages(
                Box::new(RescaleNode::new()),
                &rescale_inputs(0.5, "bilinear"),
                false,
            )
            .unwrap();
        let inputs = HashMap::from([
            ("width".to_string(), PortData::Int(640)),
            ("height".to_string(), PortData::Int(360)),
        ]);

        let config = context
            .encoder_config(&inputs, &output_path_outputs())
            .unwrap();

        assert_eq!(config.output_scales.len(), 1);
        assert_eq!(config.output_dimensions(), (640, 360));
    }

    #[test]
    fn video_output_requires_both_dimensions() {
        let context = context_with_output(1920, 1080);
        let inputs = HashMap::from([("width".to_string(), PortData::Int(1280))]);

        let error = context
            .encoder_config(&inputs, &output_path_outputs())
            .unwrap_err();

        assert!(
            error.to_string().contains("set both width and height"),
            "{error}"
        );
    }

    #[test]
    fn odd_output_size_is_rejected_for_420_pixel_formats() {
        let context = context_with_output(1920, 1080);
        let inputs = HashMap::from([
            ("width".to_string(), PortData::Int(1279)),
            ("height".to_string(), PortData::Int(720)),
        ]);

        let error = context
            .encoder_config(&inputs, &output_path_outputs())
            .unwrap_err();

        assert!(error.to_string().contains("even"), "{error}");
    }

    #[test]
    fn encoder_pipe_uses_the_tracked_frame_bit_depth() {
        let context = context_with_output(1920, 1080);
        context.pipe_bit_depth.set(16);

        let config = context
            .encoder_config(&HashMap::new(), &output_path_outputs())
            .unwrap();

        assert_eq!(config.bit_depth, 16);
        assert_eq!(pipe_bit_depth_for_source(10), 16);
        assert_eq!(pipe_bit_depth_for_source(8), 8);
    }

    #[test]
    fn video_processing_chain_accepts_trailing_output_resizes() {
        for chain in [
            vec![],
            vec!["SuperResolution"],
            vec!["SuperResolution", "Rescale"],
            vec!["FrameInterpolation", "SuperResolution", "Resize", "Rescale"],
            vec!["Resize"],
        ] {
            validate_video_processing_chain(&chain)
                .unwrap_or_else(|error| panic!("{chain:?} should be accepted: {error}"));
        }
    }

    #[test]
    fn video_processing_chain_rejects_processing_after_output_resize() {
        let error = validate_video_processing_chain(&["Rescale", "SuperResolution"]).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("'Rescale'"), "{message}");
        assert!(message.contains("'SuperResolution'"), "{message}");
    }

    #[test]
    fn video_processing_chain_rejects_nodes_without_a_video_stage() {
        let error = validate_video_processing_chain(&["ColorSpace"]).unwrap_err();
        assert!(
            error.to_string().contains("'ColorSpace' is not supported"),
            "{error}"
        );
    }

    #[test]
    fn frame_interpolation_parallel_lanes_support_any_model_format() {
        assert!(!should_use_parallel_fi(1, ModelFormat::ThreeInput));
        for model_format in [ModelFormat::ThreeInput, ModelFormat::Concatenated] {
            assert!(should_use_parallel_fi(7, model_format));
        }
    }

    #[test]
    fn superres_worker_cache_identity_uses_pre_scale_dimensions() {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should follow the Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("videnoa_sr_cache_identity_{id}"));
        std::fs::create_dir_all(&root).expect("temporary cache root should be created");
        let model_path = root.join("model.onnx");
        std::fs::write(&model_path, b"test model identity")
            .expect("temporary model should be written");
        let context = VideoCompileContext::new(root.join("cache"));
        context.output_width.set(1920);
        context.output_height.set(1080);
        let inputs = HashMap::from([
            ("backend".to_string(), PortData::Str("tensorrt".to_string())),
            ("model_path".to_string(), PortData::Path(model_path)),
        ]);
        let dimensions = SuperResDimensions::new(1920, 1080, 4);

        let first_lane = context
            .resolve_trt_cache_dir_for_dimensions(
                &inputs,
                4,
                dimensions.input_width,
                dimensions.input_height,
            )
            .expect("first lane cache identity should resolve");
        context.output_width.set(dimensions.output_width);
        context.output_height.set(dimensions.output_height);
        let second_lane = context
            .resolve_trt_cache_dir_for_dimensions(
                &inputs,
                4,
                dimensions.input_width,
                dimensions.input_height,
            )
            .expect("second lane cache identity should resolve");

        assert_eq!(first_lane, second_lane);
        assert!(first_lane
            .expect("TensorRT should produce a cache directory")
            .to_string_lossy()
            .contains("1080x1920"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_video_compile_context_sr_fi_combined() {
        let passthrough = should_enable_sr_to_fi_passthrough(Some("SuperResolution"), true);
        assert!(
            passthrough,
            "FP16 SuperResolution -> FrameInterpolation should enable passthrough"
        );

        let total_with_passthrough =
            superres_stage_count(true, 0) + fi_stage_count(ModelFormat::Concatenated, passthrough);
        assert_eq!(total_with_passthrough, 6);
        assert!((2..=6).contains(&total_with_passthrough));

        let total_without_passthrough =
            superres_stage_count(true, 0) + fi_stage_count(ModelFormat::Concatenated, false);
        assert_eq!(total_without_passthrough, 6);

        let no_passthrough_from_fp32 =
            should_enable_sr_to_fi_passthrough(Some("SuperResolution"), false);
        assert!(!no_passthrough_from_fp32);
    }

    #[test]
    fn test_video_compile_context_fi_sr_passthrough() {
        assert!(
            should_enable_fi_to_sr_passthrough(Some("FrameInterpolation")),
            "FrameInterpolation -> SuperResolution should enable passthrough"
        );
        assert!(
            !should_enable_fi_to_sr_passthrough(Some("SuperResolution")),
            "SuperResolution -> SuperResolution should not enable FI passthrough"
        );
        assert!(
            !should_enable_fi_to_sr_passthrough(None),
            "No previous node should not enable FI passthrough"
        );
    }

    #[test]
    fn test_video_compile_context_is_interpolator() {
        let ctx = VideoCompileContext::default();
        assert!(ctx.is_interpolator_type("FrameInterpolation"));
        assert!(!ctx.is_interpolator_type("RIFE"));
        assert!(!ctx.is_interpolator_type("SuperResolution"));
        assert!(!ctx.is_interpolator_type("VideoInput"));
    }

    #[test]
    fn video_compile_context_when_rife_input_resolution_changes_uses_separate_cache_namespace() {
        // Given: one RIFE model and a TensorRT video compile context.
        let temp = tempfile::tempdir().expect("create temporary cache fixture");
        let model_path = temp.path().join("rife.onnx");
        std::fs::write(&model_path, b"rife-model").expect("write model fixture");
        let ctx = VideoCompileContext::new(temp.path().join("trt-cache"));
        let inputs = HashMap::from([
            ("model_path".to_string(), PortData::Path(model_path)),
            ("backend".to_string(), PortData::Str("tensorrt".to_string())),
        ]);

        // When: the same model is planned first for 1080p and then for 4K input.
        ctx.output_width.set(1920);
        ctx.output_height.set(1080);
        let hd = ctx
            .resolve_trt_cache_dir(&inputs, 32)
            .expect("resolve HD cache namespace")
            .expect("TensorRT uses a cache namespace");
        let hd_reuse = ctx
            .resolve_trt_cache_dir(&inputs, 32)
            .expect("resolve matching HD cache namespace")
            .expect("TensorRT uses a cache namespace");
        ctx.output_width.set(3840);
        ctx.output_height.set(2160);
        let uhd = ctx
            .resolve_trt_cache_dir(&inputs, 32)
            .expect("resolve UHD cache namespace")
            .expect("TensorRT uses a cache namespace");

        // Then: matching effective shapes reuse a directory and incompatible shapes do not.
        assert_eq!(hd, hd_reuse);
        assert_ne!(hd, uhd);
        assert!(hd
            .file_name()
            .expect("HD cache namespace has a name")
            .to_string_lossy()
            .ends_with("1088x1920"));
        assert!(uhd
            .file_name()
            .expect("UHD cache namespace has a name")
            .to_string_lossy()
            .ends_with("2176x3840"));
    }

    #[test]
    fn video_compile_context_when_superres_tile_size_changes_uses_separate_cache_namespace() {
        // Given: one super-resolution model, resolution, and TensorRT cache root.
        let temp = tempfile::tempdir().expect("create temporary cache fixture");
        let model_path = temp.path().join("superres.onnx");
        std::fs::write(&model_path, b"superres-model").expect("write model fixture");
        let ctx = VideoCompileContext::new(temp.path().join("trt-cache"));
        ctx.output_width.set(1920);
        ctx.output_height.set(1080);
        let mut inputs = HashMap::from([
            ("model_path".to_string(), PortData::Path(model_path)),
            ("backend".to_string(), PortData::Str("tensorrt".to_string())),
            ("tile_size".to_string(), PortData::Int(64)),
        ]);

        // When: the same frame shape is planned with two supported tile sizes.
        let tile_64 = ctx
            .resolve_trt_cache_dir(&inputs, 4)
            .expect("resolve 64-pixel tile namespace")
            .expect("TensorRT uses a cache namespace");
        inputs.insert("tile_size".to_string(), PortData::Int(128));
        let tile_128 = ctx
            .resolve_trt_cache_dir(&inputs, 4)
            .expect("resolve 128-pixel tile namespace")
            .expect("TensorRT uses a cache namespace");

        // Then: incompatible tiled inference shapes do not share cached engines.
        assert_ne!(tile_64, tile_128);
    }

    #[test]
    fn video_compile_context_when_tiled_edge_regime_changes_uses_separate_cache_namespace() {
        // Given: one tiled super-resolution model and dimensions sharing one padded shape.
        let temp = tempfile::tempdir().expect("create temporary cache fixture");
        let model_path = temp.path().join("superres.onnx");
        std::fs::write(&model_path, b"superres-model").expect("write model fixture");
        let ctx = VideoCompileContext::new(temp.path().join("trt-cache"));
        let inputs = HashMap::from([
            ("model_path".to_string(), PortData::Path(model_path)),
            ("backend".to_string(), PortData::Str("tensorrt".to_string())),
            ("tile_size".to_string(), PortData::Int(65)),
        ]);

        // When: original heights 66 and 68 both align to 68 but create different edge tiles.
        ctx.output_width.set(68);
        ctx.output_height.set(66);
        let height_66 = ctx
            .resolve_trt_cache_dir(&inputs, 4)
            .expect("resolve 66-pixel input namespace")
            .expect("TensorRT uses a cache namespace");
        ctx.output_height.set(68);
        let height_68 = ctx
            .resolve_trt_cache_dir(&inputs, 4)
            .expect("resolve 68-pixel input namespace")
            .expect("TensorRT uses a cache namespace");

        // Then: incompatible edge-tile shape sets do not share cached engines.
        assert_ne!(height_66, height_68);
    }
}
