//! VideoOutput node: FFmpeg encode with full stream mux from source file.
//!
//! Launches an FFmpeg encode subprocess that receives raw RGB frames via stdin
//! pipe, converts them with swscale (`scale` with the BT.709 matrix in limited
//! range, then `format` and `setparams` tags), and muxes the encoded video with
//! ALL original non-video streams (audio, subtitle, attachment, chapter) from
//! the source file.
//!
//! Matroska outputs are muxed in two steps: FFmpeg writes the encoded video to
//! a hidden file beside the output, then mkvmerge adds the source's other
//! streams. When FFmpeg copies them itself, its scheduler reads the source far
//! ahead of the slow piped video whenever a copied stream has no packet for a
//! while (font attachments never have any, subtitles pause between lines,
//! shorter audio tracks end early), so whole audio tracks can land before the
//! first video frame. Other containers keep the single FFmpeg pass.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tracing::{debug, info, warn};

use crate::frame_pool::FramePool;
use crate::node::{ExecutionContext, Node, PortDefinition};
use crate::nodes::resize::ResizeAlgorithm;
use crate::streaming_executor::FrameSink;
use crate::subprocess::{
    describe_stderr, output_with_timeout, wait_or_kill, StderrTail, STDERR_TAIL_BYTES,
};
use crate::types::{Frame, PortData, PortType};

/// Test frame for the NVENC probe. NVENC enforces a minimum frame size (an
/// A40 rejects anything below 144x144 with "Frame dimensions are less than
/// the minimum supported value"), so the probe must not use a tiny frame.
const NVENC_PROBE_SIZE: &str = "256x256";
/// Longest an NVENC probe may run before it is killed and reported as failed.
const NVENC_PROBE_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest `mkvmerge --identify` may take on the source (it may sit on slow
/// network storage).
const MKVMERGE_IDENTIFY_TIMEOUT: Duration = Duration::from_secs(120);

/// The encoder settings an NVENC probe exercises.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct NvencProbeKey {
    codec: String,
    pixel_format: String,
    preset: String,
    profile: &'static str,
}

/// NVENC configurations that passed their probe in this process; the GPU and
/// FFmpeg do not change while it runs.
fn probed_nvenc_configs() -> &'static Mutex<HashSet<NvencProbeKey>> {
    static PROBED: OnceLock<Mutex<HashSet<NvencProbeKey>>> = OnceLock::new();
    PROBED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// One spatial resample applied by FFmpeg before pixel-format conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputScale {
    pub width: u32,
    pub height: u32,
    pub algorithm: ResizeAlgorithm,
}

/// swscale options converting RGB to BT.709 limited-range YUV.
pub(crate) const BT709_LIMITED_OUTPUT: &str = "out_color_matrix=bt709:out_range=limited";
/// `setparams` filter tagging frames as BT.709 limited range.
pub(crate) const BT709_LIMITED_TAGS: &str =
    "setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=limited";

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
    /// Width of the raw frames written to the FFmpeg pipe.
    pub width: u32,
    /// Height of the raw frames written to the FFmpeg pipe.
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
    /// Resamples applied in order to the piped frames before encoding.
    pub output_scales: Vec<OutputScale>,
}

impl EncoderConfig {
    /// Final encoded resolution after all output scales.
    pub fn output_dimensions(&self) -> (u32, u32) {
        self.output_scales
            .last()
            .map_or((self.width, self.height), |scale| {
                (scale.width, scale.height)
            })
    }

    fn is_nvenc(&self) -> bool {
        self.codec == "hevc_nvenc" || self.codec == "h264_nvenc"
    }

    fn output_is_matroska(&self) -> bool {
        self.output_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("mkv") || extension.eq_ignore_ascii_case("mka")
            })
    }

    /// Hidden file beside the output that holds the encoded video until
    /// mkvmerge adds the source's other streams.
    pub fn video_only_path(&self) -> PathBuf {
        let stem = self
            .output_path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.output_path
            .with_file_name(format!(".{stem}.videnoa-video.mkv"))
    }

    /// mkvmerge arguments that put the encoded video first and append every
    /// non-video stream, chapter, tag and attachment of the source. Global
    /// tags come from the source only; mkvmerge writes the track statistics.
    pub fn build_mkvmerge_args(&self) -> Vec<String> {
        vec![
            "--quiet".into(),
            "--output".into(),
            self.output_path.to_string_lossy().into_owned(),
            "--no-global-tags".into(),
            "--default-track-flag".into(),
            "0:yes".into(),
            self.video_only_path().to_string_lossy().into_owned(),
            "--no-video".into(),
            self.source_path.to_string_lossy().into_owned(),
        ]
    }

    fn nvenc_profile(&self) -> &'static str {
        if self.codec == "h264_nvenc" {
            "high"
        } else if self.pixel_format.contains("10") || self.pixel_format == "p010le" {
            "main10"
        } else {
            "main"
        }
    }

    fn nvenc_probe_error(&self, details: &str) -> anyhow::Error {
        anyhow::anyhow!(
            "NVENC probe failed for requested codec '{}'; software fallback was not applied: {}",
            self.codec,
            details.trim()
        )
    }

    /// Confirms the NVENC encoder can open with this configuration before a
    /// job streams frames into it.
    fn probe_nvenc(&self) -> Result<()> {
        self.probe_nvenc_with(|| crate::runtime::command_for("ffmpeg"))
    }

    /// Runs the probe from `command` unless the same NVENC configuration
    /// already passed in this process. Failures are not memoised so a later
    /// job reports a fresh cause (the GPU may have been busy or recovered).
    fn probe_nvenc_with(&self, command: impl FnOnce() -> Command) -> Result<()> {
        if !self.is_nvenc() {
            return Ok(());
        }
        let key = self.nvenc_probe_key();
        let mut probed = probed_nvenc_configs()
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if probed.contains(&key) {
            debug!(codec = %self.codec, "NVENC probe already passed for this configuration");
            return Ok(());
        }
        self.run_nvenc_probe(&mut command())?;
        probed.insert(key);
        Ok(())
    }

    fn nvenc_probe_key(&self) -> NvencProbeKey {
        NvencProbeKey {
            codec: self.codec.clone(),
            pixel_format: self.pixel_format.clone(),
            preset: self.nvenc_preset.as_deref().unwrap_or("p4").to_string(),
            profile: self.nvenc_profile(),
        }
    }

    fn run_nvenc_probe(&self, command: &mut Command) -> Result<()> {
        self.run_nvenc_probe_with_timeout(command, NVENC_PROBE_TIMEOUT)
    }

    fn run_nvenc_probe_with_timeout(&self, command: &mut Command, timeout: Duration) -> Result<()> {
        let cq = self.cq_value.unwrap_or(20).to_string();
        let mut child = command
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                &format!("color=size={NVENC_PROBE_SIZE}:rate=1:color=black"),
                "-frames:v",
                "1",
                "-c:v",
                &self.codec,
                "-rc",
                "vbr",
                "-cq",
                &cq,
                "-preset",
                self.nvenc_preset.as_deref().unwrap_or("p4"),
                "-profile:v",
                self.nvenc_profile(),
                "-b:v",
                "0",
                "-pix_fmt",
                &self.pixel_format,
                "-f",
                "null",
                "-",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                self.nvenc_probe_error(&format!("failed to execute FFmpeg probe: {error}"))
            })?;
        let stderr = child.stderr.take().expect("stderr should be piped");
        let mut stderr_tail = StderrTail::spawn(stderr, |line| {
            debug!(target: "ffmpeg_nvenc_probe_stderr", "{line}");
        });

        let status = wait_or_kill(&mut child, timeout).map_err(|error| {
            self.nvenc_probe_error(&format!("failed to wait for FFmpeg probe: {error}"))
        })?;
        let stderr = describe_stderr(&stderr_tail.join());
        match status {
            Some(status) if status.success() => Ok(()),
            Some(status) => {
                Err(self.nvenc_probe_error(&format!("FFmpeg exited with {status}; {stderr}")))
            }
            None => Err(self.nvenc_probe_error(&format!(
                "FFmpeg probe did not finish within {}s and was killed; {stderr}",
                timeout.as_secs(),
            ))),
        }
    }

    /// Arguments for one FFmpeg pass that encodes the piped frames and copies
    /// the source's non-video streams into the output.
    pub fn build_ffmpeg_args(&self) -> Vec<String> {
        self.ffmpeg_args(true)
    }

    /// Arguments that encode only the piped frames into
    /// [`Self::video_only_path`], for mkvmerge to finish.
    pub fn build_video_only_ffmpeg_args(&self) -> Vec<String> {
        self.ffmpeg_args(false)
    }

    fn ffmpeg_args(&self, copy_source_streams: bool) -> Vec<String> {
        let input_pix_fmt = if self.bit_depth > 8 {
            "rgb48le"
        } else {
            "rgb24"
        };

        let size = format!("{}x{}", self.width, self.height);

        // The final swscale pass resamples (if requested) and converts RGB to the
        // output YUV format with the BT.709 matrix in limited range; `setparams`
        // then tags the stream to match. swscale's default matrix is BT.601, so
        // the matrix must be explicit. Earlier resizes stay in the RGB pipe format
        // so that only the final pass converts.
        let mut scale_filters: Vec<String> = self
            .output_scales
            .iter()
            .map(|scale| {
                format!(
                    "scale={}:{}:flags={}+accurate_rnd+full_chroma_int",
                    scale.width,
                    scale.height,
                    scale.algorithm.ffmpeg_flag()
                )
            })
            .collect();
        if scale_filters.is_empty() {
            scale_filters.push("scale=flags=bicubic".to_string());
        }
        let color_conversion = format!(
            "{}:{BT709_LIMITED_OUTPUT}",
            scale_filters.pop().expect("at least one scale filter")
        );
        let rgb_resizes: String = scale_filters
            .iter()
            .map(|filter| format!("{filter},format={input_pix_fmt},"))
            .collect();
        let vf_filter = format!(
            "{rgb_resizes}{color_conversion},format={pf},{BT709_LIMITED_TAGS},setsar=1",
            pf = self.pixel_format,
        );

        let is_nvenc = self.is_nvenc();

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
        ];
        if copy_source_streams {
            args.extend(["-i".into(), self.source_path.to_string_lossy().into_owned()]);
        }
        args.extend(["-map".into(), "0:v:0".into()]);
        if copy_source_streams {
            args.extend(["-map".into(), "1".into(), "-map".into(), "-1:v".into()]);
        }
        args.extend(["-c:v".into(), self.codec.clone()]);

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
                self.nvenc_profile().into(),
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
        ]);
        if copy_source_streams {
            args.extend([
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
                "-disposition:v:0".into(),
                "default".into(),
                "-copy_unknown".into(),
            ]);
        }

        if self.codec == "libx265" && self.pixel_format.contains("10") {
            args.push("-x265-params".into());
            args.push("profile=main10".into());
        }

        if !copy_source_streams {
            args.push(self.video_only_path().to_string_lossy().into_owned());
            return args;
        }

        if self.output_is_matroska() {
            args.extend(["-default_mode".into(), "passthrough".into()]);
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
    stderr: StderrTail,
    frame_size: usize,
    /// Pipe bit depth; RGB frames of another depth are converted before writing.
    bit_depth: u8,
    output_path: PathBuf,
    /// Pending mkvmerge step; [`MuxPlan::Direct`] once nothing is left to do.
    mux: MuxPlan,
    /// Receives written frames so upstream stages can reuse their buffers.
    pool: Arc<FramePool>,
}

