//! VideoOutput node: FFmpeg encode with full stream mux from source file.
//!
//! Launches an FFmpeg encode subprocess that receives raw RGB frames via stdin
//! pipe, applies zscale color-space conversion (RGB -> YUV BT.709 limited range),
//! and muxes the encoded video with ALL original non-video streams (audio, subtitle,
//! attachment, chapter) from the source file.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::thread::{self, JoinHandle};

use anyhow::{bail, Context, Result};
use tracing::{debug, info, warn};

use crate::node::{ExecutionContext, Node, PortDefinition};
use crate::streaming_executor::FrameSink;
use crate::types::{Frame, PortData, PortType};

#[derive(Debug, Clone)]
pub struct EncoderConfig {
    /// Path to the original source file (for non-video stream muxing).
    pub source_path: PathBuf,
    /// Path to the output file.
    pub output_path: PathBuf,
    /// Video codec (e.g. "libx265").
    pub codec: String,
    /// Constant Rate Factor.
    pub crf: i64,
    /// Output pixel format (e.g. "yuv420p10le").
    pub pixel_format: String,
    /// Output video width.
    pub width: u32,
    /// Output video height.
    pub height: u32,
    /// Frame rate as rational string (e.g. "24000/1001").
    pub fps: String,
    /// Input bit depth (8 or 10+). Determines rawvideo pix_fmt (rgb24 vs rgb48le).
    pub bit_depth: u8,
    /// Constant quality for NVENC (used instead of CRF). Default: 20.
    pub cq_value: Option<i64>,
    /// NVENC preset (p1-p7). Default: "p4".
    pub nvenc_preset: Option<String>,
    /// Software encoder preset (e.g. "medium", "slow", "veryslow" for x265/x264).
    pub x265_preset: Option<String>,
}

impl EncoderConfig {
    pub fn build_ffmpeg_args(&self) -> Vec<String> {
        let input_pix_fmt = if self.bit_depth > 8 {
            "rgb48le"
        } else {
            "rgb24"
        };

        let size = format!("{}x{}", self.width, self.height);

        // FFmpeg 4.4's zscale (libzimg) cannot convert directly from packed RGB
        // (rgb24/rgb48le) to YUV — it fails with "no path between colorspaces".
        // Fix: use swscale via `format=` to convert RGB→YUV first, then `setparams`
        // to label the BT.709 colorspace metadata, then `zscale` for limited-range
        // conversion with dithering.
        let vf_filter = format!(
            "format={pf},setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709,\
             zscale=range=limited:dither=error_diffusion",
            pf = self.pixel_format,
        );

        let is_nvenc = self.codec.contains("nvenc");

        let mut args: Vec<String> = vec![
            "-nostdin".into(),
            "-y".into(),
            "-f".into(),
            "rawvideo".into(),
            "-pix_fmt".into(),
            input_pix_fmt.into(),
            "-s".into(),
            size,
            "-r".into(),
            self.fps.clone(),
            "-i".into(),
            "pipe:0".into(),
            "-i".into(),
            self.source_path.to_string_lossy().into_owned(),
            "-map".into(),
            "0:v:0".into(),
            "-map".into(),
            "1".into(),
            "-map".into(),
            "-1:v".into(),
            "-c:v".into(),
            self.codec.clone(),
        ];

        if is_nvenc {
            let cq = self.cq_value.unwrap_or(20);
            let preset = self.nvenc_preset.as_deref().unwrap_or("p4");
            args.extend([
                "-rc".into(),
                "vbr".into(),
                "-cq".into(),
                cq.to_string(),
                "-preset".into(),
                preset.into(),
                "-profile:v".into(),
                "main10".into(),
                "-b:v".into(),
                "0".into(),
            ]);
        } else {
            args.extend(["-crf".into(), self.crf.to_string()]);
            if let Some(ref preset) = self.x265_preset {
                args.extend(["-preset".into(), preset.clone()]);
            }
        }

        args.extend([
            "-pix_fmt".into(),
            self.pixel_format.clone(),
            "-vf".into(),
            vf_filter,
            "-c:a".into(),
            "copy".into(),
            "-c:s".into(),
            "copy".into(),
            "-c:t".into(),
            "copy".into(),
            "-map_metadata".into(),
            "1".into(),
            "-map_chapters".into(),
            "1".into(),
            "-copy_unknown".into(),
        ]);

        if self.codec == "libx265" && self.pixel_format.contains("10") {
            args.push("-x265-params".into());
            args.push("profile=main10".into());
        }

        args.push(self.output_path.to_string_lossy().into_owned());

        args
    }

