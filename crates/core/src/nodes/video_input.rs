use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use tracing::{debug, warn};

use crate::frame_pool::FramePool;
use crate::node::{ExecutionContext, Node, PortDefinition};
use crate::subprocess::output_with_timeout;
use crate::types::{Chapter, Frame, MediaMetadata, PortData, PortType, StreamInfo};
// ffprobe JSON model (serde)
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize, Debug)]
pub struct FfprobeOutput {
    streams: Vec<FfprobeStream>,
    #[serde(default)]
    chapters: Vec<FfprobeChapter>,
    format: FfprobeFormat,
}

#[derive(serde::Deserialize, Debug)]
#[allow(dead_code)]
struct FfprobeStream {
    index: usize,
    codec_name: Option<String>,
    codec_type: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    pix_fmt: Option<String>,
    r_frame_rate: Option<String>,
    avg_frame_rate: Option<String>,
    /// "tt"/"bb"/"tb"/"bt" = interlaced; absent or "progressive" = progressive
    field_order: Option<String>,
    /// "smpte2084" = PQ, "arib-std-b67" = HLG
    color_transfer: Option<String>,
    /// YUV matrix tag, e.g. "bt709"; "unknown" or absent when untagged.
    color_space: Option<String>,
    /// Colour primaries tag, e.g. "bt709"; "unknown" or absent when untagged.
    color_primaries: Option<String>,
    bits_per_raw_sample: Option<String>,
    #[serde(default)]
    tags: HashMap<String, String>,
    #[serde(default)]
    disposition: HashMap<String, serde_json::Value>,
}

#[derive(serde::Deserialize, Debug)]
struct FfprobeChapter {
    start_time: Option<String>,
    end_time: Option<String>,
    #[serde(default)]
    tags: HashMap<String, String>,
}

#[derive(serde::Deserialize, Debug)]
struct FfprobeFormat {
    format_name: Option<String>,
    #[serde(default)]
    tags: HashMap<String, String>,
}

pub(crate) fn parse_frame_rate(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() == 2 {
        let num: f64 = parts[0].parse().ok()?;
        let den: f64 = parts[1].parse().ok()?;
        if den > 0.0 {
            return Some(num / den);
        }
    }
    s.parse().ok()
}

fn detect_bit_depth(pix_fmt: &str, bits_per_raw_sample: Option<&str>) -> u8 {
    if let Some(bps) = bits_per_raw_sample {
        if let Ok(d) = bps.parse::<u8>() {
            if d > 8 {
                return d;
            }
        }
    }
    if pix_fmt.contains("10le") || pix_fmt.contains("10be") || pix_fmt.ends_with("p10") {
        return 10;
    }
    if pix_fmt.contains("12le") || pix_fmt.contains("12be") || pix_fmt.ends_with("p12") {
        return 12;
    }
    if pix_fmt.contains("16le") || pix_fmt.contains("16be") || pix_fmt.ends_with("p16") {
        return 16;
    }
    8
}

fn disposition_flag(stream: &FfprobeStream, key: &str) -> bool {
    stream
        .disposition
        .get(key)
        .and_then(|value| {
            value
                .as_bool()
                .or_else(|| value.as_i64().map(|n| n != 0))
                .or_else(|| value.as_str().map(|s| s != "0"))
        })
        .unwrap_or(false)
}

fn select_primary_video_stream(streams: &[FfprobeStream]) -> Option<&FfprobeStream> {
    streams
        .iter()
        .filter(|stream| stream.codec_type.as_deref() == Some("video"))
        .min_by_key(|stream| {
            let is_attached_picture = disposition_flag(stream, "attached_pic");
            let is_default = disposition_flag(stream, "default");
            (is_attached_picture, !is_default, stream.index)
        })
}

fn is_interlaced(field_order: Option<&str>) -> bool {
    match field_order {
        Some(fo) => matches!(fo, "tt" | "bb" | "tb" | "bt"),
        None => false,
    }
}

fn is_hdr(color_transfer: Option<&str>) -> bool {
    match color_transfer {
        Some(ct) => matches!(ct, "smpte2084" | "arib-std-b67"),
        None => false,
    }
}

/// ffprobe options for the metadata [`extract_metadata`] reads.
const FFPROBE_ARGS: [&str; 7] = [
    "-v",
    "quiet",
    "-print_format",
    "json",
    "-show_format",
    "-show_streams",
    "-show_chapters",
];

/// [`run_ffprobe`], killed when it does not finish within `timeout`.
pub(crate) fn run_ffprobe_within(path: &Path, timeout: Duration) -> Result<FfprobeOutput> {
    let mut command = crate::runtime::command_for("ffprobe");
    command.args(FFPROBE_ARGS).arg(path);
    let output = output_with_timeout(&mut command, None, timeout)
        .context("failed to execute ffprobe — is FFmpeg installed?")?;
    output.check("ffprobe")?;
    serde_json::from_slice(&output.stdout).context("failed to parse ffprobe JSON output")
}

pub fn run_ffprobe(path: &Path) -> Result<FfprobeOutput> {
    let output = crate::runtime::command_for("ffprobe")
        .args(FFPROBE_ARGS)
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("failed to execute ffprobe — is FFmpeg installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "ffprobe exited with status {}: {}",
            output.status,
            stderr.trim()
        );
    }

    let probe: FfprobeOutput =
        serde_json::from_slice(&output.stdout).context("failed to parse ffprobe JSON output")?;

    Ok(probe)
}

pub fn parse_ffprobe_json(json: &[u8]) -> Result<FfprobeOutput> {
    serde_json::from_slice(json).context("failed to parse ffprobe JSON")
}

#[derive(Debug, Clone)]
pub struct VideoStreamInfo {
    pub stream_index: usize,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub codec_name: String,
    pub pix_fmt: String,
    pub bit_depth: u8,
    /// YUV matrix tag reported by ffprobe; `None` when the stream is untagged.
    pub color_space: Option<String>,
    /// Colour primaries tag reported by ffprobe; `None` when untagged.
    pub color_primaries: Option<String>,
}