/// How the encoded video is combined with the source's other streams.
enum MuxPlan {
    /// FFmpeg copies them while it encodes.
    Direct,
    /// FFmpeg writes only the video to `video_only_path`; `command`
    /// (mkvmerge) then builds the output from it and the source.
    Mkvmerge {
        video_only_path: PathBuf,
        command: Box<Command>,
    },
}

impl MuxPlan {
    fn for_config(config: &EncoderConfig) -> Result<Self> {
        Self::for_config_with(config, || crate::runtime::command_for("mkvmerge"))
    }

    /// Matroska outputs use mkvmerge when it can read the source. A missing
    /// mkvmerge fails the job now rather than after hours of processing.
    fn for_config_with(config: &EncoderConfig, mkvmerge: impl Fn() -> Command) -> Result<Self> {
        if !config.output_is_matroska() {
            return Ok(Self::Direct);
        }

        let mut identify = mkvmerge();
        identify
            .args(["--identify", "--identification-format", "json"])
            .arg(&config.source_path);
        let output = output_with_timeout(&mut identify, None, MKVMERGE_IDENTIFY_TIMEOUT).map_err(
            |error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    anyhow::anyhow!(
                        "mkvmerge is required to mux MKV outputs but was not found; install the \
                         media tools bundle with scripts/setup_dev_media_tools.sh (on Windows, \
                         bin_win64.zip must include mkvmerge.exe)"
                    )
                } else {
                    anyhow::anyhow!("failed to run mkvmerge: {error}")
                }
            },
        )?;
        // mkvmerge reports its errors on stdout rather than stderr.
        output.check("mkvmerge --identify").with_context(|| {
            format!(
                "mkvmerge cannot identify {}: {}",
                config.source_path.display(),
                String::from_utf8_lossy(&output.stdout).trim()
            )
        })?;

        if mkvmerge_can_read(&output.stdout)? {
            Ok(Self::Mkvmerge {
                video_only_path: config.video_only_path(),
                command: Box::new(mkvmerge()),
            })
        } else {
            warn!(
                source = %config.source_path.display(),
                "mkvmerge cannot read the source; FFmpeg will copy its streams, which may \
                 leave audio poorly interleaved with the video"
            );
            Ok(Self::Direct)
        }
    }
}

/// Whether `mkvmerge --identify --identification-format json` output says it
/// can read the file.
fn mkvmerge_can_read(identification: &[u8]) -> Result<bool> {
    let report: serde_json::Value = serde_json::from_slice(identification)
        .context("failed to parse mkvmerge identification JSON")?;
    let container = &report["container"];
    Ok(container["recognized"].as_bool() == Some(true)
        && container["supported"].as_bool() == Some(true))
}

/// Runs the prepared mkvmerge `command`. Exit status 1 means the output was
/// written with warnings. On failure the encoded video stays at
/// `video_only_path` so the expensive encode is not lost.
fn mux_with_mkvmerge(
    command: &mut Command,
    video_only_path: &Path,
    output_path: &Path,
) -> Result<()> {
    info!(output = %output_path.display(), "mkvmerge: adding the source's other streams");
    let result = command
        .stdin(Stdio::null())
        .output()
        .context("failed to run mkvmerge")?;
    // mkvmerge reports warnings and errors on stdout.
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let report = report.trim();
    let tail =
        &report[report.floor_char_boundary(report.len().saturating_sub(STDERR_TAIL_BYTES))..];

    match result.status.code() {
        Some(0) => {}
        Some(1) => {
            warn!(output = %output_path.display(), report = %tail, "mkvmerge finished with warnings")
        }
        _ => {
            let _ = std::fs::remove_file(output_path);
            bail!(
                "mkvmerge exited with {}; the encoded video is kept at {}; output: {}",
                result.status,
                video_only_path.display(),
                if tail.is_empty() { "(none)" } else { tail }
            );
        }
    }

    if let Err(error) = std::fs::remove_file(video_only_path) {
        warn!(path = %video_only_path.display(), %error, "failed to remove the video-only file");
    }
    Ok(())
}

