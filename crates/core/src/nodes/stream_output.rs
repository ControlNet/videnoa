use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Stdio};
use std::thread::{self, JoinHandle};

use anyhow::{bail, Context, Result};
use tracing::{debug, info};

use crate::node::{ExecutionContext, Node, PortDefinition};
use crate::types::{PortData, PortType};

fn validate_stream_url(url: &str) -> Result<()> {
    if url.is_empty() {
        bail!("stream URL must not be empty");
    }

    let schemes = [
        "http://", "https://", "rtmp://", "rtmps://", "rtsp://", "srt://", "udp://", "tcp://",
        "rtp://", "mms://",
    ];

    let lower = url.to_ascii_lowercase();
    if !schemes.iter().any(|s| lower.starts_with(s)) {
        bail!(
            "unsupported stream URL scheme: '{}'. Expected one of: {}",
            url,
            schemes.join(", ")
        );
    }

    Ok(())
}

/// Detect output format from URL scheme. Returns `None` if auto-detection fails.
fn detect_format_from_url(url: &str) -> Option<&'static str> {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("rtmp://") || lower.starts_with("rtmps://") {
        Some("flv")
    } else if lower.starts_with("http://") || lower.starts_with("https://") {
        Some("mpegts")
    } else if lower.starts_with("srt://")
        || lower.starts_with("udp://")
        || lower.starts_with("rtp://")
    {
        Some("mpegts")
    } else {
        None
    }
}

#[derive(Debug, Clone)]
pub struct StreamEncoderConfig {
    pub url: String,
    pub codec: String,
    pub bitrate: String,
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub fps: String,
    pub bit_depth: u8,
}

impl StreamEncoderConfig {
    pub fn build_ffmpeg_args(&self) -> Vec<String> {
        let input_pix_fmt = if self.bit_depth > 8 {
            "rgb48le"
        } else {
            "rgb24"
        };

        let size = format!("{}x{}", self.width, self.height);

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
            "-c:v".into(),
            self.codec.clone(),
            "-b:v".into(),
            self.bitrate.clone(),
            "-f".into(),
            self.format.clone(),
        ];

        // RTMP/FLV needs this flag to avoid seeking issues on live streams
        if self.format == "flv" {
            args.push("-flvflags".into());
            args.push("no_duration_filesize".into());
        }

        args.push(self.url.clone());

        args
    }

    pub fn frame_size(&self) -> usize {
        let bytes_per_pixel: usize = if self.bit_depth > 8 { 6 } else { 3 };
        self.width as usize * self.height as usize * bytes_per_pixel
    }
}

pub struct StreamEncoder {
    child: Child,
    stdin: Option<ChildStdin>,
    stderr_thread: Option<JoinHandle<()>>,
    frame_size: usize,
}

impl StreamEncoder {
    pub fn new(config: &StreamEncoderConfig) -> Result<Self> {
        let args = config.build_ffmpeg_args();
        let frame_size = config.frame_size();

        debug!(
            cmd = %format!("ffmpeg {}", args.join(" ")),
            "launching FFmpeg stream encoder"
        );

        let mut child = crate::runtime::command_for("ffmpeg")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to launch ffmpeg -- is it installed?")?;

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
                        debug!(target: "ffmpeg_stream_stderr", "{}", line);
                    }
                    Err(e) => {
                        debug!(target: "ffmpeg_stream_stderr", "read error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
        });

        info!(
            url = %config.url,
            codec = %config.codec,
            bitrate = %config.bitrate,
            format = %config.format,
            "FFmpeg stream encoder started"
        );

        Ok(Self {
            child,
            stdin: Some(stdin),
            stderr_thread: Some(stderr_thread),
            frame_size,
        })
    }

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

    pub fn finish(mut self) -> Result<()> {
        drop(self.stdin.take());

        let status = self.child.wait().context("failed to wait for ffmpeg")?;

        if let Some(handle) = self.stderr_thread.take() {
            let _ = handle.join();
        }

        if !status.success() {
            bail!("ffmpeg stream encoder exited with status {}", status);
        }

        info!("FFmpeg stream encoder finished successfully");
        Ok(())
    }
}

impl Drop for StreamEncoder {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(handle) = self.stderr_thread.take() {
            let _ = handle.join();
        }
    }
}

pub struct StreamOutputNode;