pub fn extract_metadata(
    probe: &FfprobeOutput,
    source_path: &Path,
) -> Result<(VideoStreamInfo, MediaMetadata)> {
    let video_stream = select_primary_video_stream(&probe.streams)
        .ok_or_else(|| anyhow!("no video stream found"))?;

    if is_interlaced(video_stream.field_order.as_deref()) {
        bail!(
            "interlaced content detected (field_order={}). \
             Deinterlace before processing.",
            video_stream.field_order.as_deref().unwrap_or("unknown")
        );
    }

    if is_hdr(video_stream.color_transfer.as_deref()) {
        bail!(
            "HDR content detected (color_transfer={}). \
             Only SDR content is supported.",
            video_stream.color_transfer.as_deref().unwrap_or("unknown")
        );
    }

    let width = video_stream
        .width
        .ok_or_else(|| anyhow!("video stream missing width"))?;
    let height = video_stream
        .height
        .ok_or_else(|| anyhow!("video stream missing height"))?;

    let fps_str = video_stream
        .r_frame_rate
        .as_deref()
        .or(video_stream.avg_frame_rate.as_deref())
        .unwrap_or("0/0");
    let fps = parse_frame_rate(fps_str).unwrap_or(0.0);
    if fps <= 0.0 {
        warn!("could not determine frame rate (got {fps_str}), defaulting to 23.976");
    }
    let fps = if fps <= 0.0 { 23.976 } else { fps };

    let pix_fmt = video_stream
        .pix_fmt
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let bit_depth = detect_bit_depth(&pix_fmt, video_stream.bits_per_raw_sample.as_deref());
    let codec_name = video_stream
        .codec_name
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    let video_info = VideoStreamInfo {
        stream_index: video_stream.index,
        width,
        height,
        fps,
        codec_name,
        pix_fmt,
        bit_depth,
        color_space: video_stream
            .color_space
            .clone()
            .filter(|tag| tag != "unknown"),
        color_primaries: video_stream
            .color_primaries
            .clone()
            .filter(|tag| tag != "unknown"),
    };

    let mut audio_streams = Vec::new();
    let mut subtitle_streams = Vec::new();
    let mut attachment_streams = Vec::new();

    for stream in &probe.streams {
        let codec_type = stream.codec_type.as_deref().unwrap_or("");
        let info = StreamInfo {
            index: stream.index,
            codec_name: stream.codec_name.clone().unwrap_or_default(),
            codec_type: codec_type.to_string(),
            language: stream.tags.get("language").cloned(),
            title: stream.tags.get("title").cloned(),
            metadata: stream.tags.clone(),
        };

        match codec_type {
            "audio" => audio_streams.push(info),
            "subtitle" => subtitle_streams.push(info),
            "attachment" => attachment_streams.push(info),
            _ => {}
        }
    }

    let chapters: Vec<Chapter> = probe
        .chapters
        .iter()
        .map(|ch| Chapter {
            start_time: ch
                .start_time
                .as_deref()
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0),
            end_time: ch
                .end_time
                .as_deref()
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0),
            title: ch.tags.get("title").cloned(),
        })
        .collect();

    let global_metadata = probe.format.tags.clone();
    let container_format = probe
        .format
        .format_name
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    let metadata = MediaMetadata {
        source_path: source_path.to_path_buf(),
        audio_streams,
        subtitle_streams,
        attachment_streams,
        chapters,
        global_metadata,
        container_format,
    };

    Ok((video_info, metadata))
}

pub struct VideoInputNode;

impl VideoInputNode {
    pub fn new(_params: &HashMap<String, serde_json::Value>) -> Result<Self> {
        Ok(Self)
    }
}

impl Node for VideoInputNode {
    fn node_type(&self) -> &str {
        "video_input"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![PortDefinition {
            name: "path".to_string(),
            port_type: PortType::Path,
            required: true,
            default_value: None,
        }]
    }

    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![
            PortDefinition {
                name: "metadata".to_string(),
                port_type: PortType::Metadata,
                required: true,
                default_value: None,
            },
            PortDefinition {
                name: "source_path".to_string(),
                port_type: PortType::Path,
                required: true,
                default_value: None,
            },
        ]
    }

    fn execute(
        &mut self,
        inputs: &HashMap<String, PortData>,
        _ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        let path = match inputs.get("path") {
            Some(PortData::Path(p)) => p.clone(),
            _ => bail!("missing or invalid 'path' input (expected Path)"),
        };

        if !path.exists() {
            bail!("input file does not exist: {}", path.display());
        }

        debug!(path = %path.display(), "running ffprobe");
        let probe = run_ffprobe(&path)?;
        let (_video_info, metadata) = extract_metadata(&probe, &path)?;

        debug!(
            stream_index = _video_info.stream_index,
            width = _video_info.width,
            height = _video_info.height,
            fps = _video_info.fps,
            codec = %_video_info.codec_name,
            pix_fmt = %_video_info.pix_fmt,
            bit_depth = _video_info.bit_depth,
            audio_streams = metadata.audio_streams.len(),
            subtitle_streams = metadata.subtitle_streams.len(),
            "video input probed"
        );

        let mut outputs = HashMap::new();
        outputs.insert("metadata".to_string(), PortData::Metadata(metadata));
        outputs.insert("source_path".to_string(), PortData::Path(path));
        Ok(outputs)
    }
}

/// Decodes video to raw RGB frames via FFmpeg subprocess, yielding one frame
/// at a time. Uses `rgb24` for 8-bit, `rgb48le` for 10-bit+. Drains stderr in
/// a background thread to prevent pipe deadlock. Kills FFmpeg on [`Drop`].
pub struct VideoDecoder {
    child: Child,
    width: u32,
    height: u32,
    bit_depth: u8,
    frame_size: usize,
    _stderr_thread: Option<thread::JoinHandle<()>>,
    pool: Arc<FramePool>,
    done: bool,
    #[allow(dead_code)]
    hwaccel: Option<String>,
}

/// swscale `in_color_matrix` for a source stream.
///
/// A matrix tag wins. Untagged streams are treated as BT.709 at any size;
/// without this, swscale assumes BT.601 for every untagged stream.
pub(crate) fn source_color_matrix(color_space: Option<&str>) -> &'static str {
    match color_space {
        Some("bt709") => "bt709",
        Some("bt470bg") => "bt470",
        Some("smpte170m") => "smpte170m",
        Some("smpte240m") => "smpte240m",
        Some("fcc") => "fcc",
        Some("bt2020nc" | "bt2020c") => "bt2020",
        _ => "bt709",
    }
}

/// swscale flags for the YUV -> RGB conversion of decoded and preview frames.
/// `accurate_rnd` and `full_chroma_int` match the encoder side and keep the
/// round trip within ±2 per channel on 4:2:0 sources (±3 with plain bicubic).
pub(crate) const RGB_DECODE_SCALE_FLAGS: &str = "bicubic+accurate_rnd+full_chroma_int";

/// Whether a source uses wide-gamut primaries. The decoder converts YUV to RGB
/// with the source matrix, but the output is tagged BT.709 without a primaries
/// conversion, so such sources come out visibly desaturated.
///
/// A BT.2020 matrix implies BT.2020 primaries even when the primaries tag is
/// missing; PQ/HLG BT.2020 sources are rejected as HDR before this point.
pub(crate) fn primaries_not_converted(
    color_space: Option<&str>,
    color_primaries: Option<&str>,
) -> bool {
    color_space.is_some_and(|matrix| matrix.starts_with("bt2020"))
        || color_primaries.is_some_and(|primaries| {
            // Wide gamuts only: BT.2020, XYZ (SMPTE 428) and DCI-P3 (431/432).
            // SD and other near-BT.709 primaries differ too little to warn about.
            matches!(primaries, "bt2020" | "smpte428" | "smpte431" | "smpte432")
        })
}