    pub fn frame_size(&self) -> usize {
        let bytes_per_pixel: usize = if self.bit_depth > 8 { 6 } else { 3 };
        self.width as usize * self.height as usize * bytes_per_pixel
    }
}

/// FFmpeg encode subprocess. Accepts raw RGB frames via stdin pipe, drains
/// stderr in a background thread, kills FFmpeg on [`Drop`].
pub struct VideoEncoder {
    child: Child,
    stdin: Option<ChildStdin>,
    stderr_thread: Option<JoinHandle<()>>,
    frame_size: usize,
    output_path: PathBuf,
}

impl VideoEncoder {
    pub fn new(config: &EncoderConfig) -> Result<Self> {
        let args = config.build_ffmpeg_args();
        let frame_size = config.frame_size();

        debug!(
            cmd = %format!("ffmpeg {}", args.join(" ")),
            "launching FFmpeg encoder"
        );

        let mut child = crate::runtime::command_for("ffmpeg")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to launch ffmpeg — is it installed?")?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("failed to open ffmpeg stdin"))?;

        let stderr = child.stderr.take().expect("stderr should be piped");
        let stderr_thread = thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                match line {
                    Ok(line) if !line.is_empty() => {
                        debug!(target: "ffmpeg_encode_stderr", "{}", line);
                    }
                    Err(e) => {
                        debug!(target: "ffmpeg_encode_stderr", "read error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
        });

        debug!(
            width = config.width,
            height = config.height,
            fps = %config.fps,
            codec = %config.codec,
            crf = config.crf,
            pix_fmt = %config.pixel_format,
            "FFmpeg encoder started"
        );

        Ok(Self {
            child,
            stdin: Some(stdin),
            stderr_thread: Some(stderr_thread),
            frame_size,
            output_path: config.output_path.clone(),
        })
    }

    /// Frame data must be exactly `width * height * bpp` bytes.
    pub fn write_frame(&mut self, data: &[u8]) -> Result<()> {
        if data.len() != self.frame_size {
            bail!(
                "frame size mismatch: expected {} bytes, got {}",
                self.frame_size,
                data.len()
            );
        }

        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("encoder stdin already closed"))?;

        stdin
            .write_all(data)
            .context("failed to write frame to ffmpeg stdin")?;

        Ok(())
    }

    pub fn finish(&mut self) -> Result<()> {
        drop(self.stdin.take());

        let status = self.child.wait().context("failed to wait for ffmpeg")?;

        if let Some(handle) = self.stderr_thread.take() {
            let _ = handle.join();
        }

        if !status.success() {
            bail!("ffmpeg encoder exited with status {}", status);
        }

        debug!("FFmpeg encoder finished successfully");

        // Post-process MKV files: regenerate track statistics tags.
        add_mkv_statistics_tags(&self.output_path);

        Ok(())
    }
}

impl Drop for VideoEncoder {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(handle) = self.stderr_thread.take() {
            let _ = handle.join();
        }
    }
}

impl FrameSink for VideoEncoder {
    fn write_frame(&mut self, frame: &Frame) -> Result<()> {
        match frame {
            Frame::CpuRgb { data, .. } => VideoEncoder::write_frame(self, data),
            Frame::NchwF16 {
                data,
                height,
                width,
            } => {
                let rgb = nchw_f16_to_rgb(data, *height as usize, *width as usize)?;
                VideoEncoder::write_frame(self, &rgb)
            }
            Frame::NchwF32 {
                data,
                height,
                width,
            } => {
                let rgb = nchw_f32_to_rgb(data, *height as usize, *width as usize)?;
                VideoEncoder::write_frame(self, &rgb)
            }
            _ => bail!("unsupported Frame variant for encoding"),
        }
    }

    fn finish(&mut self) -> Result<()> {
        VideoEncoder::finish(self)
    }
}