impl StreamOutputNode {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StreamOutputNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for StreamOutputNode {
    fn node_type(&self) -> &str {
        "stream_output"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![
            PortDefinition {
                name: "url".to_string(),
                port_type: PortType::Str,
                required: true,
                default_value: None,
            },
            PortDefinition {
                name: "codec".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("libx264")),
            },
            PortDefinition {
                name: "bitrate".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("5M")),
            },
            PortDefinition {
                name: "format".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("flv")),
            },
            PortDefinition {
                name: "source_url".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("")),
            },
        ]
    }

    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![PortDefinition {
            name: "output_url".to_string(),
            port_type: PortType::Str,
            required: true,
            default_value: None,
        }]
    }

    fn execute(
        &mut self,
        inputs: &HashMap<String, PortData>,
        _ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        let url = match inputs.get("url") {
            Some(PortData::Str(u)) => u.clone(),
            _ => bail!("missing or invalid 'url' input (expected Str)"),
        };

        validate_stream_url(&url)?;

        let codec = match inputs.get("codec") {
            Some(PortData::Str(s)) => s.clone(),
            _ => "libx264".to_string(),
        };

        let bitrate = match inputs.get("bitrate") {
            Some(PortData::Str(s)) => s.clone(),
            _ => "5M".to_string(),
        };

        let format = match inputs.get("format") {
            Some(PortData::Str(s)) if !s.is_empty() => s.clone(),
            _ => detect_format_from_url(&url).unwrap_or("flv").to_string(),
        };

        debug!(
            url = %url,
            codec = %codec,
            bitrate = %bitrate,
            format = %format,
            "stream output config validated"
        );

        let mut outputs = HashMap::new();
        outputs.insert("output_url".to_string(), PortData::Str(url));
        Ok(outputs)
    }
}