fn build_decoder_args(
    path: &Path,
    pix_fmt: &str,
    stream_index: usize,
    hwaccel: Option<&str>,
    color_matrix: &str,
) -> Vec<String> {
    let mut args: Vec<String> = vec!["-nostdin".to_string()];

    // FFmpeg requires -hwaccel before -i
    if let Some(accel) = hwaccel {
        if accel == "cuda" {
            args.extend(["-hwaccel".to_string(), "cuda".to_string()]);
        }
    }

    args.push("-i".to_string());
    args.push(path.to_string_lossy().into_owned());
    args.extend([
        "-map".to_string(),
        format!("0:{stream_index}"),
        // Convert to RGB explicitly so the source matrix is applied.
        "-vf".to_string(),
        format!("scale=in_color_matrix={color_matrix}:flags={RGB_DECODE_SCALE_FLAGS}"),
        "-f".to_string(),
        "rawvideo".to_string(),
        "-pix_fmt".to_string(),
        pix_fmt.to_string(),
        "-vsync".to_string(),
        "cfr".to_string(),
        "-v".to_string(),
        "error".to_string(),
        "pipe:1".to_string(),
    ]);
    args
}

/// Decodes only the first frame of `path`, exactly as [`VideoDecoder`] would
/// (same matrix, RGB format and software decode), killing FFmpeg when it does
/// not finish within `timeout`.
pub(crate) fn decode_first_frame(
    path: &Path,
    info: &VideoStreamInfo,
    timeout: Duration,
) -> Result<Frame> {
    decode_first_frame_with(crate::runtime::command_for("ffmpeg"), path, info, timeout)
}

fn decode_first_frame_with(
    mut command: std::process::Command,
    path: &Path,
    info: &VideoStreamInfo,
    timeout: Duration,
) -> Result<Frame> {
    let (pix_fmt, bytes_per_pixel) = if info.bit_depth > 8 {
        ("rgb48le", 6usize)
    } else {
        ("rgb24", 3usize)
    };
    let frame_size = info.width as usize * info.height as usize * bytes_per_pixel;
    let color_matrix = source_color_matrix(info.color_space.as_deref());
    let mut args = build_decoder_args(path, pix_fmt, info.stream_index, None, color_matrix);
    // Stop after one frame; the output target stays last.
    let output_index = args.len() - 1;
    args.splice(
        output_index..output_index,
        ["-frames:v".to_string(), "1".to_string()],
    );

    let output = output_with_timeout(command.args(&args), None, timeout)
        .context("failed to launch ffmpeg — is it installed?")?;
    output.check("ffmpeg frame decoder")?;
    let mut data = output.stdout;
    match data.len() {
        0 => bail!("ffmpeg decoded no frame from {}", path.display()),
        len if len < frame_size => {
            bail!("partial frame from ffmpeg ({len}/{frame_size} bytes)")
        }
        _ => data.truncate(frame_size),
    }
    Ok(Frame::CpuRgb {
        data,
        width: info.width,
        height: info.height,
        bit_depth: info.bit_depth.max(8),
    })
}

impl VideoDecoder {
    pub fn new(path: &Path, info: &VideoStreamInfo, hwaccel: Option<&str>) -> Result<Self> {
        let (pix_fmt, bytes_per_pixel) = if info.bit_depth > 8 {
            ("rgb48le", 6usize)
        } else {
            ("rgb24", 3usize)
        };
        let frame_size = info.width as usize * info.height as usize * bytes_per_pixel;

        let hwaccel = match hwaccel {
            Some("none") | Some("") | None => None,
            Some(other) => Some(other),
        };

        let color_matrix = source_color_matrix(info.color_space.as_deref());
        if primaries_not_converted(info.color_space.as_deref(), info.color_primaries.as_deref()) {
            warn!(
                path = %path.display(),
                color_space = info.color_space.as_deref().unwrap_or("unknown"),
                color_primaries = info.color_primaries.as_deref().unwrap_or("unknown"),
                "source colours are converted with the source matrix ({color_matrix}), but \
                 its primaries are not converted to BT.709; the BT.709-tagged output will \
                 look less saturated or shifted"
            );
        }
        let decode_args =
            build_decoder_args(path, pix_fmt, info.stream_index, hwaccel, color_matrix);

        if hwaccel == Some("cuda") {
            debug!("NVDEC hardware decode enabled (hwaccel=cuda)");
        }

        let mut child = crate::runtime::command_for("ffmpeg")
            .args(&decode_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to launch ffmpeg — is it installed?")?;

        let stderr = child.stderr.take().expect("stderr should be piped");
        let stderr_thread = thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                match line {
                    Ok(line) if !line.is_empty() => {
                        debug!(target: "ffmpeg_stderr", "{}", line);
                    }
                    Err(e) => {
                        debug!(target: "ffmpeg_stderr", "read error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
        });

        Ok(Self {
            child,
            width: info.width,
            height: info.height,
            bit_depth: if info.bit_depth > 8 {
                info.bit_depth
            } else {
                8
            },
            frame_size,
            _stderr_thread: Some(stderr_thread),
            pool: FramePool::shared(),
            done: false,
            hwaccel: hwaccel.map(|s| s.to_string()),
        })
    }

    /// Takes frame buffers from `pool`, where downstream stages return them.
    pub fn set_frame_pool(&mut self, pool: Arc<FramePool>) {
        self.pool = pool;
    }

    fn read_frame(&mut self) -> Result<Option<Frame>> {
        let stdout = self
            .child
            .stdout
            .as_mut()
            .ok_or_else(|| anyhow!("ffmpeg stdout not available"))?;

        // The read loop fills the whole buffer or fails.
        let mut data = self.pool.take_u8(self.frame_size);
        let mut total_read = 0;
        while total_read < self.frame_size {
            match stdout.read(&mut data[total_read..]) {
                Ok(0) => {
                    if total_read == 0 {
                        self.pool.recycle_u8(data);
                        return Ok(None);
                    }
                    bail!(
                        "partial frame at EOF ({total_read}/{} bytes)",
                        self.frame_size
                    );
                }
                Ok(n) => {
                    total_read += n;
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                    continue;
                }
                Err(e) => {
                    return Err(e).context("failed to read frame from ffmpeg stdout");
                }
            }
        }

        Ok(Some(Frame::CpuRgb {
            data,
            width: self.width,
            height: self.height,
            bit_depth: self.bit_depth,
        }))
    }

    pub fn finish(&mut self) -> Result<()> {
        let status = self.child.wait().context("failed to wait for ffmpeg")?;
        if !status.success() {
            bail!("ffmpeg exited with status {}", status);
        }
        Ok(())
    }
}

impl Iterator for VideoDecoder {
    type Item = Result<Frame>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.read_frame() {
            Ok(Some(frame)) => Some(Ok(frame)),
            Ok(None) => {
                self.done = true;
                self.finish().err().map(Err)
            }
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }
}

impl Drop for VideoDecoder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(handle) = self._stderr_thread.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    #[cfg(unix)]
    fn fault_injected_decoder(script: &str) -> VideoDecoder {
        // Synthetic subprocess output exercises the raw-frame protocol without a codec.
        let child = std::process::Command::new("sh")
            .args(["-c", script])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        VideoDecoder {
            child,
            width: 1,
            height: 1,
            bit_depth: 8,
            frame_size: 3,
            _stderr_thread: None,
            pool: FramePool::shared(),
            done: false,
            hwaccel: None,
        }
    }

    #[test]
    #[cfg(unix)]
    fn decoder_reports_failure_after_complete_frames() {
        let mut decoder = fault_injected_decoder("printf abc; exit 7");
        assert!(decoder.next().unwrap().is_ok());
        assert!(decoder
            .next()
            .unwrap()
            .err()
            .unwrap()
            .to_string()
            .contains("status"));
        assert!(decoder.next().is_none());
    }

    #[test]
    #[cfg(unix)]
    fn decoder_rejects_partial_frame_even_on_successful_exit() {
        let mut decoder = fault_injected_decoder("printf ab");
        assert!(decoder
            .next()
            .unwrap()
            .err()
            .unwrap()
            .to_string()
            .contains("partial frame"));
        assert!(decoder.next().is_none());
    }

    #[test]
    #[cfg(unix)]
    fn decoder_reads_frames_into_buffers_returned_to_its_pool() {
        let mut decoder = fault_injected_decoder("printf abcdef");
        let pool = FramePool::shared();
        let recycled = pool.seed_u8(3, 0);
        decoder.set_frame_pool(Arc::clone(&pool));

        let Some(Ok(Frame::CpuRgb { data, .. })) = decoder.next() else {
            panic!("expected a frame");
        };
        assert_eq!(data.as_ptr(), recycled);
        assert_eq!(data, b"abc");
        let Some(Ok(Frame::CpuRgb { data, .. })) = decoder.next() else {
            panic!("expected a second frame");
        };
        assert_eq!(data, b"def");
        assert!(decoder.next().is_none());
    }

    #[test]
    #[cfg(unix)]
    fn decoder_accepts_clean_eof() {
        let mut decoder = fault_injected_decoder("printf abc");
        assert!(decoder.next().unwrap().is_ok());
        assert!(decoder.next().is_none());
        assert!(decoder.next().is_none());
    }

    fn one_pixel_info() -> VideoStreamInfo {
        VideoStreamInfo {
            stream_index: 0,
            width: 1,
            height: 1,
            fps: 1.0,
            codec_name: "png".to_string(),
            pix_fmt: "rgb24".to_string(),
            bit_depth: 8,
            color_space: None,
            color_primaries: None,
        }
    }

    #[cfg(unix)]
    fn sh_command(script: &str) -> std::process::Command {
        // Decoder arguments follow as ignored positional parameters.
        let mut command = std::process::Command::new("sh");
        command.args(["-c", script, "ffmpeg"]);
        command
    }

    #[test]
    #[cfg(unix)]
    fn first_frame_decode_returns_one_rgb_frame() {
        let frame = decode_first_frame_with(
            sh_command("printf abc"),
            Path::new("frame.png"),
            &one_pixel_info(),
            Duration::from_secs(10),
        )
        .unwrap();
        let Frame::CpuRgb {
            data,
            width,
            height,
            bit_depth,
        } = frame
        else {
            panic!("expected an RGB frame");
        };
        assert_eq!(
            (data.as_slice(), width, height, bit_depth),
            (&b"abc"[..], 1, 1, 8)
        );
    }

    #[test]
    #[cfg(unix)]
    fn first_frame_decode_uses_the_job_decoder_arguments_for_one_frame() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("args");
        let script = format!(
            "printf '%s\\n' \"$@\" > '{}'; printf abc",
            args_file.display()
        );
        decode_first_frame_with(
            sh_command(&script),
            Path::new("frame.png"),
            &one_pixel_info(),
            Duration::from_secs(10),
        )
        .unwrap();
        let args: Vec<String> = std::fs::read_to_string(&args_file)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        let mut expected = build_decoder_args(Path::new("frame.png"), "rgb24", 0, None, "bt709");
        expected.insert(expected.len() - 1, "-frames:v".to_string());
        expected.insert(expected.len() - 1, "1".to_string());
        assert_eq!(args, expected);
    }

