//! StreamInput node: accepts a URL (HTTP, RTMP, etc.) as a video source.
//!
//! Similar to `VideoInputNode` but uses URL-based input instead of a file path.
//! FFmpeg handles network protocol decoding natively, so `VideoDecoder` can be
//! reused with the URL as the input path.

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;

use anyhow::{bail, Context, Result};
use tracing::debug;

use crate::node::{ExecutionContext, Node, PortDefinition};
use crate::types::{PortData, PortType};

use crate::nodes::video_input::{extract_metadata, FfprobeOutput};

/// Validate that a string looks like a plausible stream URL.
///
/// Accepts `http://`, `https://`, `rtmp://`, `rtmps://`, `rtsp://`,
/// `srt://`, `udp://`, `tcp://`, `rtp://`, and `mms://` schemes.
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

/// Run ffprobe against a URL and parse the JSON output.
fn run_ffprobe_url(url: &str) -> Result<FfprobeOutput> {
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
        .arg(url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("failed to execute ffprobe -- is FFmpeg installed?")?;

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

pub struct StreamInputNode;

impl StreamInputNode {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StreamInputNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for StreamInputNode {
    fn node_type(&self) -> &str {
        "stream_input"
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
                name: "protocol_options".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("")),
            },
        ]
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
                name: "source_url".to_string(),
                port_type: PortType::Str,
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
        let url = match inputs.get("url") {
            Some(PortData::Str(u)) => u.clone(),
            _ => bail!("missing or invalid 'url' input (expected Str)"),
        };

        validate_stream_url(&url)?;

        debug!(url = %url, "running ffprobe on stream URL");
        let probe = run_ffprobe_url(&url)?;

        // Use a synthetic path for metadata (stream URLs don't have local paths)
        let synthetic_path = Path::new(&url);
        let (_video_info, metadata) = extract_metadata(&probe, synthetic_path)?;

        debug!(
            width = _video_info.width,
            height = _video_info.height,
            fps = _video_info.fps,
            codec = %_video_info.codec_name,
            pix_fmt = %_video_info.pix_fmt,
            bit_depth = _video_info.bit_depth,
            "stream input probed"
        );

        let mut outputs = HashMap::new();
        outputs.insert("metadata".to_string(), PortData::Metadata(metadata));
        outputs.insert("source_url".to_string(), PortData::Str(url));
        Ok(outputs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_type() {
        let node = StreamInputNode::new();
        assert_eq!(node.node_type(), "stream_input");
    }

    #[test]
    fn test_input_ports() {
        let node = StreamInputNode::new();
        let ports = node.input_ports();
        assert_eq!(ports.len(), 2);
        assert_eq!(ports[0].name, "url");
        assert_eq!(ports[0].port_type, PortType::Str);
        assert!(ports[0].required);
        assert_eq!(ports[1].name, "protocol_options");
        assert_eq!(ports[1].port_type, PortType::Str);
        assert!(!ports[1].required);
    }

    #[test]
    fn test_output_ports() {
        let node = StreamInputNode::new();
        let ports = node.output_ports();
        assert_eq!(ports.len(), 2);
        assert_eq!(ports[0].name, "metadata");
        assert_eq!(ports[0].port_type, PortType::Metadata);
        assert_eq!(ports[1].name, "source_url");
        assert_eq!(ports[1].port_type, PortType::Str);
    }

    #[test]
    fn test_execute_missing_url() {
        let mut node = StreamInputNode::new();
        let ctx = ExecutionContext::default();
        let inputs = HashMap::new();
        let result = node.execute(&inputs, &ctx);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("url"), "error should mention url: {msg}");
    }

    #[test]
    fn test_validate_url_valid_http() {
        assert!(validate_stream_url("http://example.com/stream.m3u8").is_ok());
    }

    #[test]
    fn test_validate_url_valid_https() {
        assert!(validate_stream_url("https://example.com/stream.m3u8").is_ok());
    }

    #[test]
    fn test_validate_url_valid_rtmp() {
        assert!(validate_stream_url("rtmp://live.example.com/app/stream").is_ok());
    }

    #[test]
    fn test_validate_url_valid_rtsp() {
        assert!(validate_stream_url("rtsp://camera.local:554/stream").is_ok());
    }

    #[test]
    fn test_validate_url_valid_srt() {
        assert!(validate_stream_url("srt://host:9000").is_ok());
    }

    #[test]
    fn test_validate_url_valid_udp() {
        assert!(validate_stream_url("udp://239.0.0.1:1234").is_ok());
    }

    #[test]
    fn test_validate_url_empty() {
        let result = validate_stream_url("");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn test_validate_url_invalid_scheme() {
        let result = validate_stream_url("ftp://example.com/file");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unsupported"));
    }

    #[test]
    fn test_validate_url_no_scheme() {
        let result = validate_stream_url("example.com/stream");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unsupported"));
    }

    #[test]
    fn test_execute_invalid_url_scheme() {
        let mut node = StreamInputNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert(
            "url".to_string(),
            PortData::Str("ftp://invalid.example.com/stream".to_string()),
        );
        let result = node.execute(&inputs, &ctx);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("unsupported"), "error: {msg}");
    }

    #[test]
    fn test_execute_empty_url() {
        let mut node = StreamInputNode::new();
        let ctx = ExecutionContext::default();
        let mut inputs = HashMap::new();
        inputs.insert("url".to_string(), PortData::Str(String::new()));
        let result = node.execute(&inputs, &ctx);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("empty"), "error: {msg}");
    }

    #[test]
    fn test_default_trait() {
        let node = StreamInputNode::default();
        assert_eq!(node.node_type(), "stream_input");
    }
}