pub fn stream_encoder_config_from_inputs(
    inputs: &HashMap<String, PortData>,
    width: u32,
    height: u32,
    fps: &str,
    bit_depth: u8,
) -> Result<StreamEncoderConfig> {
    let url = match inputs.get("url") {
        Some(PortData::Str(u)) => u.clone(),
        _ => bail!("missing or invalid 'url' input"),
    };

    let codec = match inputs.get("codec") {
        Some(PortData::Str(s)) => s.clone(),
        _ => "libx264".to_string(),
    };

    let bitrate = match inputs.get("bitrate") {
        Some(PortData::Str(s)) => s.clone(),
        _ => "5M".to_string(),
    };

    let format = match inputs.get("format") {
        Some(PortData::Str(s)) if !s.is_empty() => s.clone(),
        _ => detect_format_from_url(&url).unwrap_or("flv").to_string(),
    };

    Ok(StreamEncoderConfig {
        url,
        codec,
        bitrate,
        format,
        width,
        height,
        fps: fps.to_string(),
        bit_depth,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_type() {
        let node = StreamOutputNode::new();
        assert_eq!(node.node_type(), "stream_output");
    }

    #[test]
    fn test_input_ports() {
        let node = StreamOutputNode::new();
        let ports = node.input_ports();
        assert_eq!(ports.len(), 5);

        let names: Vec<&str> = ports.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"url"));
        assert!(names.contains(&"codec"));
        assert!(names.contains(&"bitrate"));
        assert!(names.contains(&"format"));
        assert!(names.contains(&"source_url"));

        let required: Vec<&str> = ports
            .iter()
            .filter(|p| p.required)
            .map(|p| p.name.as_str())
            .collect();
        assert!(required.contains(&"url"));
        assert!(!required.contains(&"codec"));
        assert!(!required.contains(&"bitrate"));
        assert!(!required.contains(&"format"));
    }

    #[test]
    fn test_output_ports() {
        let node = StreamOutputNode::new();
        let ports = node.output_ports();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].name, "output_url");
        assert_eq!(ports[0].port_type, PortType::Str);
    }

    #[test]
    fn test_execute_missing_url() {
        let mut node = StreamOutputNode::new();
        let ctx = ExecutionContext::default();
        let inputs = HashMap::new();
        let result = node.execute(&inputs, &ctx);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("url"), "error should mention url: {msg}");
    }

    #[test]
    fn test_execute_invalid_url() {
        let mut node = StreamOutputNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert(
            "url".to_string(),
            PortData::Str("ftp://invalid.example.com".to_string()),
        );
        let result = node.execute(&inputs, &ctx);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("unsupported"), "error: {msg}");
    }

    #[test]
    fn test_execute_empty_url() {
        let mut node = StreamOutputNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("url".to_string(), PortData::Str(String::new()));
        let result = node.execute(&inputs, &ctx);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("empty"), "error: {msg}");
    }

    #[test]
    fn test_detect_format_rtmp() {
        assert_eq!(
            detect_format_from_url("rtmp://live.example.com/app/key"),
            Some("flv")
        );
        assert_eq!(
            detect_format_from_url("rtmps://live.example.com/app/key"),
            Some("flv")
        );
    }

    #[test]
    fn test_detect_format_http() {
        assert_eq!(
            detect_format_from_url("http://example.com/stream"),
            Some("mpegts")
        );
        assert_eq!(
            detect_format_from_url("https://example.com/stream"),
            Some("mpegts")
        );
    }

    #[test]
    fn test_detect_format_srt_udp() {
        assert_eq!(detect_format_from_url("srt://host:9000"), Some("mpegts"));
        assert_eq!(
            detect_format_from_url("udp://239.0.0.1:1234"),
            Some("mpegts")
        );
        assert_eq!(detect_format_from_url("rtp://host:5004"), Some("mpegts"));
    }

    #[test]
    fn test_detect_format_unknown() {
        assert_eq!(detect_format_from_url("tcp://host:1234"), None);
        assert_eq!(detect_format_from_url("mms://host/stream"), None);
    }

    #[test]
    fn test_encoder_config_frame_size_8bit() {
        let config = StreamEncoderConfig {
            url: "rtmp://live.example.com/app/key".to_string(),
            codec: "libx264".to_string(),
            bitrate: "5M".to_string(),
            format: "flv".to_string(),
            width: 1920,
            height: 1080,
            fps: "30/1".to_string(),
            bit_depth: 8,
        };
        assert_eq!(config.frame_size(), 1920 * 1080 * 3);
    }

    #[test]
    fn test_encoder_config_frame_size_10bit() {
        let config = StreamEncoderConfig {
            url: "rtmp://live.example.com/app/key".to_string(),
            codec: "libx264".to_string(),
            bitrate: "5M".to_string(),
            format: "flv".to_string(),
            width: 1920,
            height: 1080,
            fps: "30/1".to_string(),
            bit_depth: 10,
        };
        assert_eq!(config.frame_size(), 1920 * 1080 * 6);
    }

    #[test]
    fn test_ffmpeg_args_basic_structure() {
        let config = StreamEncoderConfig {
            url: "rtmp://live.example.com/app/key".to_string(),
            codec: "libx264".to_string(),
            bitrate: "5M".to_string(),
            format: "flv".to_string(),
            width: 1920,
            height: 1080,
            fps: "30/1".to_string(),
            bit_depth: 8,
        };
        let args = config.build_ffmpeg_args();

        assert_eq!(args[0], "-nostdin");
        assert_eq!(args[1], "-y");
        assert!(args.contains(&"rawvideo".to_string()));
        assert!(args.contains(&"pipe:0".to_string()));
        assert!(args.contains(&"rgb24".to_string()));
        assert!(args.contains(&"1920x1080".to_string()));
        assert!(args.contains(&"30/1".to_string()));
        assert!(args.contains(&"libx264".to_string()));
        assert!(args.contains(&"5M".to_string()));
        assert!(args.contains(&"flv".to_string()));
        assert!(args.contains(&"-flvflags".to_string()));
        assert!(args.contains(&"no_duration_filesize".to_string()));
        assert_eq!(args.last().unwrap(), "rtmp://live.example.com/app/key");
    }

    #[test]
    fn test_ffmpeg_args_no_flvflags_for_mpegts() {
        let config = StreamEncoderConfig {
            url: "udp://239.0.0.1:1234".to_string(),
            codec: "libx264".to_string(),
            bitrate: "5M".to_string(),
            format: "mpegts".to_string(),
            width: 1920,
            height: 1080,
            fps: "30/1".to_string(),
            bit_depth: 8,
        };
        let args = config.build_ffmpeg_args();

        assert!(!args.contains(&"-flvflags".to_string()));
        assert!(args.contains(&"mpegts".to_string()));
    }

    #[test]
    fn test_ffmpeg_args_10bit_input() {
        let config = StreamEncoderConfig {
            url: "rtmp://live.example.com/app/key".to_string(),
            codec: "libx264".to_string(),
            bitrate: "5M".to_string(),
            format: "flv".to_string(),
            width: 1920,
            height: 1080,
            fps: "30/1".to_string(),
            bit_depth: 10,
        };
        let args = config.build_ffmpeg_args();
        assert!(args.contains(&"rgb48le".to_string()));
    }

    #[test]
    fn test_stream_encoder_config_from_inputs_defaults() {
        let mut inputs = HashMap::new();
        inputs.insert(
            "url".to_string(),
            PortData::Str("rtmp://live.example.com/app/key".to_string()),
        );

        let config = stream_encoder_config_from_inputs(&inputs, 1920, 1080, "30/1", 8).unwrap();
        assert_eq!(config.codec, "libx264");
        assert_eq!(config.bitrate, "5M");
        assert_eq!(config.format, "flv");
        assert_eq!(config.width, 1920);
        assert_eq!(config.height, 1080);
        assert_eq!(config.fps, "30/1");
        assert_eq!(config.bit_depth, 8);
    }

    #[test]
    fn test_stream_encoder_config_from_inputs_custom() {
        let mut inputs = HashMap::new();
        inputs.insert(
            "url".to_string(),
            PortData::Str("srt://host:9000".to_string()),
        );
        inputs.insert("codec".to_string(), PortData::Str("libx265".to_string()));
        inputs.insert("bitrate".to_string(), PortData::Str("10M".to_string()));
        inputs.insert("format".to_string(), PortData::Str("mpegts".to_string()));

        let config =
            stream_encoder_config_from_inputs(&inputs, 3840, 2160, "24000/1001", 10).unwrap();
        assert_eq!(config.codec, "libx265");
        assert_eq!(config.bitrate, "10M");
        assert_eq!(config.format, "mpegts");
        assert_eq!(config.width, 3840);
        assert_eq!(config.height, 2160);
        assert_eq!(config.bit_depth, 10);
    }

    #[test]
    fn test_default_trait() {
        let node = StreamOutputNode::default();
        assert_eq!(node.node_type(), "stream_output");
    }
}