    #[test]
    #[cfg(unix)]
    fn first_frame_decode_rejects_missing_or_partial_frames() {
        for (script, expected) in [
            ("exit 0", "no frame"),
            ("printf ab", "partial frame"),
            (
                "printf 'Invalid data found\\n' >&2; exit 1",
                "Invalid data found",
            ),
        ] {
            let message = decode_first_frame_with(
                sh_command(script),
                Path::new("frame.png"),
                &one_pixel_info(),
                Duration::from_secs(10),
            )
            .err()
            .expect("the decode should fail")
            .to_string();
            assert!(message.contains(expected), "{script}: {message}");
        }
    }

    #[test]
    #[cfg(unix)]
    fn first_frame_decode_kills_a_hung_ffmpeg() {
        let start = std::time::Instant::now();
        let message = decode_first_frame_with(
            sh_command("exec sleep 30"),
            Path::new("frame.png"),
            &one_pixel_info(),
            Duration::from_millis(200),
        )
        .err()
        .expect("a hung decoder should fail")
        .to_string();
        assert!(start.elapsed() < Duration::from_secs(10));
        assert!(message.contains("timed out"), "{message}");
    }

    const SAMPLE_FFPROBE_JSON: &str = r#"{
        "streams": [
            {
                "index": 0,
                "codec_name": "hevc",
                "codec_type": "video",
                "width": 1920,
                "height": 1080,
                "pix_fmt": "yuv420p",
                "r_frame_rate": "24000/1001",
                "avg_frame_rate": "24000/1001",
                "tags": {
                    "BPS": "2440323",
                    "DURATION": "00:23:40.044000000"
                },
                "disposition": {}
            },
            {
                "index": 1,
                "codec_name": "aac",
                "codec_type": "audio",
                "tags": {
                    "language": "jpn",
                    "title": "Japanese"
                },
                "disposition": {}
            },
            {
                "index": 2,
                "codec_name": "aac",
                "codec_type": "audio",
                "tags": {
                    "language": "chi",
                    "title": "Chinese"
                },
                "disposition": {}
            },
            {
                "index": 3,
                "codec_name": "subrip",
                "codec_type": "subtitle",
                "tags": {
                    "language": "chi",
                    "title": "Traditional Chinese"
                },
                "disposition": {}
            },
            {
                "index": 4,
                "codec_name": "subrip",
                "codec_type": "subtitle",
                "tags": {
                    "language": "chi",
                    "title": "Simplified Chinese"
                },
                "disposition": {}
            }
        ],
        "chapters": [
            {
                "start_time": "0.000000",
                "end_time": "89.964000",
                "tags": { "title": "Opening" }
            },
            {
                "start_time": "89.964000",
                "end_time": "1330.037000",
                "tags": { "title": "Episode" }
            }
        ],
        "format": {
            "format_name": "matroska,webm",
            "tags": {
                "encoder": "libebml v1.4.4 + libmatroska v1.7.1",
                "creation_time": "2023-10-07T08:01:20.000000Z"
            }
        }
    }"#;

    #[test]
    fn test_parse_ffprobe_json() {
        let probe = parse_ffprobe_json(SAMPLE_FFPROBE_JSON.as_bytes()).unwrap();
        assert_eq!(probe.streams.len(), 5);
        assert_eq!(probe.chapters.len(), 2);
        assert_eq!(probe.format.format_name.as_deref(), Some("matroska,webm"));
    }

    #[test]
    fn test_extract_metadata_basic() {
        let probe = parse_ffprobe_json(SAMPLE_FFPROBE_JSON.as_bytes()).unwrap();
        let path = test_mkv_path();
        let (video_info, metadata) = extract_metadata(&probe, &path).unwrap();

        assert_eq!(video_info.stream_index, 0);
        assert_eq!(video_info.width, 1920);
        assert_eq!(video_info.height, 1080);
        assert!((video_info.fps - 23.976).abs() < 0.01);
        assert_eq!(video_info.codec_name, "hevc");
        assert_eq!(video_info.pix_fmt, "yuv420p");
        assert_eq!(video_info.bit_depth, 8);

        assert_eq!(metadata.source_path, path);
        assert_eq!(metadata.audio_streams.len(), 2);
        assert_eq!(metadata.subtitle_streams.len(), 2);
        assert_eq!(metadata.attachment_streams.len(), 0);
        assert_eq!(metadata.chapters.len(), 2);
        assert_eq!(metadata.container_format, "matroska,webm");
    }

    #[test]
    fn test_extract_metadata_audio_details() {
        let probe = parse_ffprobe_json(SAMPLE_FFPROBE_JSON.as_bytes()).unwrap();
        let path = test_mkv_path();
        let (_, metadata) = extract_metadata(&probe, path.as_path()).unwrap();

        let audio0 = &metadata.audio_streams[0];
        assert_eq!(audio0.index, 1);
        assert_eq!(audio0.codec_name, "aac");
        assert_eq!(audio0.codec_type, "audio");
        assert_eq!(audio0.language.as_deref(), Some("jpn"));
        assert_eq!(audio0.title.as_deref(), Some("Japanese"));

        let audio1 = &metadata.audio_streams[1];
        assert_eq!(audio1.index, 2);
        assert_eq!(audio1.language.as_deref(), Some("chi"));
    }

    #[test]
    fn test_extract_metadata_chapters() {
        let probe = parse_ffprobe_json(SAMPLE_FFPROBE_JSON.as_bytes()).unwrap();
        let path = test_mkv_path();
        let (_, metadata) = extract_metadata(&probe, path.as_path()).unwrap();

        assert_eq!(metadata.chapters.len(), 2);
        assert!((metadata.chapters[0].start_time - 0.0).abs() < 0.001);
        assert!((metadata.chapters[0].end_time - 89.964).abs() < 0.001);
        assert_eq!(metadata.chapters[0].title.as_deref(), Some("Opening"));
        assert_eq!(metadata.chapters[1].title.as_deref(), Some("Episode"));
    }

    #[test]
    fn test_reject_interlaced() {
        let json = r#"{
            "streams": [{
                "index": 0,
                "codec_name": "h264",
                "codec_type": "video",
                "width": 1920, "height": 1080,
                "pix_fmt": "yuv420p",
                "r_frame_rate": "30000/1001",
                "field_order": "tt",
                "tags": {}, "disposition": {}
            }],
            "chapters": [],
            "format": { "format_name": "matroska,webm", "tags": {} }
        }"#;

        let probe = parse_ffprobe_json(json.as_bytes()).unwrap();
        let path = test_mkv_path();
        let result = extract_metadata(&probe, path.as_path());
        assert!(result.is_err());
        let err_msg = result.err().expect("should be Err").to_string();
        assert!(
            err_msg.contains("interlaced"),
            "error should mention interlaced: {err_msg}"
        );
    }

    #[test]
    fn test_reject_hdr_pq() {
        let json = r#"{
            "streams": [{
                "index": 0,
                "codec_name": "hevc",
                "codec_type": "video",
                "width": 3840, "height": 2160,
                "pix_fmt": "yuv420p10le",
                "r_frame_rate": "24000/1001",
                "color_transfer": "smpte2084",
                "bits_per_raw_sample": "10",
                "tags": {}, "disposition": {}
            }],
            "chapters": [],
            "format": { "format_name": "matroska,webm", "tags": {} }
        }"#;

        let probe = parse_ffprobe_json(json.as_bytes()).unwrap();
        let path = test_mkv_path();
        let result = extract_metadata(&probe, path.as_path());
        assert!(result.is_err());
        let err_msg = result.err().expect("should be Err").to_string();
        assert!(
            err_msg.contains("HDR"),
            "error should mention HDR: {err_msg}"
        );
    }

    #[test]
    fn test_reject_hdr_hlg() {
        let json = r#"{
            "streams": [{
                "index": 0,
                "codec_name": "hevc",
                "codec_type": "video",
                "width": 3840, "height": 2160,
                "pix_fmt": "yuv420p10le",
                "r_frame_rate": "24000/1001",
                "color_transfer": "arib-std-b67",
                "bits_per_raw_sample": "10",
                "tags": {}, "disposition": {}
            }],
            "chapters": [],
            "format": { "format_name": "matroska,webm", "tags": {} }
        }"#;

        let probe = parse_ffprobe_json(json.as_bytes()).unwrap();
        let path = test_mkv_path();
        let result = extract_metadata(&probe, path.as_path());
        assert!(result.is_err());
        let err_msg = result.err().expect("should be Err").to_string();
        assert!(
            err_msg.contains("HDR"),
            "error should mention HDR: {err_msg}"
        );
    }

    #[test]
    fn test_accept_progressive_sdr() {
        let json = r#"{
            "streams": [{
                "index": 0,
                "codec_name": "hevc",
                "codec_type": "video",
                "width": 1920, "height": 1080,
                "pix_fmt": "yuv420p",
                "r_frame_rate": "24000/1001",
                "field_order": "progressive",
                "color_transfer": "bt709",
                "tags": {}, "disposition": {}
            }],
            "chapters": [],
            "format": { "format_name": "matroska,webm", "tags": {} }
        }"#;

        let probe = parse_ffprobe_json(json.as_bytes()).unwrap();
        let path = test_mkv_path();
        let result = extract_metadata(&probe, path.as_path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_detect_bit_depth_8bit() {
        assert_eq!(detect_bit_depth("yuv420p", None), 8);
        assert_eq!(detect_bit_depth("yuv420p", Some("8")), 8);
    }

    #[test]
    fn test_detect_bit_depth_10bit() {
        assert_eq!(detect_bit_depth("yuv420p10le", Some("10")), 10);
        assert_eq!(detect_bit_depth("yuv420p10le", None), 10);
    }

    #[test]
    fn test_detect_bit_depth_12bit() {
        assert_eq!(detect_bit_depth("yuv420p12le", Some("12")), 12);
    }

    #[test]
    fn test_parse_frame_rate() {
        let fps = parse_frame_rate("24000/1001").unwrap();
        assert!((fps - 23.976).abs() < 0.01);

        let fps = parse_frame_rate("30/1").unwrap();
        assert!((fps - 30.0).abs() < 0.001);

        assert!(parse_frame_rate("0/0").is_none());
    }

    #[test]
    fn test_is_interlaced() {
        assert!(is_interlaced(Some("tt")));
        assert!(is_interlaced(Some("bb")));
        assert!(is_interlaced(Some("tb")));
        assert!(is_interlaced(Some("bt")));
        assert!(!is_interlaced(Some("progressive")));
        assert!(!is_interlaced(None));
    }

    #[test]
    fn test_is_hdr() {
        assert!(is_hdr(Some("smpte2084")));
        assert!(is_hdr(Some("arib-std-b67")));
        assert!(!is_hdr(Some("bt709")));
        assert!(!is_hdr(None));
    }

    #[test]
    fn test_no_video_stream_error() {
        let json = r#"{
            "streams": [{
                "index": 0,
                "codec_name": "aac",
                "codec_type": "audio",
                "tags": {}, "disposition": {}
            }],
            "chapters": [],
            "format": { "format_name": "mp3", "tags": {} }
        }"#;

        let probe = parse_ffprobe_json(json.as_bytes()).unwrap();
        let path = test_mp3_path();
        let result = extract_metadata(&probe, path.as_path());
        assert!(result.is_err());
        assert!(result
            .err()
            .expect("should be Err")
            .to_string()
            .contains("no video stream"));
    }

    #[test]
    fn test_node_ports() {
        let node = VideoInputNode;
        assert_eq!(node.node_type(), "video_input");

        let inputs = node.input_ports();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].name, "path");
        assert_eq!(inputs[0].port_type, PortType::Path);
        assert!(inputs[0].required);

        let outputs = node.output_ports();
        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0].name, "metadata");
        assert_eq!(outputs[0].port_type, PortType::Metadata);
        assert_eq!(outputs[1].name, "source_path");
        assert_eq!(outputs[1].port_type, PortType::Path);
    }

    #[test]
    fn test_node_execute_missing_path() {
        let mut node = VideoInputNode;
        let ctx = ExecutionContext::default();
        let inputs = HashMap::new();
        let result = node.execute(&inputs, &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_node_execute_nonexistent_file() {
        let mut node = VideoInputNode;
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert(
            "path".to_string(),
            PortData::Path(PathBuf::from("/nonexistent/video.mkv")),
        );
        let result = node.execute(&inputs, &ctx);
        assert!(result.is_err());
        assert!(result
            .err()
            .expect("should be Err")
            .to_string()
            .contains("does not exist"));
    }

    #[test]
    fn test_10bit_source_metadata() {
        let json = r#"{
            "streams": [{
                "index": 0,
                "codec_name": "hevc",
                "codec_type": "video",
                "width": 1920, "height": 1080,
                "pix_fmt": "yuv420p10le",
                "r_frame_rate": "24000/1001",
                "bits_per_raw_sample": "10",
                "tags": {}, "disposition": {}
            }],
            "chapters": [],
            "format": { "format_name": "matroska,webm", "tags": {} }
        }"#;

        let probe = parse_ffprobe_json(json.as_bytes()).unwrap();
        let path = test_mkv_path();
        let (info, _) = extract_metadata(&probe, path.as_path()).unwrap();
        assert_eq!(info.stream_index, 0);
        assert_eq!(info.bit_depth, 10);
    }

    #[test]
    fn test_extract_metadata_prefers_non_attached_picture_video_stream() {
        let json = r#"{
            "streams": [
                {
                    "index": 0,
                    "codec_name": "mjpeg",
                    "codec_type": "video",
                    "width": 720,
                    "height": 576,
                    "pix_fmt": "yuvj420p",
                    "r_frame_rate": "0/0",
                    "avg_frame_rate": "0/0",
                    "tags": {},
                    "disposition": {"attached_pic": 1}
                },
                {
                    "index": 3,
                    "codec_name": "hevc",
                    "codec_type": "video",
                    "width": 1920,
                    "height": 1080,
                    "pix_fmt": "yuv420p10le",
                    "r_frame_rate": "24000/1001",
                    "avg_frame_rate": "24000/1001",
                    "bits_per_raw_sample": "10",
                    "tags": {},
                    "disposition": {"attached_pic": 0, "default": 1}
                }
            ],
            "chapters": [],
            "format": {"format_name": "matroska,webm", "tags": {}}
        }"#;

        let probe = parse_ffprobe_json(json.as_bytes()).unwrap();
        let path = test_mkv_path();
        let (info, _metadata) = extract_metadata(&probe, path.as_path()).unwrap();

        assert_eq!(info.stream_index, 3);
        assert_eq!(info.width, 1920);
        assert_eq!(info.height, 1080);
        assert_eq!(info.codec_name, "hevc");
        assert_eq!(info.bit_depth, 10);
    }

    #[test]
    fn test_attachment_streams_collected() {
        let json = r#"{
            "streams": [
                {
                    "index": 0,
                    "codec_name": "hevc",
                    "codec_type": "video",
                    "width": 1920, "height": 1080,
                    "pix_fmt": "yuv420p",
                    "r_frame_rate": "24000/1001",
                    "tags": {}, "disposition": {}
                },
                {
                    "index": 1,
                    "codec_name": "ttf",
                    "codec_type": "attachment",
                    "tags": { "filename": "font.ttf" },
                    "disposition": {}
                }
            ],
            "chapters": [],
            "format": { "format_name": "matroska,webm", "tags": {} }
        }"#;

        let probe = parse_ffprobe_json(json.as_bytes()).unwrap();
        let path = test_mkv_path();
        let (_, metadata) = extract_metadata(&probe, path.as_path()).unwrap();
        assert_eq!(metadata.attachment_streams.len(), 1);
        assert_eq!(metadata.attachment_streams[0].codec_name, "ttf");
        assert_eq!(metadata.attachment_streams[0].codec_type, "attachment");
    }

    #[test]
    #[ignore]
    fn test_ffprobe_real_file() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../1.mkv");
        assert!(path.exists(), "1.mkv not found at {}", path.display());

        let probe = run_ffprobe(&path).unwrap();
        let (info, metadata) = extract_metadata(&probe, &path).unwrap();

        assert_eq!(info.width, 1920);
        assert_eq!(info.height, 1080);
        assert!((info.fps - 23.976).abs() < 0.01);
        assert_eq!(info.codec_name, "hevc");
        assert_eq!(info.bit_depth, 8);

        assert!(!metadata.audio_streams.is_empty());
        assert!(!metadata.subtitle_streams.is_empty());
        println!("container: {}", metadata.container_format);
        println!("audio streams: {}", metadata.audio_streams.len());
        println!("subtitle streams: {}", metadata.subtitle_streams.len());
        println!("chapters: {}", metadata.chapters.len());
    }

    #[test]
    #[ignore]
    fn test_video_decoder_reads_frames() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../1.mkv");
        assert!(path.exists(), "1.mkv not found at {}", path.display());

        let probe = run_ffprobe(&path).unwrap();
        let (info, _) = extract_metadata(&probe, &path).unwrap();

        let mut decoder = VideoDecoder::new(&path, &info, None).unwrap();

        let mut count = 0;
        for frame_result in decoder.by_ref().take(5) {
            let frame = frame_result.unwrap();
            match frame {
                Frame::CpuRgb {
                    ref data,
                    width,
                    height,
                    bit_depth,
                } => {
                    assert_eq!(width, 1920);
                    assert_eq!(height, 1080);
                    assert_eq!(bit_depth, 8);
                    assert_eq!(data.len(), 1920 * 1080 * 3);
                    count += 1;
                }
                _ => panic!("expected CpuRgb frame"),
            }
        }
        assert_eq!(count, 5);
    }

    #[test]
    #[ignore]
    fn test_node_execute_real_file() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../1.mkv");
        assert!(path.exists(), "1.mkv not found at {}", path.display());

        let mut node = VideoInputNode;
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("path".to_string(), PortData::Path(path.clone()));

        let outputs = node.execute(&inputs, &ctx).unwrap();
        assert!(outputs.contains_key("metadata"));
        assert!(outputs.contains_key("source_path"));

        match outputs.get("source_path") {
            Some(PortData::Path(p)) => assert_eq!(p, &path),
            _ => panic!("expected Path output"),
        }

        match outputs.get("metadata") {
            Some(PortData::Metadata(m)) => {
                assert!(!m.audio_streams.is_empty());
                assert_eq!(m.source_path, path);
            }
            _ => panic!("expected Metadata output"),
        }
    }

    #[test]
    fn test_decoder_args_no_hwaccel() {
        let path = test_mkv_path();
        let args = build_decoder_args(path.as_path(), "rgb24", 4, None, "bt709");

        assert!(!args.contains(&"-hwaccel".to_string()));
        let i_idx = args.iter().position(|a| a == "-i").unwrap();
        let map_idx = args.iter().position(|a| a == "-map").unwrap();
        assert_eq!(args[i_idx + 1], path.to_string_lossy());
        assert_eq!(args[map_idx + 1], "0:4");
        assert!(args.contains(&"rawvideo".to_string()));
        assert!(args.contains(&"rgb24".to_string()));
        assert!(args.contains(&"pipe:1".to_string()));
    }

    #[test]
    fn test_decoder_args_cuda_hwaccel() {
        let path = test_mkv_path();
        let args = build_decoder_args(path.as_path(), "rgb48le", 2, Some("cuda"), "bt709");

        let hwaccel_idx = args.iter().position(|a| a == "-hwaccel").unwrap();
        let i_idx = args.iter().position(|a| a == "-i").unwrap();
        let map_idx = args.iter().position(|a| a == "-map").unwrap();

        assert_eq!(args[hwaccel_idx + 1], "cuda");
        assert!(hwaccel_idx < i_idx, "-hwaccel must come before -i");
        assert_eq!(args[map_idx + 1], "0:2");
        assert!(args.contains(&"rgb48le".to_string()));
        assert!(args.contains(&"pipe:1".to_string()));
    }

    #[test]
    fn test_decoder_args_none_string_hwaccel() {
        let path = test_mkv_path();
        let args = build_decoder_args(path.as_path(), "rgb24", 0, Some("none"), "bt709");

        assert!(!args.contains(&"-hwaccel".to_string()));
    }

    #[test]
    fn test_decoder_args_unknown_hwaccel_ignored() {
        let path = test_mkv_path();
        let args = build_decoder_args(path.as_path(), "rgb24", 7, Some("vulkan"), "bt709");

        assert!(!args.contains(&"-hwaccel".to_string()));
        let map_idx = args.iter().position(|a| a == "-map").unwrap();
        assert_eq!(args[map_idx + 1], "0:7");
    }

    #[test]
    fn decoder_args_convert_to_rgb_with_the_given_matrix() {
        let path = test_mkv_path();
        let args = build_decoder_args(path.as_path(), "rgb24", 0, None, "bt601");

        let i_idx = args.iter().position(|a| a == "-i").unwrap();
        let vf_idx = args.iter().position(|a| a == "-vf").unwrap();
        assert!(vf_idx > i_idx, "-vf must be an output option");
        assert_eq!(
            args[vf_idx + 1],
            "scale=in_color_matrix=bt601:flags=bicubic+accurate_rnd+full_chroma_int"
        );
    }

    #[test]
    fn source_color_matrix_follows_the_stream_tag() {
        for (tag, expected) in [
            ("bt709", "bt709"),
            ("bt470bg", "bt470"),
            ("smpte170m", "smpte170m"),
            ("smpte240m", "smpte240m"),
            ("fcc", "fcc"),
            ("bt2020nc", "bt2020"),
            ("bt2020c", "bt2020"),
        ] {
            assert_eq!(source_color_matrix(Some(tag)), expected, "{tag}");
        }
    }

    #[test]
    fn primaries_warning_covers_bt2020_matrices_and_wide_gamut_primaries() {
        // BT.2020 matrices (SDR; PQ/HLG are rejected earlier) imply BT.2020 primaries.
        assert!(primaries_not_converted(Some("bt2020nc"), None));
        assert!(primaries_not_converted(Some("bt2020c"), Some("bt2020")));
        for primaries in ["bt2020", "smpte428", "smpte431", "smpte432"] {
            assert!(
                primaries_not_converted(Some("bt709"), Some(primaries)),
                "{primaries}"
            );
        }

        // SD and other near-BT.709 gamuts shift colours only slightly, so common
        // DVD/SD sources must not warn.
        for primaries in [
            "bt470m",
            "bt470bg",
            "smpte170m",
            "smpte240m",
            "film",
            "jedec-p22",
            "ebu3213",
        ] {
            assert!(
                !primaries_not_converted(Some("smpte170m"), Some(primaries)),
                "{primaries}"
            );
        }

        // BT.709 or unknown primaries need no warning.
        assert!(!primaries_not_converted(Some("bt709"), Some("bt709")));
        assert!(!primaries_not_converted(None, None));
        assert!(!primaries_not_converted(Some("smpte170m"), None));
        assert!(!primaries_not_converted(Some("bt709"), Some("unknown")));
        assert!(!primaries_not_converted(None, Some("reserved")));
    }

    #[test]
    fn extract_metadata_reads_the_primaries_tag() {
        let json = br#"{
            "streams": [{
                "index": 0, "codec_type": "video", "codec_name": "hevc",
                "width": 3840, "height": 2160, "pix_fmt": "yuv420p10le",
                "r_frame_rate": "24/1", "color_space": "bt2020nc",
                "color_transfer": "bt709", "color_primaries": "bt2020"
            }],
            "chapters": [],
            "format": {"format_name": "matroska,webm"}
        }"#;
        let probe = parse_ffprobe_json(json).unwrap();
        let (info, _) = extract_metadata(&probe, Path::new("/tmp/test.mkv")).unwrap();
        assert_eq!(info.color_space.as_deref(), Some("bt2020nc"));
        assert_eq!(info.color_primaries.as_deref(), Some("bt2020"));

        let untagged = parse_ffprobe_json(SAMPLE_FFPROBE_JSON.as_bytes()).unwrap();
        let (info, _) = extract_metadata(&untagged, Path::new("/tmp/test.mkv")).unwrap();
        assert_eq!(info.color_primaries, None);
    }

    #[test]
    fn untagged_sources_are_treated_as_bt709_at_any_size() {
        assert_eq!(source_color_matrix(None), "bt709");
        assert_eq!(source_color_matrix(Some("unknown")), "bt709");
        assert_eq!(source_color_matrix(Some("reserved")), "bt709");
    }

    #[test]
    fn extract_metadata_reads_the_color_space_tag() {
        let json = br#"{
            "streams": [{
                "index": 0, "codec_type": "video", "codec_name": "hevc",
                "width": 1920, "height": 1080, "pix_fmt": "yuv420p",
                "r_frame_rate": "24/1", "color_space": "bt709"
            }],
            "chapters": [],
            "format": {"format_name": "matroska,webm"}
        }"#;
        let probe = parse_ffprobe_json(json).unwrap();
        let (info, _) = extract_metadata(&probe, Path::new("/tmp/test.mkv")).unwrap();
        assert_eq!(info.color_space.as_deref(), Some("bt709"));

        let untagged = parse_ffprobe_json(SAMPLE_FFPROBE_JSON.as_bytes()).unwrap();
        let (info, _) = extract_metadata(&untagged, Path::new("/tmp/test.mkv")).unwrap();
        assert_eq!(info.color_space, None);
    }

    pub(crate) const COLOR_QUADRANTS: [[u8; 3]; 4] =
        [[200, 30, 30], [30, 200, 30], [30, 30, 200], [128, 128, 128]];

    /// RGB24 frame split into four solid quadrants of [`COLOR_QUADRANTS`].
    pub(crate) fn quadrant_frame(width: usize, height: usize) -> Vec<u8> {
        let mut rgb = Vec::with_capacity(width * height * 3);
        for y in 0..height {
            for x in 0..width {
                let quadrant = usize::from(y >= height / 2) * 2 + usize::from(x >= width / 2);
                rgb.extend_from_slice(&COLOR_QUADRANTS[quadrant]);
            }
        }
        rgb
    }

    /// Asserts each quadrant centre of an RGB24 frame matches [`COLOR_QUADRANTS`].
    pub(crate) fn assert_quadrant_colors(rgb: &[u8], width: usize, height: usize, label: &str) {
        for (quadrant, expected) in COLOR_QUADRANTS.iter().enumerate() {
            let x = width / 4 + (quadrant % 2) * width / 2;
            let y = height / 4 + (quadrant / 2) * height / 2;
            let offset = (y * width + x) * 3;
            let actual = &rgb[offset..offset + 3];
            let close = actual
                .iter()
                .zip(expected)
                .all(|(a, e)| (i16::from(*a) - i16::from(*e)).abs() <= 2);
            assert!(
                close,
                "{label}: quadrant {quadrant} decoded {actual:?}, expected {expected:?}"
            );
        }
    }

    /// 8-bit RGB samples of a decoded `CpuRgb` frame (16-bit samples are rounded down to 8 bits).
    pub(crate) fn rgb8_samples(frame: Frame) -> Vec<u8> {
        let Frame::CpuRgb {
            data, bit_depth, ..
        } = frame
        else {
            panic!("expected a CpuRgb frame");
        };
        if bit_depth > 8 {
            data.as_chunks::<2>()
                .0
                .iter()
                .map(|sample| {
                    let value = u32::from(u16::from_le_bytes(*sample));
                    ((value * 255 + 32_767) / 65_535) as u8
                })
                .collect()
        } else {
            data
        }
    }

    /// Losslessly encodes one quadrant frame converted with `matrix` in
    /// limited or full range, optionally tagged. An untagged clip carries no
    /// colour tags at all: FFmpeg 5+ stamps the `scale` output matrix on the
    /// frame and libx264 writes it to the VUI, so `setparams` clears it.
    fn write_quadrant_clip(
        path: &Path,
        width: usize,
        height: usize,
        matrix: &str,
        full_range: bool,
        tag: Option<&str>,
    ) {
        let range = if full_range { "full" } else { "limited" };
        let mut filter =
            format!("scale=out_color_matrix={matrix}:out_range={range},format=yuv444p");
        if tag.is_none() {
            filter.push_str(
                ",setparams=colorspace=unknown:color_primaries=unknown:color_trc=unknown",
            );
        }
        let mut command = crate::runtime::command_for("ffmpeg");
        command.args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24"]);
        command.args([
            "-s",
            &format!("{width}x{height}"),
            "-r",
            "1",
            "-i",
            "pipe:0",
        ]);
        command.args(["-vf", &filter, "-c:v", "libx264", "-qp", "0"]);
        if let Some(tag) = tag {
            command.args([
                "-colorspace",
                tag,
                "-color_primaries",
                tag,
                "-color_trc",
                tag,
            ]);
        }
        if full_range {
            command.args(["-color_range", "pc"]);
        }
        let mut child = command
            .arg(path)
            .stdin(Stdio::piped())
            .spawn()
            .expect("ffmpeg should start");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&quadrant_frame(width, height))
            .unwrap();
        assert!(
            child.wait().unwrap().success(),
            "ffmpeg failed to write {}",
            path.display()
        );
    }

    #[test]
    #[ignore = "requires ffmpeg with libx264"]
    fn first_frame_decode_matches_the_streaming_decoder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.mkv");
        write_quadrant_clip(&path, 1280, 720, "bt709", false, Some("bt709"));
        let info = extract_metadata(
            &run_ffprobe_within(&path, Duration::from_secs(30)).unwrap(),
            &path,
        )
        .unwrap()
        .0;
        let streamed = VideoDecoder::new(&path, &info, None)
            .unwrap()
            .next()
            .expect("one frame")
            .unwrap();
        let first = decode_first_frame(&path, &info, Duration::from_secs(30)).unwrap();
        assert_eq!(rgb8_samples(first), rgb8_samples(streamed));
    }

    #[test]
    #[ignore = "requires ffmpeg with libx264"]
    fn decoder_recovers_source_rgb_for_tagged_and_untagged_matrices() {
        let dir = tempfile::tempdir().unwrap();
        for (name, width, height, matrix, full_range, tag) in [
            ("untagged-hd-bt709", 1280, 720, "bt709", false, None),
            ("untagged-sd-bt709", 720, 480, "bt709", false, None),
            (
                "tagged-sd-bt601",
                720,
                480,
                "smpte170m",
                false,
                Some("smpte170m"),
            ),
            ("tagged-hd-bt709", 1280, 720, "bt709", false, Some("bt709")),
            // Range is taken from the stream, not assumed limited.
            (
                "tagged-hd-bt709-full",
                1280,
                720,
                "bt709",
                true,
                Some("bt709"),
            ),
        ] {
            let path = dir.path().join(format!("{name}.mkv"));
            write_quadrant_clip(&path, width, height, matrix, full_range, tag);
            let probe = run_ffprobe(&path).unwrap();
            let (info, _) = extract_metadata(&probe, &path).unwrap();
            // The fixture must really be (un)tagged under every FFmpeg version.
            assert_eq!(info.color_space.as_deref(), tag, "{name}");
            let frame = VideoDecoder::new(&path, &info, None)
                .unwrap()
                .next()
                .expect("one frame")
                .unwrap();
            assert_quadrant_colors(&rgb8_samples(frame), width, height, name);
        }
    }

    fn test_mkv_path() -> PathBuf {
        std::env::temp_dir().join("test.mkv")
    }

    fn test_mp3_path() -> PathBuf {
        std::env::temp_dir().join("test.mp3")
    }
}