pub fn verify_output(output_path: &Path, expected_width: u32, expected_height: u32) -> Result<()> {
    let output = crate::runtime::command_for("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            "-show_chapters",
        ])
        .arg(output_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("failed to execute ffprobe for verification")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "ffprobe verification failed with status {}: {}",
            output.status,
            stderr.trim()
        );
    }

    let probe: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("failed to parse ffprobe JSON")?;

    let streams = probe["streams"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("ffprobe output missing streams array"))?;

    let video = streams
        .iter()
        .find(|s| s["codec_type"].as_str() == Some("video"))
        .ok_or_else(|| anyhow::anyhow!("output file has no video stream"))?;

    let width = video["width"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("video stream missing width"))?;
    let height = video["height"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("video stream missing height"))?;

    if width != expected_width as u64 || height != expected_height as u64 {
        bail!(
            "output resolution mismatch: expected {}x{}, got {}x{}",
            expected_width,
            expected_height,
            width,
            height
        );
    }

    info!(
        path = %output_path.display(),
        width = width,
        height = height,
        codec = %video["codec_name"].as_str().unwrap_or("unknown"),
        "output verification passed"
    );

    Ok(())
}

pub struct VideoOutputNode;

impl VideoOutputNode {
    pub fn new() -> Self {
        Self
    }
}

impl Default for VideoOutputNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for VideoOutputNode {
    fn node_type(&self) -> &str {
        "video_output"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![
            PortDefinition {
                name: "source_path".to_string(),
                port_type: PortType::Path,
                required: true,
                default_value: None,
            },
            PortDefinition {
                name: "output_path".to_string(),
                port_type: PortType::Path,
                required: true,
                default_value: None,
            },
            PortDefinition {
                name: "codec".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("libx265")),
            },
            PortDefinition {
                name: "crf".to_string(),
                port_type: PortType::Int,
                required: false,
                default_value: Some(serde_json::json!(18)),
            },
            PortDefinition {
                name: "pixel_format".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("yuv420p10le")),
            },
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
                name: "fps".to_string(),
                port_type: PortType::Str,
                required: true,
                default_value: None,
            },
        ]
    }

    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![PortDefinition {
            name: "output_path".to_string(),
            port_type: PortType::Path,
            required: true,
            default_value: None,
        }]
    }

    fn execute(
        &mut self,
        inputs: &HashMap<String, PortData>,
        _ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        let source_path = match inputs.get("source_path") {
            Some(PortData::Path(p)) => p.clone(),
            _ => bail!("missing or invalid 'source_path' input (expected Path)"),
        };

        let output_path = match inputs.get("output_path") {
            Some(PortData::Path(p)) => p.clone(),
            _ => bail!("missing or invalid 'output_path' input (expected Path)"),
        };

        let width = match inputs.get("width") {
            Some(PortData::Int(w)) => {
                if *w <= 0 {
                    bail!("width must be positive, got {}", w);
                }
                *w as u32
            }
            _ => bail!("missing or invalid 'width' input (expected Int)"),
        };

        let height = match inputs.get("height") {
            Some(PortData::Int(h)) => {
                if *h <= 0 {
                    bail!("height must be positive, got {}", h);
                }
                *h as u32
            }
            _ => bail!("missing or invalid 'height' input (expected Int)"),
        };

        let fps = match inputs.get("fps") {
            Some(PortData::Str(s)) => s.clone(),
            _ => bail!("missing or invalid 'fps' input (expected Str)"),
        };

        let codec = match inputs.get("codec") {
            Some(PortData::Str(s)) => s.clone(),
            _ => "libx265".to_string(),
        };

        let crf = match inputs.get("crf") {
            Some(PortData::Int(v)) => *v,
            _ => 18,
        };

        let pixel_format = match inputs.get("pixel_format") {
            Some(PortData::Str(s)) => s.clone(),
            _ => "yuv420p10le".to_string(),
        };

        if !source_path.exists() {
            bail!("source file does not exist: {}", source_path.display());
        }

        debug!(
            source = %source_path.display(),
            output = %output_path.display(),
            codec = %codec,
            crf = crf,
            pix_fmt = %pixel_format,
            width = width,
            height = height,
            fps = %fps,
            "video output config validated"
        );

        let mut outputs = HashMap::new();
        outputs.insert("output_path".to_string(), PortData::Path(output_path));
        Ok(outputs)
    }
}

