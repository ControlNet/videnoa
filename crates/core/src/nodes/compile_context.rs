use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};

use crate::compile::CompileContext;
use crate::node::{ExecutionContext, FrameProcessor, Node, PortDefinition};
use crate::streaming_executor::{FrameInterpolator, FrameSink, PipelineStage};
use crate::types::{Frame, PortData};

use crate::nodes::frame_interpolation::{
    FrameInterpolationNode, FrameInterpolationPostprocess, ModelFormat,
};
use crate::nodes::super_res::{SuperResNode, SuperResPostprocess};
use crate::nodes::video_input::{extract_metadata, run_ffprobe, VideoDecoder};
use crate::nodes::video_output::{EncoderConfig, VideoEncoder};

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
    previous_superres_fp16: Cell<bool>,
    pending_fi_emit_tensor: RefCell<Option<Arc<AtomicBool>>>,
    trt_cache_dir: PathBuf,
}

impl VideoCompileContext {
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
            previous_superres_fp16: Cell::new(false),
            pending_fi_emit_tensor: RefCell::new(None),
            trt_cache_dir,
        }
    }

    fn create_superres_node(&self, inputs: &HashMap<String, PortData>) -> Result<SuperResNode> {
        let mut node = SuperResNode::new();
        node.set_trt_cache_dir(self.trt_cache_dir.clone());
        node.execute(inputs, &ExecutionContext::default())
            .context("failed to initialize SuperResolution node")?;
        Ok(node)
    }

    fn create_fi_node(&self, inputs: &HashMap<String, PortData>) -> Result<FrameInterpolationNode> {
        let mut node = FrameInterpolationNode::new();
        node.set_trt_cache_dir(self.trt_cache_dir.clone());
        node.execute(inputs, &ExecutionContext::default())
            .context("failed to initialize FrameInterpolation node")?;
        Ok(node)
    }

    fn create_superres_stages(
        &self,
        inputs: &HashMap<String, PortData>,
    ) -> Result<Vec<PipelineStage>> {
        let node = self.create_superres_node(inputs)?;
        let scale = read_positive_u32(inputs, "scale", 4)?;
        self.output_width
            .set(self.output_width.get().saturating_mul(scale));
        self.output_height
            .set(self.output_height.get().saturating_mul(scale));

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
        self.pending_fi_emit_tensor.replace(None);

        if should_use_superres_micro_stages(node.is_fp16(), node.tile_size()) {
            let micro = node
                .into_micro_stages()
                .ok_or_else(|| anyhow!("failed to build SuperResolution micro-stages"))?;
            self.accumulated_stages
                .borrow_mut()
                .push(PipelineStage::Processor(Box::new(micro.preprocess)));
            self.accumulated_stages
                .borrow_mut()
                .push(PipelineStage::Processor(Box::new(micro.inference)));
            self.accumulated_stages
                .borrow_mut()
                .push(PipelineStage::Processor(Box::new(
                    SuperResPostprocessStage {
                        inner: micro.postprocess,
                        emit_tensor,
                    },
                )));
        } else {
            self.accumulated_stages
                .borrow_mut()
                .push(PipelineStage::Processor(Box::new(SuperResSingleStage {
                    inner: node,
                    emit_tensor,
                })));
        }

        self.previous_node_type
            .replace(Some("SuperResolution".to_string()));
        Ok(take_stages(&self.accumulated_stages))
    }

    fn create_fi_stages(&self, inputs: &HashMap<String, PortData>) -> Result<Vec<PipelineStage>> {
        let node = self.create_fi_node(inputs)?;

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

        if sr_to_fi {
            if let Some(emit_tensor) = self.pending_superres_emit_tensor.borrow().as_ref() {
                emit_tensor.store(true, Ordering::Relaxed);
            }
        }

        let fi_emit_tensor = Arc::new(AtomicBool::new(false));
        self.pending_fi_emit_tensor
            .replace(Some(Arc::clone(&fi_emit_tensor)));

        if should_use_fi_micro_stages(node.model_format(), sr_to_fi) {
            let mut micro = node
                .into_micro_stages()
                .ok_or_else(|| anyhow!("failed to build FrameInterpolation micro-stages"))?;
            self.accumulated_stages
                .borrow_mut()
                .push(PipelineStage::Processor(Box::new(micro.preprocess)));
            self.accumulated_stages
                .borrow_mut()
                .push(PipelineStage::Interpolator(Box::new(micro.inference)));
            micro.postprocess.emit_tensor = fi_emit_tensor.load(Ordering::Relaxed);
            self.accumulated_stages
                .borrow_mut()
                .push(PipelineStage::Processor(Box::new(FIPostprocessStage {
                    inner: micro.postprocess,
                    emit_tensor: fi_emit_tensor,
                })));
        } else {
            self.accumulated_stages
                .borrow_mut()
                .push(PipelineStage::Interpolator(Box::new(FISingleStage {
                    inner: node,
                    emit_tensor: fi_emit_tensor,
                })));
        }

        self.pending_superres_emit_tensor.replace(None);
        self.previous_superres_fp16.set(false);
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
    ) -> Result<(Box<dyn Iterator<Item = Result<Frame>> + Send>, Option<u64>)> {
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

        let decoder = VideoDecoder::new(&source_path, &video_info, Some("none"))
            .context("failed to create video decoder")?;

        self.source_path.replace(Some(source_path));
        self.output_width.set(video_info.width);
        self.output_height.set(video_info.height);
        self.output_fps_num.set(fps_num);
        self.output_fps_den.set(fps_den);
        self.total_output_frames.set(total_frames);
        self.previous_node_type.replace(None);
        self.pending_superres_emit_tensor.replace(None);
        self.previous_superres_fp16.set(false);
        self.pending_fi_emit_tensor.replace(None);

        Ok((Box::new(decoder), total_frames))
    }

    fn create_encoder(
        &self,
        node: &mut dyn Node,
        outputs: &HashMap<String, PortData>,
    ) -> Result<Box<dyn FrameSink>> {
        if node.node_type() != "video_output" && node.node_type() != "VideoOutput" {
            bail!("expected VideoOutput sink node, got '{}'", node.node_type());
        }

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

        let codec = match outputs.get("codec") {
            Some(PortData::Str(value)) => value.clone(),
            _ => "libx265".to_string(),
        };
        let crf = match outputs.get("crf") {
            Some(PortData::Int(value)) => *value,
            _ => 18,
        };
        let pixel_format = match outputs.get("pixel_format") {
            Some(PortData::Str(value)) => value.clone(),
            _ => "yuv420p10le".to_string(),
        };

        let width = self.output_width.get();
        let height = self.output_height.get();
        if width == 0 || height == 0 {
            bail!("output resolution is not initialized");
        }

        let config = EncoderConfig {
            source_path,
            output_path,
            codec,
            crf,
            pixel_format,
            width,
            height,
            fps: self.output_fps_string(),
            bit_depth: 8,
            cq_value: None,
            nvenc_preset: None,
            x265_preset: None,
        };

        let encoder = VideoEncoder::new(&config).context("failed to create video encoder")?;
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

        let node = self.create_superres_node(inputs)?;
        let scale = read_positive_u32(inputs, "scale", 4)?;
        self.output_width
            .set(self.output_width.get().saturating_mul(scale));
        self.output_height
            .set(self.output_height.get().saturating_mul(scale));

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
        self.previous_superres_fp16.set(false);
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
        if self.emit_tensor.load(Ordering::Relaxed) {
            if matches!(frame, Frame::NchwF16 { .. }) {
                return Ok(frame);
            }
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

fn should_use_superres_micro_stages(is_fp16_model: bool, tile_size: u32) -> bool {
    is_fp16_model && tile_size == 0
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

fn fps_to_rational(fps: f64) -> (u32, u32) {
    if !fps.is_finite() || fps <= 0.0 {
        return (24000, 1001);
    }

    let den = 1000u32;
    let num = (fps * den as f64).round() as u32;
    if num == 0 {
        return (24000, 1001);
    }

    let divisor = gcd(num, den).max(1);
    (num / divisor, den / divisor)
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_video_compile_context_fi_only() {
        assert_eq!(fi_stage_count(ModelFormat::ThreeInput, false), 1);
        assert_eq!(fi_stage_count(ModelFormat::Concatenated, false), 3);
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
}