/// How long a failed encoder may take to exit before it is killed so its
/// stderr can be reported.
const ENCODER_FAILURE_EXIT_TIMEOUT: Duration = Duration::from_secs(5);

impl VideoEncoder {
    pub fn new(config: &EncoderConfig) -> Result<Self> {
        config.probe_nvenc()?;
        let mux = MuxPlan::for_config(config)?;
        Self::spawn_with(config, crate::runtime::command_for("ffmpeg"), mux)
    }

    /// Starts the encoder from `command` (an FFmpeg executable) with the
    /// arguments built from `config`, finishing the output as `mux` says.
    fn spawn_with(config: &EncoderConfig, mut command: Command, mut mux: MuxPlan) -> Result<Self> {
        let args = match &mut mux {
            MuxPlan::Direct => config.build_ffmpeg_args(),
            MuxPlan::Mkvmerge { command, .. } => {
                command.args(config.build_mkvmerge_args());
                config.build_video_only_ffmpeg_args()
            }
        };
        let frame_size = config.frame_size();

        debug!(
            cmd = %format!("ffmpeg {}", args.join(" ")),
            "launching FFmpeg encoder"
        );

        let mut child = command
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
        let stderr = StderrTail::spawn(stderr, |line| {
            debug!(target: "ffmpeg_encode_stderr", "{line}");
        });

        let (output_width, output_height) = config.output_dimensions();
        debug!(
            width = config.width,
            height = config.height,
            output_width,
            output_height,
            fps = %config.fps,
            codec = %config.codec,
            crf = config.crf,
            pix_fmt = %config.pixel_format,
            "FFmpeg encoder started"
        );

        Ok(Self {
            child,
            stdin: Some(stdin),
            stderr,
            frame_size,
            bit_depth: config.bit_depth,
            output_path: config.output_path.clone(),
            mux,
            pool: FramePool::shared(),
        })
    }

    pub fn set_frame_pool(&mut self, pool: Arc<FramePool>) {
        self.pool = pool;
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

        if let Err(error) = stdin.write_all(data) {
            // A broken pipe means FFmpeg already exited; its stderr says why.
            let exit = describe_encoder_exit(&mut self.child, &mut self.stderr);
            bail!("failed to write frame to ffmpeg stdin: {error}; {exit}");
        }

        Ok(())
    }

    pub fn finish(&mut self) -> Result<()> {
        drop(self.stdin.take());

        let status = self.child.wait().context("failed to wait for ffmpeg")?;
        let stderr = self.stderr.join();

        if !status.success() {
            bail!(
                "ffmpeg encoder exited with {status}; {}",
                describe_stderr(&stderr)
            );
        }

        debug!("FFmpeg encoder finished successfully");

        match std::mem::replace(&mut self.mux, MuxPlan::Direct) {
            // Post-process MKV files: regenerate track statistics tags.
            MuxPlan::Direct => add_mkv_statistics_tags(&self.output_path),
            // mkvmerge writes the track statistics tags itself.
            MuxPlan::Mkvmerge {
                video_only_path,
                mut command,
            } => mux_with_mkvmerge(&mut command, &video_only_path, &self.output_path)?,
        }

        Ok(())
    }
}

/// Reports how a failed encoder process ended, quoting its stderr tail.
/// Waits briefly for the exit, killing the process if it is still running.
pub(crate) fn describe_encoder_exit(child: &mut Child, stderr: &mut StderrTail) -> String {
    let exit = match wait_or_kill(child, ENCODER_FAILURE_EXIT_TIMEOUT) {
        Ok(Some(status)) => format!("ffmpeg exited with {status}"),
        Ok(None) => "ffmpeg was still running and has been killed".to_string(),
        Err(error) => format!("failed to wait for ffmpeg: {error}"),
    };
    format!("{exit}; {}", describe_stderr(&stderr.join()))
}

impl Drop for VideoEncoder {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.stderr.join();
        // The encode stopped before mkvmerge ran; its partial video is useless.
        if let MuxPlan::Mkvmerge {
            video_only_path, ..
        } = &self.mux
        {
            let _ = std::fs::remove_file(video_only_path);
        }
    }
}

impl VideoEncoder {
    fn write_rgb24(&mut self, rgb: &[u8]) -> Result<()> {
        if self.bit_depth > 8 {
            VideoEncoder::write_frame(self, &rgb24_to_rgb48le(rgb))
        } else {
            VideoEncoder::write_frame(self, rgb)
        }
    }
}

fn rgb48le_to_rgb24(data: &[u8]) -> Result<Vec<u8>> {
    let (samples, remainder) = data.as_chunks::<2>();
    anyhow::ensure!(
        remainder.is_empty(),
        "RGB48 frame has an odd byte length {}",
        data.len()
    );
    Ok(samples
        .iter()
        .map(|sample| {
            let value = u32::from(u16::from_le_bytes(*sample));
            ((value * 255 + 32_767) / 65_535) as u8
        })
        .collect())
}

fn rgb24_to_rgb48le(data: &[u8]) -> Vec<u8> {
    data.iter()
        .flat_map(|&value| (u16::from(value) * 257).to_le_bytes())
        .collect()
}

impl FrameSink for VideoEncoder {
    fn write_frame(&mut self, frame: &Frame) -> Result<()> {
        match frame {
            Frame::CpuRgb {
                data, bit_depth, ..
            } => match (*bit_depth > 8, self.bit_depth > 8) {
                (true, false) => VideoEncoder::write_frame(self, &rgb48le_to_rgb24(data)?),
                (false, true) => VideoEncoder::write_frame(self, &rgb24_to_rgb48le(data)),
                _ => VideoEncoder::write_frame(self, data),
            },
            Frame::NchwF16 {
                data,
                height,
                width,
            } => {
                let rgb = nchw_f16_to_rgb(data, *height as usize, *width as usize)?;
                self.write_rgb24(&rgb)
            }
            Frame::NchwF32 {
                data,
                height,
                width,
            } => {
                let rgb = nchw_f32_to_rgb(data, *height as usize, *width as usize)?;
                self.write_rgb24(&rgb)
            }
            _ => bail!("unsupported Frame variant for encoding"),
        }
    }

    fn finish(&mut self) -> Result<()> {
        VideoEncoder::finish(self)
    }

    fn release_frame(&mut self, frame: Frame) {
        self.pool.recycle_frame(frame);
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
                name: "cq_value".to_string(),
                port_type: PortType::Int,
                required: false,
                default_value: Some(serde_json::json!(20)),
            },
            PortDefinition {
                name: "nvenc_preset".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("p4")),
            },
            PortDefinition {
                name: "x265_preset".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("medium")),
            },
            PortDefinition {
                name: "pixel_format".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("yuv420p10le")),
            },
            // Optional final output size; applied as an FFmpeg scale before encoding.
            PortDefinition {
                name: "width".to_string(),
                port_type: PortType::Int,
                required: false,
                default_value: None,
            },
            PortDefinition {
                name: "height".to_string(),
                port_type: PortType::Int,
                required: false,
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

        let width = optional_positive_dimension(inputs, "width")?;
        let height = optional_positive_dimension(inputs, "height")?;

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
            width = ?width,
            height = ?height,
            "video output config validated"
        );

        let mut outputs = HashMap::new();
        outputs.insert("output_path".to_string(), PortData::Path(output_path));
        Ok(outputs)
    }
}

/// Read an optional positive `width`/`height` input.
pub(crate) fn optional_positive_dimension(
    inputs: &HashMap<String, PortData>,
    name: &str,
) -> Result<Option<u32>> {
    match inputs.get(name) {
        Some(PortData::Int(value)) => {
            if *value <= 0 {
                bail!("{name} must be positive, got {value}");
            }
            u32::try_from(*value)
                .map(Some)
                .with_context(|| format!("{name} is too large: {value}"))
        }
        Some(_) => bail!("invalid '{name}' input (expected Int)"),
        None => Ok(None),
    }
}

