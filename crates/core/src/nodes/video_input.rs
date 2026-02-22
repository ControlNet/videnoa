use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Stdio};
use std::thread;

use anyhow::{anyhow, bail, Context, Result};
use tracing::{debug, warn};

use crate::node::{ExecutionContext, Node, PortDefinition};
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

fn parse_frame_rate(s: &str) -> Option<f64> {
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

pub fn run_ffprobe(path: &Path) -> Result<FfprobeOutput> {
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
    buf: Vec<u8>,
    done: bool,
    #[allow(dead_code)]
    hwaccel: Option<String>,
}

fn build_decoder_args(
    path: &Path,
    pix_fmt: &str,
    stream_index: usize,
    hwaccel: Option<&str>,
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

        let decode_args = build_decoder_args(path, pix_fmt, info.stream_index, hwaccel);

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
            buf: vec![0u8; frame_size],
            done: false,
            hwaccel: hwaccel.map(|s| s.to_string()),
        })
    }

    fn read_frame(&mut self) -> Result<Option<Frame>> {
        let stdout = self
            .child
            .stdout
            .as_mut()
            .ok_or_else(|| anyhow!("ffmpeg stdout not available"))?;

        let mut total_read = 0;
        while total_read < self.frame_size {
            match stdout.read(&mut self.buf[total_read..self.frame_size]) {
                Ok(0) => {
                    if total_read == 0 {
                        return Ok(None);
                    }
                    warn!(
                        "partial frame at EOF ({total_read}/{} bytes), discarding",
                        self.frame_size
                    );
                    return Ok(None);
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
            data: self.buf[..self.frame_size].to_vec(),
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
                None
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
mod tests {
    use super::*;
    use std::path::PathBuf;

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

        assert!(metadata.audio_streams.len() >= 1);
        assert!(metadata.subtitle_streams.len() >= 1);
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
                assert!(m.audio_streams.len() >= 1);
                assert_eq!(m.source_path, path);
            }
            _ => panic!("expected Metadata output"),
        }
    }

    #[test]
    fn test_decoder_args_no_hwaccel() {
        let path = test_mkv_path();
        let args = build_decoder_args(path.as_path(), "rgb24", 4, None);

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
        let args = build_decoder_args(path.as_path(), "rgb48le", 2, Some("cuda"));

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
        let args = build_decoder_args(path.as_path(), "rgb24", 0, Some("none"));

        assert!(!args.contains(&"-hwaccel".to_string()));
    }

    #[test]
    fn test_decoder_args_unknown_hwaccel_ignored() {
        let path = test_mkv_path();
        let args = build_decoder_args(path.as_path(), "rgb24", 7, Some("vulkan"));

        assert!(!args.contains(&"-hwaccel".to_string()));
        let map_idx = args.iter().position(|a| a == "-map").unwrap();
        assert_eq!(args[map_idx + 1], "0:7");
    }

    fn test_mkv_path() -> PathBuf {
        std::env::temp_dir().join("test.mkv")
    }

    fn test_mp3_path() -> PathBuf {
        std::env::temp_dir().join("test.mp3")
    }
}