pub fn encoder_config_from_inputs(
    inputs: &HashMap<String, PortData>,
    bit_depth: u8,
) -> Result<EncoderConfig> {
    let source_path = match inputs.get("source_path") {
        Some(PortData::Path(p)) => p.clone(),
        _ => bail!("missing or invalid 'source_path' input"),
    };

    let output_path = match inputs.get("output_path") {
        Some(PortData::Path(p)) => p.clone(),
        _ => bail!("missing or invalid 'output_path' input"),
    };

    let width = match inputs.get("width") {
        Some(PortData::Int(w)) => *w as u32,
        _ => bail!("missing or invalid 'width' input"),
    };

    let height = match inputs.get("height") {
        Some(PortData::Int(h)) => *h as u32,
        _ => bail!("missing or invalid 'height' input"),
    };

    let fps = match inputs.get("fps") {
        Some(PortData::Str(s)) => s.clone(),
        _ => bail!("missing or invalid 'fps' input"),
    };

    let codec = match inputs.get("codec") {
        Some(PortData::Str(s)) => s.clone(),
        _ => "libx265".to_string(),
    };

    let crf = match inputs.get("crf") {
        Some(PortData::Int(v)) => *v,
        _ => 18,
    };

    let pixel_format = match inputs.get("pixel_format") {
        Some(PortData::Str(s)) => s.clone(),
        _ => "yuv420p10le".to_string(),
    };

    Ok(EncoderConfig {
        source_path,
        output_path,
        codec,
        crf,
        pixel_format,
        width,
        height,
        fps,
        bit_depth,
        cq_value: None,
        nvenc_preset: None,
        x265_preset: None,
    })
}

/// Run `mkvpropedit --add-track-statistics-tags` on an MKV output file to
/// regenerate BPS, NUMBER_OF_FRAMES, NUMBER_OF_BYTES, and other track
/// statistics tags that FFmpeg does not produce.
///
/// Degrades gracefully: logs a warning if mkvpropedit is not installed or
/// the output is not an MKV file.
fn add_mkv_statistics_tags(output_path: &Path) {
    let ext = output_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    if !ext.eq_ignore_ascii_case("mkv") && !ext.eq_ignore_ascii_case("mka") {
        return;
    }

    match crate::runtime::command_for("mkvpropedit")
        .arg(output_path)
        .arg("--add-track-statistics-tags")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(result) if result.status.success() => {
            info!(
                path = %output_path.display(),
                "mkvpropedit: track statistics tags added"
            );
        }
        Ok(result) => {
            let stderr = String::from_utf8_lossy(&result.stderr);
            warn!(
                path = %output_path.display(),
                status = %result.status,
                stderr = %stderr.trim(),
                "mkvpropedit failed to add track statistics tags"
            );
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            warn!("mkvpropedit not found — install mkvtoolnix to regenerate MKV track statistics");
        }
        Err(e) => {
            warn!(error = %e, "failed to run mkvpropedit");
        }
    }
}

fn nchw_f16_to_rgb(data: &[u16], h: usize, w: usize) -> Result<Vec<u8>> {
    use half::f16;
    use half::slice::HalfFloatSliceExt;

    let expected = 3 * h * w;
    anyhow::ensure!(
        data.len() == expected,
        "NchwF16 length mismatch: expected {expected}, got {}",
        data.len()
    );
    let f16_slice: &[f16] =
        unsafe { std::slice::from_raw_parts(data.as_ptr() as *const f16, data.len()) };
    let mut f32_buf = vec![0.0f32; data.len()];
    f16_slice.convert_to_f32_slice(&mut f32_buf);
    nchw_f32_to_rgb(&f32_buf, h, w)
}