pub(crate) fn default_pixel_format_for_codec(codec: &str) -> &'static str {
    match codec {
        "libx264" | "h264_nvenc" => "yuv420p",
        "hevc_nvenc" => "p010le",
        _ => "yuv420p10le",
    }
}

pub(crate) fn compatible_pixel_format<'a>(codec: &str, requested: &'a str) -> &'a str {
    let compatible = match codec {
        "libx265" => matches!(requested, "yuv420p10le" | "yuv420p"),
        "hevc_nvenc" => matches!(requested, "p010le" | "yuv420p"),
        "libx264" | "h264_nvenc" => requested == "yuv420p",
        _ => true,
    };
    if compatible {
        requested
    } else {
        default_pixel_format_for_codec(codec)
    }
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
        .stdout(Stdio::piped())
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
            let stdout = String::from_utf8_lossy(&result.stdout);
            let stderr = String::from_utf8_lossy(&result.stderr);
            warn!(
                path = %output_path.display(),
                status = %result.status,
                stdout = %stdout.trim(),
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
    crate::nodes::fp16_rgb::f16_bits_nchw_to_rgb(
        data,
        crate::nodes::fp16_rgb::NchwCrop::full(h, w),
        crate::nodes::fp16_rgb::Quantization::RoundNearest,
    )
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
            output_scales: Vec::new(),
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
    fn nchw_f16_fallback_preserves_round_to_nearest() {
        let data = vec![half::f16::from_f32(0.5).to_bits(); 3];
        let rgb = nchw_f16_to_rgb(&data, 1, 1).unwrap();
        assert_eq!(rgb, vec![128, 128, 128]);
    }

    #[test]
    fn nchw_f16_fallback_rejects_wrong_length() {
        let error = nchw_f16_to_rgb(&[0; 2], 1, 1).unwrap_err();
        assert!(error.to_string().contains("expected 3, got 2"));
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
        assert!(
            vf.contains("out_color_matrix=bt709:out_range=limited"),
            "vf: {vf}"
        );
        assert!(
            vf.contains(
                "setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=limited"
            ),
            "vf: {vf}"
        );
        assert!(!vf.contains("zscale"), "vf: {vf}");

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

    fn output_scale(width: u32, height: u32, algorithm: ResizeAlgorithm) -> OutputScale {
        OutputScale {
            width,
            height,
            algorithm,
        }
    }

    fn video_filter(config: &EncoderConfig) -> String {
        let args = config.build_ffmpeg_args();
        let vf_idx = args.iter().position(|a| a == "-vf").unwrap();
        args[vf_idx + 1].clone()
    }

    #[test]
    fn output_scales_resample_before_pixel_format_conversion() {
        let mut config = default_config();
        config.width = 5120;
        config.height = 2880;
        config.output_scales = vec![output_scale(3840, 2160, ResizeAlgorithm::Lanczos)];

        let args = config.build_ffmpeg_args();
        let size_idx = args.iter().position(|a| a == "-s").unwrap();
        assert_eq!(
            args[size_idx + 1],
            "5120x2880",
            "pipe input keeps the upstream size"
        );

        let vf = video_filter(&config);
        assert!(
            vf.starts_with(
                "scale=3840:2160:flags=lanczos+accurate_rnd+full_chroma_int:\
                 out_color_matrix=bt709:out_range=limited,format=yuv420p10le,"
            ),
            "vf: {vf}"
        );
        assert_eq!(config.output_dimensions(), (3840, 2160));
    }

    #[test]
    fn output_scales_are_applied_in_order() {
        let mut config = default_config();
        config.output_scales = vec![
            output_scale(1920, 1080, ResizeAlgorithm::Bilinear),
            output_scale(1280, 720, ResizeAlgorithm::Nearest),
        ];

        let vf = video_filter(&config);
        assert!(
            vf.starts_with(
                "scale=1920:1080:flags=bilinear+accurate_rnd+full_chroma_int,format=rgb24,\
                 scale=1280:720:flags=neighbor+accurate_rnd+full_chroma_int:\
                 out_color_matrix=bt709:out_range=limited,format=yuv420p10le,"
            ),
            "vf: {vf}"
        );
        assert_eq!(config.output_dimensions(), (1280, 720));
    }

    #[test]
    fn video_filter_without_output_scales_converts_with_bt709_first() {
        let config = default_config();
        assert!(video_filter(&config).starts_with(
            "scale=flags=bicubic:out_color_matrix=bt709:out_range=limited,format=yuv420p10le,"
        ));
        assert_eq!(config.output_dimensions(), (3840, 2160));
    }

    #[test]
    fn matroska_output_preserves_source_default_track_flags() {
        // Given: an MKV output whose source can contain multiple default tracks.
        let config = default_config();

        // When: FFmpeg arguments are built for the final mux.
        let args = config.build_ffmpeg_args();

        // Then: the Matroska muxer uses source dispositions without re-inferring them.
        assert!(args
            .windows(2)
            .any(|w| w[0] == "-default_mode" && w[1] == "passthrough"));
    }

    #[test]
    fn raw_video_output_is_marked_as_the_default_video_track() {
        // Given: the generated rawvideo input has no source disposition metadata.
        let config = default_config();

        // When: FFmpeg arguments are built for the final mux.
        let args = config.build_ffmpeg_args();

        // Then: the single generated video stream remains the default video track.
        assert!(args
            .windows(2)
            .any(|w| w[0] == "-disposition:v:0" && w[1] == "default"));
    }

    #[test]
    fn video_filter_marks_ai_output_as_square_pixel() {
        // Given: raw RGB frames, which do not carry a sample aspect ratio.
        let config = default_config();

        // When: FFmpeg arguments are built for encoding.
        let args = config.build_ffmpeg_args();

        // Then: the encoded video explicitly carries square-pixel display metadata.
        let vf_idx = args.iter().position(|arg| arg == "-vf").unwrap();
        assert!(args[vf_idx + 1].contains("setsar=1"));
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

        assert_eq!(ports.len(), 10);

        let names: Vec<&str> = ports.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"source_path"));
        assert!(names.contains(&"output_path"));
        assert!(names.contains(&"codec"));
        assert!(names.contains(&"crf"));
        assert!(names.contains(&"pixel_format"));
        assert!(names.contains(&"cq_value"));
        assert!(names.contains(&"nvenc_preset"));
        assert!(names.contains(&"x265_preset"));
        assert!(names.contains(&"width"));
        assert!(names.contains(&"height"));
        assert!(!names.contains(&"fps"));

        let required: Vec<&str> = ports
            .iter()
            .filter(|p| p.required)
            .map(|p| p.name.as_str())
            .collect();
        assert!(required.contains(&"source_path"));
        assert!(required.contains(&"output_path"));
        assert!(!required.contains(&"width"));
        assert!(!required.contains(&"height"));
        assert!(!required.contains(&"codec"));
        assert!(!required.contains(&"crf"));
        assert!(!required.contains(&"pixel_format"));
        assert!(!required.contains(&"cq_value"));
        assert!(!required.contains(&"nvenc_preset"));
        assert!(!required.contains(&"x265_preset"));
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
    fn test_node_execute_accepts_missing_dimensions() {
        let source = tempfile::NamedTempFile::with_suffix(".mkv").unwrap();
        let mut node = VideoOutputNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert(
            "source_path".to_string(),
            PortData::Path(source.path().to_path_buf()),
        );
        inputs.insert(
            "output_path".to_string(),
            PortData::Path(test_output_path()),
        );
        let outputs = node
            .execute(&inputs, &ctx)
            .expect("width, height and fps are not required");
        assert!(matches!(
            outputs.get("output_path"),
            Some(PortData::Path(path)) if *path == test_output_path()
        ));
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
        let result = node.execute(&inputs, &ctx);
        assert!(result.is_err());
        let msg = result.err().expect("should be Err").to_string();
        assert!(msg.contains("positive"), "error: {msg}");
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
    fn test_ffmpeg_args_h264_nvenc_uses_high_profile() {
        let mut config = default_config();
        config.codec = "h264_nvenc".to_string();
        config.pixel_format = "yuv420p".to_string();
        let args = config.build_ffmpeg_args();

        assert!(args
            .windows(2)
            .any(|window| window[0] == "-profile:v" && window[1] == "high"));
        assert!(!args.contains(&"main10".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn nvenc_unavailable_reports_requested_codec_and_ffmpeg_cause() {
        // Given: an FFmpeg probe process that reports unsupported NVENC hardware.
        let mut config = default_config();
        config.codec = "hevc_nvenc".to_string();
        config.pixel_format = "p010le".to_string();
        let mut command = std::process::Command::new("sh");
        command.args([
            "-c",
            "printf 'OpenEncodeSessionEx failed: unsupported device (2)\\nNo capable devices found\\n' >&2; exit 7",
        ]);

        // When: the actual probe execution returns a non-zero status.
        let error = config
            .run_nvenc_probe(&mut command)
            .expect_err("unsupported NVENC hardware should fail encoder creation");
        let message = error.to_string();

        // Then: the requested codec, exit status, and original FFmpeg cause remain visible.
        assert!(message.contains("hevc_nvenc"));
        assert!(message.contains("exit status: 7"));
        assert!(message.contains("unsupported device (2)"));
        assert!(message.contains("No capable devices found"));
        assert!(message.contains("software fallback was not applied"));
    }

    #[cfg(unix)]
    #[test]
    fn unrelated_ffmpeg_failure_remains_visible() {
        // Given: an FFmpeg probe process that rejects an invalid encoder option.
        let mut config = default_config();
        config.codec = "hevc_nvenc".to_string();
        let mut command = std::process::Command::new("sh");
        command.args([
            "-c",
            "printf \"Undefined constant or missing '(' in 'invalid-preset'\\n\" >&2; exit 8",
        ]);

        // When: the actual probe execution returns a non-zero status.
        let error = config
            .run_nvenc_probe(&mut command)
            .expect_err("invalid encoder options should fail encoder creation");
        let message = error.to_string();

        // Then: configuration errors remain visible instead of being hidden.
        assert!(message.contains("exit status: 8"));
        assert!(message.contains("invalid-preset"));
        assert!(message.contains("software fallback was not applied"));
    }

    #[test]
    fn nvenc_probe_execution_failure_reports_requested_codec() {
        // Given: the selected FFmpeg probe executable does not exist.
        let mut config = default_config();
        config.codec = "hevc_nvenc".to_string();
        let temp = tempfile::tempdir().expect("temporary directory should be created");
        let mut command = std::process::Command::new(temp.path().join("missing-ffmpeg"));

        // When: the operating system cannot start the probe process.
        let error = config
            .run_nvenc_probe(&mut command)
            .expect_err("missing FFmpeg should fail encoder creation");
        let message = error.to_string();

        // Then: the execution error follows the same explicit no-fallback contract.
        assert!(message.contains("hevc_nvenc"));
        assert!(message.contains("failed to execute FFmpeg probe"));
        assert!(message.contains("software fallback was not applied"));
    }

    /// Writes an `ffmpeg` stand-in running `body` as a shell script and
    /// returns a command invoking it. The script runs through `sh` so a fork
    /// in a concurrent test cannot make exec fail with "text file busy".
    #[cfg(unix)]
    fn fake_ffmpeg(dir: &Path, name: &str, body: &str) -> impl Fn() -> std::process::Command {
        let path = dir.join(name);
        std::fs::write(&path, format!("{body}\n")).unwrap();
        move || {
            let mut command = std::process::Command::new("sh");
            command.arg(&path);
            command
        }
    }

    #[cfg(unix)]
    #[test]
    fn nvenc_probe_encodes_a_frame_large_enough_for_nvenc() {
        // Given: an FFmpeg stand-in that records the probe arguments.
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let ffmpeg = fake_ffmpeg(
            dir.path(),
            "ffmpeg",
            &format!("printf '%s\\n' \"$@\" > '{}'", args_file.display()),
        );
        let mut config = default_config();
        config.codec = "hevc_nvenc".to_string();
        config.pixel_format = "p010le".to_string();

        // When: the probe runs.
        config.run_nvenc_probe(&mut ffmpeg()).unwrap();

        // Then: the test frame is above NVENC's minimum size (an A40 rejects
        // frames smaller than 144x144) and the encoder settings match the job.
        let args: Vec<String> = std::fs::read_to_string(&args_file)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        assert!(
            args.contains(&format!("color=size={NVENC_PROBE_SIZE}:rate=1:color=black")),
            "{args:?}"
        );
        assert_eq!(NVENC_PROBE_SIZE, "256x256");
        for pair in [
            ["-c:v", "hevc_nvenc"],
            ["-pix_fmt", "p010le"],
            ["-profile:v", "main10"],
            ["-preset", "p4"],
            ["-cq", "20"],
            ["-frames:v", "1"],
            ["-f", "null"],
        ] {
            assert!(
                args.windows(2).any(|window| window == pair),
                "{pair:?} missing from {args:?}"
            );
        }
        assert_eq!(args.last().map(String::as_str), Some("-"));
    }

    #[cfg(unix)]
    #[test]
    fn nvenc_probe_is_killed_after_its_deadline() {
        // Given: an FFmpeg probe that never finishes.
        let mut config = default_config();
        config.codec = "hevc_nvenc".to_string();
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "printf 'still starting\\n' >&2; exec sleep 30"]);
        let start = std::time::Instant::now();

        // When: the deadline passes.
        let error = config
            .run_nvenc_probe_with_timeout(&mut command, Duration::from_millis(200))
            .expect_err("a hung probe should fail encoder creation");

        // Then: the probe is killed and the failure names the deadline and codec.
        assert!(start.elapsed() < Duration::from_secs(10));
        let message = error.to_string();
        assert!(message.contains("hevc_nvenc"), "{message}");
        assert!(message.contains("did not finish within"), "{message}");
        assert!(message.contains("still starting"), "{message}");
        assert!(message.contains("software fallback was not applied"));
    }

    #[cfg(unix)]
    #[test]
    fn successful_nvenc_probe_runs_once_per_configuration() {
        // Given: an FFmpeg stand-in counting its invocations, and a pixel
        // format no other test shares so the process-wide memo is isolated.
        let dir = tempfile::tempdir().unwrap();
        let count_file = dir.path().join("count");
        let ffmpeg = fake_ffmpeg(
            dir.path(),
            "ffmpeg",
            &format!("printf 'run\\n' >> '{}'", count_file.display()),
        );
        let runs = || {
            std::fs::read_to_string(&count_file)
                .map(|text| text.lines().count())
                .unwrap_or(0)
        };
        let mut config = default_config();
        config.codec = "hevc_nvenc".to_string();
        config.pixel_format = format!("memo-{}", uuid::Uuid::new_v4());

        // When: the same configuration is probed twice, then a variant.
        config.probe_nvenc_with(&ffmpeg).unwrap();
        config.probe_nvenc_with(&ffmpeg).unwrap();
        assert_eq!(runs(), 1, "a successful probe is reused");

        config.nvenc_preset = Some("p7".to_string());
        config.probe_nvenc_with(&ffmpeg).unwrap();
        assert_eq!(runs(), 2, "another preset is probed on its own");

        // Then: a failing probe is reported every time rather than memoised.
        let failing = fake_ffmpeg(
            dir.path(),
            "ffmpeg-failing",
            &format!(
                "printf 'run\\n' >> '{}'; printf 'No capable devices found\\n' >&2; exit 1",
                count_file.display()
            ),
        );
        config.nvenc_preset = Some("p1".to_string());
        for _ in 0..2 {
            let error = config.probe_nvenc_with(&failing).unwrap_err();
            assert!(error.to_string().contains("No capable devices found"));
        }
        assert_eq!(runs(), 4);
    }

    #[cfg(unix)]
    #[test]
    fn encoder_exit_failure_quotes_ffmpeg_stderr() {
        // Given: an encoder process that fails after printing its diagnosis.
        let mut command = std::process::Command::new("sh");
        command.args([
            "-c",
            "printf 'x265 [error]: unsupported pixel format\\n' >&2; exit 3",
        ]);
        let mut encoder =
            VideoEncoder::spawn_with(&default_config(), command, MuxPlan::Direct).unwrap();

        // When: the job finishes the encode.
        let message = encoder.finish().unwrap_err().to_string();

        // Then: the job error carries the exit status and FFmpeg's message.
        assert!(message.contains("exit status: 3"), "{message}");
        assert!(message.contains("unsupported pixel format"), "{message}");
    }

    #[cfg(unix)]
    #[test]
    fn encoder_write_failure_quotes_ffmpeg_stderr() {
        // Given: an encoder process that dies without reading any frame.
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "printf 'Conversion failed!\\n' >&2; exit 1"]);
        let config = default_config();
        let mut encoder = VideoEncoder::spawn_with(&config, command, MuxPlan::Direct).unwrap();

        // When: a frame larger than the pipe buffer is written.
        let frame = vec![0_u8; config.frame_size()];
        let message = encoder.write_frame(&frame).unwrap_err().to_string();

        // Then: the broken pipe is explained by FFmpeg's own output.
        assert!(message.contains("failed to write frame"), "{message}");
        assert!(message.contains("exit status: 1"), "{message}");
        assert!(message.contains("Conversion failed!"), "{message}");
    }

    #[test]
    fn video_only_args_encode_just_the_piped_frames() {
        // Given: an MKV output that mkvmerge will assemble.
        let config = default_config();

        // When: the first-stage FFmpeg arguments are built.
        let args = config.build_video_only_ffmpeg_args();

        // Then: FFmpeg reads only the pipe, so no copied source stream can
        // make its scheduler read ahead, and it writes the hidden temp file.
        let inputs: Vec<&str> = args
            .windows(2)
            .filter(|w| w[0] == "-i")
            .map(|w| w[1].as_str())
            .collect();
        assert_eq!(inputs, ["pipe:0"]);
        assert!(args.windows(2).any(|w| w[0] == "-map" && w[1] == "0:v:0"));
        for absent in ["-map_metadata", "-map_chapters", "-c:a", "-c:s", "-c:t"] {
            assert!(!args.iter().any(|a| a == absent), "{absent} in {args:?}");
        }
        assert!(args.windows(2).any(|w| w[0] == "-c:v" && w[1] == "libx265"));
        assert_eq!(video_filter(&config), {
            let vf = args.iter().position(|a| a == "-vf").unwrap();
            args[vf + 1].clone()
        });
        assert_eq!(
            args.last().unwrap(),
            &config.video_only_path().to_string_lossy()
        );
    }

    #[test]
    fn video_only_file_is_hidden_beside_the_output() {
        let mut config = default_config();
        config.output_path = PathBuf::from("/media/show/Episode 01 - AI.mkv");

        assert_eq!(
            config.video_only_path(),
            PathBuf::from("/media/show/.Episode 01 - AI.videnoa-video.mkv")
        );
    }

    #[test]
    fn mkvmerge_args_put_the_encoded_video_first_and_drop_the_source_video() {
        let config = default_config();

        let args = config.build_mkvmerge_args();

        let output = test_output_path().to_string_lossy().into_owned();
        let video = config.video_only_path().to_string_lossy().into_owned();
        let source = test_source_path().to_string_lossy().into_owned();
        assert_eq!(
            args,
            [
                "--quiet",
                "--output",
                &output,
                "--no-global-tags",
                "--default-track-flag",
                "0:yes",
                &video,
                "--no-video",
                &source,
            ]
        );
    }

    #[test]
    fn mkvmerge_identification_reports_whether_it_can_read_the_source() {
        let readable =
            br#"{"container": {"recognized": true, "supported": true, "type": "Matroska"}}"#;
        let unknown = br#"{"container": {"recognized": false, "supported": false}}"#;
        let unsupported = br#"{"container": {"recognized": true, "supported": false}}"#;

        assert!(mkvmerge_can_read(readable).unwrap());
        assert!(!mkvmerge_can_read(unknown).unwrap());
        assert!(!mkvmerge_can_read(unsupported).unwrap());
        assert!(mkvmerge_can_read(b"not json").is_err());
    }

    #[test]
    fn non_matroska_outputs_keep_the_direct_ffmpeg_mux() {
        let mut config = default_config();
        config.output_path = std::env::temp_dir().join("output.mp4");

        let plan = MuxPlan::for_config_with(&config, || {
            panic!("mkvmerge must not be consulted for MP4 outputs")
        })
        .unwrap();

        assert!(matches!(plan, MuxPlan::Direct));
    }

    #[test]
    fn missing_mkvmerge_fails_before_any_frame_is_processed() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing-mkvmerge");

        let error =
            MuxPlan::for_config_with(&default_config(), || std::process::Command::new(&missing))
                .err()
                .expect("a missing mkvmerge must fail")
                .to_string();

        assert!(error.contains("mkvmerge"), "{error}");
        assert!(error.contains("setup_dev_media_tools"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn mkvmerge_plan_depends_on_whether_it_recognises_the_source() {
        let dir = tempfile::tempdir().unwrap();
        let readable = fake_ffmpeg(
            dir.path(),
            "mkvmerge-readable",
            r#"printf '{"container": {"recognized": true, "supported": true}}'"#,
        );
        let unknown = fake_ffmpeg(
            dir.path(),
            "mkvmerge-unknown",
            r#"printf '{"container": {"recognized": false, "supported": false}}'"#,
        );

        let plan = MuxPlan::for_config_with(&default_config(), &readable).unwrap();
        assert!(matches!(plan, MuxPlan::Mkvmerge { .. }));

        // A source mkvmerge cannot read still gets its streams through FFmpeg.
        let plan = MuxPlan::for_config_with(&default_config(), &unknown).unwrap();
        assert!(matches!(plan, MuxPlan::Direct));
    }

    #[cfg(unix)]
    #[test]
    fn mkvmerge_identification_failure_quotes_its_stdout() {
        // Given: mkvmerge rejects the source, reporting on stdout as it does.
        let dir = tempfile::tempdir().unwrap();
        let failing = fake_ffmpeg(
            dir.path(),
            "mkvmerge-failing",
            "printf 'Error: the file is truncated\\n'; exit 2",
        );

        // When: the encoder checks the source before processing.
        let error = MuxPlan::for_config_with(&default_config(), &failing)
            .err()
            .expect("a rejected source must fail");

        // Then: the job error carries mkvmerge's own message.
        let message = format!("{error:#}");
        assert!(message.contains("the file is truncated"), "{message}");
        assert!(message.contains("exit status: 2"), "{message}");
    }

    /// An encoder whose FFmpeg copies stdin into its last argument (the
    /// video-only file) and whose mkvmerge writes the output, prints `report`
    /// and exits with `status`.
    #[cfg(unix)]
    fn mkvmerge_encoder(dir: &Path, report: &str, status: i32) -> (EncoderConfig, VideoEncoder) {
        let mut config = default_config();
        config.output_path = dir.join("Episode - AI.mkv");
        config.width = 4;
        config.height = 2;
        let ffmpeg = fake_ffmpeg(
            dir,
            "ffmpeg",
            r#"for arg in "$@"; do out="$arg"; done; cat > "$out""#,
        );
        let mkvmerge = fake_ffmpeg(
            dir,
            "mkvmerge",
            &format!(
                r#"printf '%s\n' "$@" > '{args}'
while [ $# -gt 0 ]; do [ "$1" = --output ] && out="$2"; shift; done
printf merged > "$out"; printf '{report}\n'; exit {status}"#,
                args = dir.join("mkvmerge-args").display(),
            ),
        );
        let plan = MuxPlan::Mkvmerge {
            video_only_path: config.video_only_path(),
            command: Box::new(mkvmerge()),
        };
        let encoder = VideoEncoder::spawn_with(&config, ffmpeg(), plan).unwrap();
        (config, encoder)
    }

    #[cfg(unix)]
    #[test]
    fn mkvmerge_assembles_the_output_and_removes_the_video_only_file() {
        // Given: an MKV encode muxed by mkvmerge.
        let dir = tempfile::tempdir().unwrap();
        let (config, mut encoder) = mkvmerge_encoder(dir.path(), "", 0);

        // When: frames are written and the encode finishes.
        let frame = vec![7_u8; config.frame_size()];
        encoder.write_frame(&frame).unwrap();
        encoder.finish().unwrap();

        // Then: mkvmerge read the encoded video, wrote the output, and the
        // temp file is gone.
        let args = std::fs::read_to_string(dir.path().join("mkvmerge-args")).unwrap();
        assert_eq!(
            args.lines().collect::<Vec<_>>(),
            config.build_mkvmerge_args()
        );
        assert_eq!(std::fs::read(&config.output_path).unwrap(), b"merged");
        assert!(!config.video_only_path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn mkvmerge_warnings_do_not_fail_the_job() {
        let dir = tempfile::tempdir().unwrap();
        let (config, mut encoder) = mkvmerge_encoder(dir.path(), "Warning: odd timestamps", 1);

        encoder.finish().unwrap();

        assert!(config.output_path.exists());
        assert!(!config.video_only_path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn mkvmerge_failure_keeps_the_encoded_video_and_quotes_mkvmerge() {
        // Given: mkvmerge fails after the expensive encode succeeded.
        let dir = tempfile::tempdir().unwrap();
        let (config, mut encoder) = mkvmerge_encoder(dir.path(), "Error: disk full", 2);
        encoder
            .write_frame(&vec![0_u8; config.frame_size()])
            .unwrap();

        // When: the encode finishes and the encoder is dropped.
        let message = encoder.finish().unwrap_err().to_string();
        drop(encoder);

        // Then: the error carries mkvmerge's message and names the kept file,
        // which still holds the encoded video for a manual mux.
        assert!(message.contains("Error: disk full"), "{message}");
        assert!(message.contains("exit status: 2"), "{message}");
        let video_only = config.video_only_path();
        assert!(
            message.contains(&*video_only.to_string_lossy()),
            "{message}"
        );
        assert_eq!(
            std::fs::read(&video_only).unwrap().len(),
            config.frame_size()
        );
    }

    #[cfg(unix)]
    #[test]
    fn abandoned_encode_removes_the_partial_video_only_file() {
        let dir = tempfile::tempdir().unwrap();
        let (config, mut encoder) = mkvmerge_encoder(dir.path(), "", 0);
        encoder
            .write_frame(&vec![0_u8; config.frame_size()])
            .unwrap();

        drop(encoder);

        assert!(!config.video_only_path().exists());
        assert!(!config.output_path.exists());
    }

    #[test]
    #[ignore = "requires an NVIDIA GPU and an FFmpeg with NVENC"]
    fn nvenc_probe_passes_on_real_hardware() {
        for (codec, pixel_format) in [("hevc_nvenc", "p010le"), ("h264_nvenc", "yuv420p")] {
            let mut config = default_config();
            config.codec = codec.to_string();
            config.pixel_format = pixel_format.to_string();
            config
                .run_nvenc_probe(&mut crate::runtime::command_for("ffmpeg"))
                .unwrap_or_else(|error| panic!("{codec}/{pixel_format}: {error}"));
        }
    }

    #[test]
    fn default_pixel_format_matches_selected_codec() {
        assert_eq!(default_pixel_format_for_codec("libx265"), "yuv420p10le");
        assert_eq!(default_pixel_format_for_codec("libx264"), "yuv420p");
        assert_eq!(default_pixel_format_for_codec("hevc_nvenc"), "p010le");
        assert_eq!(default_pixel_format_for_codec("h264_nvenc"), "yuv420p");
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
    fn test_ffmpeg_args_nvenc_uses_the_same_bt709_conversion() {
        let mut config = default_config();
        config.codec = "hevc_nvenc".to_string();
        assert_eq!(video_filter(&config), video_filter(&default_config()));
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
            stderr: StderrTail::none(),
            frame_size,
            bit_depth: 8,
            output_path: null_path(),
            mux: MuxPlan::Direct,
            pool: FramePool::shared(),
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

    #[cfg(unix)]
    #[test]
    fn released_frames_return_to_the_encoder_pool() {
        let mut child = std::process::Command::new("true")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .expect("failed to spawn mock encoder process");
        let _ = child.wait();
        let pool = FramePool::shared();
        let mut encoder = VideoEncoder {
            child,
            stdin: None,
            stderr: StderrTail::none(),
            frame_size: 6,
            bit_depth: 8,
            output_path: null_path(),
            mux: MuxPlan::Direct,
            pool: Arc::clone(&pool),
        };
        let data = pool.copy_u8(&[0, 1, 2, 3, 4, 5]);
        let pointer = data.as_ptr();

        FrameSink::release_frame(
            &mut encoder,
            Frame::CpuRgb {
                data,
                width: 1,
                height: 2,
                bit_depth: 8,
            },
        );

        let reused = pool.take_u8(6);
        assert_eq!(reused.as_ptr(), pointer);
    }

    fn capture_sink_bytes(bit_depth: u8, frame: &Frame) -> Vec<u8> {
        use std::io::Read;

        let mut child = std::process::Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn mock encoder process");
        let stdin = child.stdin.take().expect("mock child stdin must be piped");
        let mut stdout = child
            .stdout
            .take()
            .expect("mock child stdout must be piped");
        let mut encoder = VideoEncoder {
            child,
            stdin: Some(stdin),
            stderr: StderrTail::none(),
            frame_size: if bit_depth > 8 { 6 } else { 3 },
            bit_depth,
            output_path: null_path(),
            mux: MuxPlan::Direct,
            pool: FramePool::shared(),
        };

        FrameSink::write_frame(&mut encoder, frame).expect("FrameSink write should succeed");
        FrameSink::finish(&mut encoder).expect("mock encoder should finish successfully");
        let mut written = Vec::new();
        stdout.read_to_end(&mut written).unwrap();
        written
    }

    #[cfg(unix)]
    #[test]
    fn frame_sink_narrows_16bit_frames_for_8bit_pipe() {
        let rgb48: Vec<u8> = [0u16, 0x8080, 0xFFFF]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let frame = Frame::CpuRgb {
            data: rgb48,
            width: 1,
            height: 1,
            bit_depth: 16,
        };
        assert_eq!(capture_sink_bytes(8, &frame), vec![0, 128, 255]);
    }

    #[cfg(unix)]
    #[test]
    fn frame_sink_widens_8bit_frames_for_16bit_pipe() {
        let frame = Frame::CpuRgb {
            data: vec![0, 128, 255],
            width: 1,
            height: 1,
            bit_depth: 8,
        };
        let expected: Vec<u8> = [0u16, 0x8080, 0xFFFF]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        assert_eq!(capture_sink_bytes(16, &frame), expected);
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
            output_scales: Vec::new(),
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
            !out_meta.audio_streams.is_empty(),
            "audio streams should be preserved from source"
        );

        verify_output(&output_path, info.width, info.height).unwrap();
    }

    #[test]
    #[ignore = "requires ffmpeg with libx264 and libx265"]
    fn encoded_output_decodes_back_to_the_source_colors() {
        use crate::nodes::video_input::tests::{
            assert_quadrant_colors, quadrant_frame, rgb8_samples,
        };
        use crate::nodes::video_input::{extract_metadata, run_ffprobe, VideoDecoder};

        let (width, height) = (1280_u32, 720_u32);
        let dir = tempfile::tempdir().unwrap();
        let source_path = dir.path().join("source.mkv");
        let status = crate::runtime::command_for("ffmpeg")
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=size=1280x720:rate=1",
            ])
            .args(["-frames:v", "1"])
            .arg(&source_path)
            .status()
            .unwrap();
        assert!(status.success(), "failed to create the mux source");

        for (codec, pixel_format) in [("libx265", "yuv420p10le"), ("libx264", "yuv420p")] {
            let output_path = dir.path().join(format!("{codec}.mkv"));
            let config = EncoderConfig {
                source_path: source_path.clone(),
                output_path: output_path.clone(),
                codec: codec.to_string(),
                crf: 0,
                pixel_format: pixel_format.to_string(),
                width,
                height,
                fps: "24/1".to_string(),
                bit_depth: 8,
                cq_value: None,
                nvenc_preset: None,
                x265_preset: None,
                output_scales: Vec::new(),
            };
            let mut encoder = VideoEncoder::new(&config).unwrap();
            let rgb = quadrant_frame(width as usize, height as usize);
            for _ in 0..3 {
                encoder.write_frame(&rgb).unwrap();
            }
            encoder.finish().unwrap();

            let probe = run_ffprobe(&output_path).unwrap();
            let (info, _) = extract_metadata(&probe, &output_path).unwrap();
            assert_eq!(info.color_space.as_deref(), Some("bt709"), "{codec}");
            let frame = VideoDecoder::new(&output_path, &info, None)
                .unwrap()
                .next()
                .expect("one frame")
                .unwrap();
            assert_quadrant_colors(&rgb8_samples(frame), width as usize, height as usize, codec);
        }
    }

    /// Runs `ffprobe` with `args` on `path` and returns its parsed JSON.
    fn probe_json(path: &Path, args: &[&str]) -> serde_json::Value {
        let output = crate::runtime::command_for("ffprobe")
            .args(["-v", "error", "-of", "json"])
            .args(args)
            .arg(path)
            .output()
            .unwrap();
        assert!(output.status.success(), "ffprobe failed for {path:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    #[test]
    #[ignore = "requires ffmpeg, ffprobe and mkvmerge; run with the repository bin/ on PATH"]
    fn mkv_output_keeps_copied_streams_interleaved_with_slow_video() {
        // Given: a source whose streams make FFmpeg's scheduler read ahead: a
        // font attachment (never has packets), a subtitle track with a single
        // line, and a second audio track that ends after one second.
        let dir = tempfile::tempdir().unwrap();
        let source_path = dir.path().join("source.mkv");
        let subtitles = dir.path().join("line.srt");
        let font = dir.path().join("font.ttf");
        let chapters = dir.path().join("chapters.txt");
        std::fs::write(&subtitles, "1\n00:00:00,500 --> 00:00:01,500\nonly line\n").unwrap();
        // Only the attachment's presence matters to the muxers, not its bytes.
        std::fs::write(&font, vec![0x5a_u8; 4096]).unwrap();
        std::fs::write(
            &chapters,
            ";FFMETADATA1\ntitle=Source Title\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=10000\n\
             title=OP\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=10000\nEND=20000\ntitle=Main\n",
        )
        .unwrap();
        let status = crate::runtime::command_for("ffmpeg")
            .args(["-v", "error", "-y"])
            .args([
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=24:duration=20",
            ])
            .args(["-f", "lavfi", "-i", "sine=f=440:duration=20"])
            .args(["-f", "lavfi", "-i", "sine=f=880:duration=1"])
            .arg("-i")
            .arg(&subtitles)
            .arg("-i")
            .arg(&chapters)
            .args(["-map", "0", "-map", "1", "-map", "2", "-map", "3"])
            .args(["-map_metadata", "4", "-map_chapters", "4"])
            .args(["-c:v", "libx264", "-c:a", "aac", "-c:s", "ass"])
            .arg("-attach")
            .arg(&font)
            .args(["-metadata:s:t", "mimetype=font/ttf"])
            .arg(&source_path)
            .status()
            .unwrap();
        assert!(status.success(), "failed to create the mux source");

        // When: the processed video arrives only after a warm-up pause, as it
        // does while an AI model loads.
        let output_path = dir.path().join("Episode - AI.mkv");
        let config = EncoderConfig {
            source_path: source_path.clone(),
            output_path: output_path.clone(),
            codec: "libx264".to_string(),
            crf: 30,
            pixel_format: "yuv420p".to_string(),
            width: 320,
            height: 240,
            fps: "24/1".to_string(),
            bit_depth: 8,
            cq_value: None,
            nvenc_preset: None,
            x265_preset: Some("ultrafast".to_string()),
            output_scales: Vec::new(),
        };
        let mut encoder = VideoEncoder::new(&config).unwrap();
        std::thread::sleep(Duration::from_secs(3));
        let frame = vec![128_u8; config.frame_size()];
        for _ in 0..20 * 24 {
            encoder.write_frame(&frame).unwrap();
        }
        encoder.finish().unwrap();

        // Then: in file order, no audio packet is more than a second ahead of
        // the video written before it.
        let packets = probe_json(
            &output_path,
            &["-show_entries", "packet=stream_index,pts_time,pos"],
        );
        let mut packets: Vec<(u64, u64, f64)> = packets["packets"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|packet| {
                Some((
                    packet["pos"].as_str()?.parse().ok()?,
                    packet["stream_index"].as_u64()?,
                    packet["pts_time"].as_str()?.parse().ok()?,
                ))
            })
            .collect();
        packets.sort_by_key(|&(pos, ..)| pos);
        let streams = probe_json(
            &output_path,
            &["-show_streams", "-show_chapters", "-show_format"],
        );
        let stream_types: Vec<&str> = streams["streams"]
            .as_array()
            .unwrap()
            .iter()
            .map(|stream| stream["codec_type"].as_str().unwrap())
            .collect();
        let audio_indices: Vec<u64> = (0..stream_types.len() as u64)
            .filter(|&index| stream_types[index as usize] == "audio")
            .collect();
        let mut last_video = None;
        let mut worst_lead = 0.0_f64;
        for (_, stream, pts) in &packets {
            if *stream == 0 {
                last_video = Some(*pts);
            } else if audio_indices.contains(stream) {
                worst_lead = worst_lead.max(pts - last_video.unwrap_or(0.0));
            }
        }
        assert!(
            worst_lead < 1.0,
            "audio leads the video by {worst_lead:.1}s"
        );

        // And: every copied stream, the chapters and the title survive, and
        // the video-only file is gone.
        assert_eq!(
            stream_types,
            ["video", "audio", "audio", "subtitle", "attachment"]
        );
        let chapter_titles: Vec<&str> = streams["chapters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|chapter| chapter["tags"]["title"].as_str().unwrap())
            .collect();
        assert_eq!(chapter_titles, ["OP", "Main"]);
        assert_eq!(
            streams["format"]["tags"]["title"].as_str(),
            Some("Source Title")
        );
        assert_eq!(streams["streams"][0]["disposition"]["default"], 1);
        assert!(!config.video_only_path().exists());
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