fn nchw_f32_to_rgb(data: &[f32], h: usize, w: usize) -> Result<Vec<u8>> {
    let expected = 3 * h * w;
    anyhow::ensure!(
        data.len() == expected,
        "NchwF32 length mismatch: expected {expected}, got {}",
        data.len()
    );
    let mut rgb = vec![0u8; expected];

    let plane_size = h * w;

    let r_plane = &data[..plane_size];
    let g_plane = &data[plane_size..2 * plane_size];
    let b_plane = &data[2 * plane_size..3 * plane_size];

    const CHUNK: usize = 4096;

    let mut offset = 0;
    while offset < plane_size {
        let len = CHUNK.min(plane_size - offset);
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
fn nchw_f32_to_rgb_scalar(data: &[f32], h: usize, w: usize) -> Result<Vec<u8>> {
    let expected = 3 * h * w;
    anyhow::ensure!(
        data.len() == expected,
        "NchwF32 length mismatch: expected {expected}, got {}",
        data.len()
    );
    let mut rgb = vec![0u8; expected];
    let plane_size = h * w;
    for y in 0..h {
        for x in 0..w {
            let idx = y * w + x;
            let r = (data[idx] * 255.0).round().clamp(0.0, 255.0) as u8;
            let g = (data[plane_size + idx] * 255.0).round().clamp(0.0, 255.0) as u8;
            let b = (data[2 * plane_size + idx] * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8;
            let out_idx = (y * w + x) * 3;
            rgb[out_idx] = r;
            rgb[out_idx + 1] = g;
            rgb[out_idx + 2] = b;
        }
    }
    Ok(rgb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Frame;
    use std::path::PathBuf;

    fn default_config() -> EncoderConfig {
        EncoderConfig {
            source_path: test_source_path(),
            output_path: test_output_path(),
            codec: "libx265".to_string(),
            crf: 18,
            pixel_format: "yuv420p10le".to_string(),
            width: 3840,
            height: 2160,
            fps: "24000/1001".to_string(),
            bit_depth: 8,
            cq_value: None,
            nvenc_preset: None,
            x265_preset: None,
        }
    }

    #[test]
    fn test_nchw_f32_to_rgb_simd_vs_scalar() {
        let h = 256;
        let w = 256;
        let plane_size = h * w;
        let mut data = vec![0.0f32; plane_size * 3];

        for c in 0..3 {
            for y in 0..h {
                for x in 0..w {
                    let idx = y * w + x;
                    let v = ((c * 1000 + idx) % 512) as f32;
                    data[c * plane_size + idx] = (v - 128.0) / 255.0;
                }
            }
        }

        let optimized = nchw_f32_to_rgb(&data, h, w).unwrap();
        let scalar = nchw_f32_to_rgb_scalar(&data, h, w).unwrap();
        assert_eq!(optimized, scalar);
    }

    #[test]
    fn test_encoder_config_frame_size_8bit() {
        let config = default_config();
        assert_eq!(config.frame_size(), 3840 * 2160 * 3);
    }

    #[test]
    fn test_encoder_config_frame_size_16bit() {
        let mut config = default_config();
        config.bit_depth = 10;
        assert_eq!(config.frame_size(), 3840 * 2160 * 6);
    }

    #[test]
    fn test_ffmpeg_args_basic_structure() {
        let config = default_config();
        let args = config.build_ffmpeg_args();

        assert_eq!(args[0], "-nostdin");
        assert_eq!(args[1], "-y");

        assert!(args.contains(&"-f".to_string()));
        assert!(args.contains(&"rawvideo".to_string()));
        assert!(args.contains(&"pipe:0".to_string()));

        let pix_idx = args.iter().position(|a| a == "-pix_fmt").unwrap();
        assert_eq!(args[pix_idx + 1], "rgb24");

        assert!(args.contains(&test_source_path().to_string_lossy().to_string()));

        assert!(args.contains(&"0:v:0".to_string()));
        assert!(args.contains(&"-1:v".to_string()));

        assert!(args.contains(&"libx265".to_string()));
        assert!(args.contains(&"18".to_string()));

        let vf_idx = args.iter().position(|a| a == "-vf").unwrap();
        let vf = &args[vf_idx + 1];
        assert!(vf.contains("format=yuv420p10le"), "vf: {vf}");
        assert!(vf.contains("setparams="), "vf: {vf}");
        assert!(vf.contains("zscale"), "vf: {vf}");
        assert!(vf.contains("bt709"), "vf: {vf}");
        assert!(vf.contains("limited"), "vf: {vf}");

        assert!(args.windows(2).any(|w| w[0] == "-c:a" && w[1] == "copy"));
        assert!(args.windows(2).any(|w| w[0] == "-c:s" && w[1] == "copy"));
        assert!(args.windows(2).any(|w| w[0] == "-c:t" && w[1] == "copy"));

        assert!(args
            .windows(2)
            .any(|w| w[0] == "-map_metadata" && w[1] == "1"));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "-map_chapters" && w[1] == "1"));

        assert!(args.contains(&"-copy_unknown".to_string()));

        assert_eq!(args.last().unwrap(), &test_output_path().to_string_lossy());
    }

    #[test]
    fn test_ffmpeg_args_10bit_input() {
        let mut config = default_config();
        config.bit_depth = 10;
        let args = config.build_ffmpeg_args();

        let pix_idx = args.iter().position(|a| a == "-pix_fmt").unwrap();
        assert_eq!(args[pix_idx + 1], "rgb48le");
    }

    #[test]
    fn test_ffmpeg_args_x265_10bit_profile() {
        let config = default_config();
        let args = config.build_ffmpeg_args();

        assert!(
            args.windows(2)
                .any(|w| w[0] == "-x265-params" && w[1] == "profile=main10"),
            "expected -x265-params profile=main10 in args: {:?}",
            args
        );
    }

    #[test]
    fn test_ffmpeg_args_no_x265_params_for_8bit_output() {
        let mut config = default_config();
        config.pixel_format = "yuv420p".to_string();
        let args = config.build_ffmpeg_args();

        assert!(
            !args.contains(&"-x265-params".to_string()),
            "should not have -x265-params for 8-bit output"
        );
    }

    #[test]
    fn test_ffmpeg_args_custom_codec() {
        let mut config = default_config();
        config.codec = "libx264".to_string();
        config.pixel_format = "yuv420p".to_string();
        let args = config.build_ffmpeg_args();

        assert!(args.contains(&"libx264".to_string()));
        assert!(!args.contains(&"-x265-params".to_string()));
    }

    #[test]
    fn test_ffmpeg_args_resolution_and_fps() {
        let config = default_config();
        let args = config.build_ffmpeg_args();

        assert!(args.contains(&"3840x2160".to_string()));
        assert!(args.contains(&"24000/1001".to_string()));
    }

    #[test]
    fn test_node_type() {
        let node = VideoOutputNode::new();
        assert_eq!(node.node_type(), "video_output");
    }

    #[test]
    fn test_node_input_ports() {
        let node = VideoOutputNode::new();
        let ports = node.input_ports();

        assert_eq!(ports.len(), 8);

        let names: Vec<&str> = ports.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"source_path"));
        assert!(names.contains(&"output_path"));
        assert!(names.contains(&"codec"));
        assert!(names.contains(&"crf"));
        assert!(names.contains(&"pixel_format"));
        assert!(names.contains(&"width"));
        assert!(names.contains(&"height"));
        assert!(names.contains(&"fps"));

        let required: Vec<&str> = ports
            .iter()
            .filter(|p| p.required)
            .map(|p| p.name.as_str())
            .collect();
        assert!(required.contains(&"source_path"));
        assert!(required.contains(&"output_path"));
        assert!(required.contains(&"width"));
        assert!(required.contains(&"height"));
        assert!(required.contains(&"fps"));
        assert!(!required.contains(&"codec"));
        assert!(!required.contains(&"crf"));
        assert!(!required.contains(&"pixel_format"));
    }

    #[test]
    fn test_node_output_ports() {
        let node = VideoOutputNode::new();
        let ports = node.output_ports();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].name, "output_path");
        assert_eq!(ports[0].port_type, PortType::Path);
    }

    #[test]
    fn test_node_execute_missing_source_path() {
        let mut node = VideoOutputNode::new();
        let ctx = ExecutionContext::default();
        let inputs = HashMap::new();
        let result = node.execute(&inputs, &ctx);
        assert!(result.is_err());
        let msg = result.err().expect("should be Err").to_string();
        assert!(msg.contains("source_path"), "error: {msg}");
    }

    #[test]
    fn test_node_execute_missing_width() {
        let mut node = VideoOutputNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert(
            "source_path".to_string(),
            PortData::Path(test_source_path()),
        );
        inputs.insert(
            "output_path".to_string(),
            PortData::Path(test_output_path()),
        );
        let result = node.execute(&inputs, &ctx);
        assert!(result.is_err());
        let msg = result.err().expect("should be Err").to_string();
        assert!(msg.contains("width"), "error: {msg}");
    }

    #[test]
    fn test_node_execute_nonexistent_source() {
        let mut node = VideoOutputNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert(
            "source_path".to_string(),
            PortData::Path(PathBuf::from("/nonexistent/video.mkv")),
        );
        inputs.insert(
            "output_path".to_string(),
            PortData::Path(test_output_path()),
        );
        inputs.insert("width".to_string(), PortData::Int(1920));
        inputs.insert("height".to_string(), PortData::Int(1080));
        inputs.insert("fps".to_string(), PortData::Str("24000/1001".to_string()));
        let result = node.execute(&inputs, &ctx);
        assert!(result.is_err());
        let msg = result.err().expect("should be Err").to_string();
        assert!(msg.contains("does not exist"), "error: {msg}");
    }

    #[test]
    fn test_node_execute_negative_width() {
        let mut node = VideoOutputNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert(
            "source_path".to_string(),
            PortData::Path(test_source_path()),
        );
        inputs.insert(
            "output_path".to_string(),
            PortData::Path(test_output_path()),
        );
        inputs.insert("width".to_string(), PortData::Int(-1));
        inputs.insert("height".to_string(), PortData::Int(1080));
        inputs.insert("fps".to_string(), PortData::Str("24000/1001".to_string()));
        let result = node.execute(&inputs, &ctx);
        assert!(result.is_err());
        let msg = result.err().expect("should be Err").to_string();
        assert!(msg.contains("positive"), "error: {msg}");
    }

    #[test]
    fn test_encoder_config_from_inputs_defaults() {
        let mut inputs = HashMap::new();
        inputs.insert(
            "source_path".to_string(),
            PortData::Path(test_source_path()),
        );
        inputs.insert(
            "output_path".to_string(),
            PortData::Path(test_output_path()),
        );
        inputs.insert("width".to_string(), PortData::Int(3840));
        inputs.insert("height".to_string(), PortData::Int(2160));
        inputs.insert("fps".to_string(), PortData::Str("24000/1001".to_string()));

        let config = encoder_config_from_inputs(&inputs, 8).unwrap();
        assert_eq!(config.codec, "libx265");
        assert_eq!(config.crf, 18);
        assert_eq!(config.pixel_format, "yuv420p10le");
        assert_eq!(config.width, 3840);
        assert_eq!(config.height, 2160);
        assert_eq!(config.fps, "24000/1001");
        assert_eq!(config.bit_depth, 8);
    }

    #[test]
    fn test_encoder_config_from_inputs_custom() {
        let mut inputs = HashMap::new();
        inputs.insert(
            "source_path".to_string(),
            PortData::Path(test_source_path()),
        );
        inputs.insert(
            "output_path".to_string(),
            PortData::Path(test_output_path()),
        );
        inputs.insert("width".to_string(), PortData::Int(1920));
        inputs.insert("height".to_string(), PortData::Int(1080));
        inputs.insert("fps".to_string(), PortData::Str("30/1".to_string()));
        inputs.insert("codec".to_string(), PortData::Str("libx264".to_string()));
        inputs.insert("crf".to_string(), PortData::Int(22));
        inputs.insert(
            "pixel_format".to_string(),
            PortData::Str("yuv420p".to_string()),
        );

        let config = encoder_config_from_inputs(&inputs, 10).unwrap();
        assert_eq!(config.codec, "libx264");
        assert_eq!(config.crf, 22);
        assert_eq!(config.pixel_format, "yuv420p");
        assert_eq!(config.bit_depth, 10);
    }

    #[test]
    fn test_ffmpeg_args_nvenc_basic() {
        let mut config = default_config();
        config.codec = "hevc_nvenc".to_string();
        config.cq_value = Some(20);
        config.nvenc_preset = Some("p4".to_string());
        let args = config.build_ffmpeg_args();

        assert!(args.contains(&"hevc_nvenc".to_string()));
        assert!(args.windows(2).any(|w| w[0] == "-rc" && w[1] == "vbr"));
        assert!(args.windows(2).any(|w| w[0] == "-cq" && w[1] == "20"));
        assert!(args.windows(2).any(|w| w[0] == "-preset" && w[1] == "p4"));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "-profile:v" && w[1] == "main10"));
        assert!(args.windows(2).any(|w| w[0] == "-b:v" && w[1] == "0"));
    }

    #[test]
    fn test_ffmpeg_args_nvenc_no_crf() {
        let mut config = default_config();
        config.codec = "hevc_nvenc".to_string();
        let args = config.build_ffmpeg_args();

        assert!(
            !args.contains(&"-crf".to_string()),
            "NVENC args must not contain -crf, got: {:?}",
            args
        );
    }

    #[test]
    fn test_ffmpeg_args_nvenc_preserves_zscale() {
        let mut config = default_config();
        config.codec = "hevc_nvenc".to_string();
        let args = config.build_ffmpeg_args();

        let vf_idx = args.iter().position(|a| a == "-vf").unwrap();
        let vf = &args[vf_idx + 1];
        assert!(vf.contains("format=yuv420p10le"), "vf: {vf}");
        assert!(vf.contains("setparams="), "vf: {vf}");
        assert!(vf.contains("zscale"), "vf: {vf}");
        assert!(vf.contains("bt709"), "vf: {vf}");
        assert!(vf.contains("limited"), "vf: {vf}");
        assert!(vf.contains("error_diffusion"), "vf: {vf}");
    }

    #[test]
    fn test_ffmpeg_args_x265_preset() {
        let mut config = default_config();
        config.x265_preset = Some("slow".to_string());
        let args = config.build_ffmpeg_args();

        assert!(args.windows(2).any(|w| w[0] == "-preset" && w[1] == "slow"));
        assert!(args.contains(&"-crf".to_string()));
    }

    #[test]
    fn test_ffmpeg_args_no_preset_by_default() {
        let config = default_config();
        let args = config.build_ffmpeg_args();

        assert!(
            !args.contains(&"-preset".to_string()),
            "default config should not include -preset, got: {:?}",
            args
        );
    }

    #[test]
    fn test_frame_sink_write_frame_cpu_rgb() {
        let cmd_name = if cfg!(windows) { "cmd" } else { "cat" };
        let mut command = std::process::Command::new(cmd_name);
        if cfg!(windows) {
            command.args(["/C", "more"]);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn mock encoder process");

        let stdin = child.stdin.take().expect("mock child stdin must be piped");
        let frame_size = 6usize;
        let mut encoder = VideoEncoder {
            child,
            stdin: Some(stdin),
            stderr_thread: None,
            frame_size,
            output_path: null_path(),
        };

        let frame = Frame::CpuRgb {
            data: vec![0, 1, 2, 3, 4, 5],
            width: 1,
            height: 2,
            bit_depth: 8,
        };

        FrameSink::write_frame(&mut encoder, &frame).expect("FrameSink write should accept CpuRgb");
        FrameSink::finish(&mut encoder).expect("mock encoder should finish successfully");
    }

    #[test]
    #[ignore]
    fn test_encode_decode_roundtrip() {
        use crate::nodes::video_input::{extract_metadata, run_ffprobe, VideoDecoder};

        let source_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../1.mkv");
        assert!(
            source_path.exists(),
            "1.mkv not found at {}",
            source_path.display()
        );

        let probe = run_ffprobe(&source_path).unwrap();
        let (info, _metadata) = extract_metadata(&probe, &source_path).unwrap();

        let decoder = VideoDecoder::new(&source_path, &info, None).unwrap();
        let frames: Vec<_> = decoder
            .take(5)
            .collect::<Result<Vec<_>, _>>()
            .expect("should decode 5 frames");
        assert_eq!(frames.len(), 5);

        let tmp_dir = tempfile::tempdir().unwrap();
        let output_path = tmp_dir.path().join("test_output.mkv");

        let config = EncoderConfig {
            source_path: source_path.clone(),
            output_path: output_path.clone(),
            codec: "libx265".to_string(),
            crf: 28,
            pixel_format: "yuv420p10le".to_string(),
            width: info.width,
            height: info.height,
            fps: format!("{}/{}", (info.fps * 1001.0).round() as u64, 1001),
            bit_depth: info.bit_depth,
            cq_value: None,
            nvenc_preset: None,
            x265_preset: None,
        };

        let mut encoder = VideoEncoder::new(&config).unwrap();
        for frame in &frames {
            match frame {
                Frame::CpuRgb { data, .. } => {
                    encoder.write_frame(data).unwrap();
                }
                _ => panic!("expected CpuRgb frame"),
            }
        }
        encoder.finish().unwrap();

        assert!(output_path.exists(), "output file should exist");

        let out_probe = run_ffprobe(&output_path).unwrap();
        let (out_info, out_meta) = extract_metadata(&out_probe, &output_path).unwrap();

        assert_eq!(out_info.width, info.width);
        assert_eq!(out_info.height, info.height);

        println!(
            "output audio streams: {}, subtitle streams: {}, attachments: {}",
            out_meta.audio_streams.len(),
            out_meta.subtitle_streams.len(),
            out_meta.attachment_streams.len()
        );

        assert!(
            out_meta.audio_streams.len() >= 1,
            "audio streams should be preserved from source"
        );

        verify_output(&output_path, info.width, info.height).unwrap();
    }

    #[test]
    fn test_add_mkv_statistics_tags_skips_non_mkv() {
        let tmp = tempfile::NamedTempFile::with_suffix(".mp4").unwrap();
        add_mkv_statistics_tags(tmp.path());
    }

    #[test]
    fn test_add_mkv_statistics_tags_handles_missing_tool() {
        let tmp = tempfile::NamedTempFile::with_suffix(".mkv").unwrap();
        add_mkv_statistics_tags(tmp.path());
    }

    fn test_source_path() -> PathBuf {
        std::env::temp_dir().join("source.mkv")
    }

    fn test_output_path() -> PathBuf {
        std::env::temp_dir().join("output.mkv")
    }

    fn null_path() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from("NUL")
        } else {
            PathBuf::from("/dev").join("null")
        }
    }
}
