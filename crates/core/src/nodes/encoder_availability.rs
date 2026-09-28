//! Rejects workflows whose output encoder is missing from the FFmpeg that jobs
//! run with, before the job is queued (issue #6: an LGPL FFmpeg bundle had no
//! libx264/libx265, so every default job failed at encoder start).

use std::collections::BTreeSet;
use std::process::Stdio;
use std::sync::OnceLock;

use anyhow::{bail, Result};

use crate::graph::PipelineGraph;

/// Output nodes and the encoder each uses when `codec` is not set.
const OUTPUT_DEFAULT_CODECS: [(&str, &str); 2] =
    [("VideoOutput", "libx265"), ("StreamOutput", "libx264")];

/// Encoders the editor offers, in the order they are suggested.
const OFFERED_ENCODERS: [&str; 4] = ["libx265", "libx264", "hevc_nvenc", "h264_nvenc"];

#[derive(Debug, PartialEq, Eq)]
struct RequestedEncoder {
    node_id: String,
    node_type: String,
    codec: String,
}

/// Fails when an output node requests an encoder that FFmpeg does not list.
/// The check is skipped when FFmpeg cannot be probed; the job then reports
/// its own start-up error.
pub fn validate_workflow_encoders(graph: &PipelineGraph) -> Result<()> {
    let requested = requested_encoders(graph)?;
    if requested.is_empty() {
        return Ok(());
    }
    match available_encoders() {
        Some(available) => check_requested_encoders(&requested, available),
        None => Ok(()),
    }
}

/// Encoders set as literal params or left at their default. A `codec` fed by
/// a connection is only known at run time and is not checked here.
fn requested_encoders(graph: &PipelineGraph) -> Result<Vec<RequestedEncoder>> {
    let mut requested = Vec::new();
    for idx in graph.execution_order()? {
        let node = graph.node(idx);
        let Some((_, default_codec)) = OUTPUT_DEFAULT_CODECS
            .iter()
            .find(|(node_type, _)| *node_type == node.node_type)
        else {
            continue;
        };
        if graph
            .connections_to(idx)
            .iter()
            .any(|(_, connection)| connection.target_port == "codec")
        {
            continue;
        }
        let codec = match node.params.get("codec") {
            None => *default_codec,
            Some(serde_json::Value::String(codec)) => codec.as_str(),
            Some(_) => continue,
        };
        requested.push(RequestedEncoder {
            node_id: node.id.clone(),
            node_type: node.node_type.clone(),
            codec: codec.to_string(),
        });
    }
    Ok(requested)
}

fn check_requested_encoders(
    requested: &[RequestedEncoder],
    available: &BTreeSet<String>,
) -> Result<()> {
    let Some(missing) = requested
        .iter()
        .find(|request| !available.contains(&request.codec))
    else {
        return Ok(());
    };
    let alternatives: Vec<&str> = OFFERED_ENCODERS
        .into_iter()
        .filter(|codec| available.contains(*codec))
        .collect();
    let choose = if alternatives.is_empty() {
        String::new()
    } else {
        format!(
            "choose an available encoder ({}), or ",
            alternatives.join(", ")
        )
    };
    bail!(
        "{} '{}' uses encoder '{}', which the bundled FFmpeg does not provide; \
         {choose}replace FFmpeg with a build that includes it (GPL builds provide \
         libx264 and libx265). NVENC encoders need an NVIDIA GPU",
        missing.node_type,
        missing.node_id,
        missing.codec,
    )
}

/// Video encoder names from `ffmpeg -encoders`.
fn parse_ffmpeg_encoders(listing: &str) -> BTreeSet<String> {
    listing
        .lines()
        .skip_while(|line| !line.trim_start().starts_with("---"))
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let flags = fields.next()?;
            let name = fields.next()?;
            flags.starts_with('V').then(|| name.to_string())
        })
        .collect()
}

/// Probed once per process; the FFmpeg binary does not change while it runs.
fn available_encoders() -> Option<&'static BTreeSet<String>> {
    static ENCODERS: OnceLock<Option<BTreeSet<String>>> = OnceLock::new();
    ENCODERS
        .get_or_init(|| {
            let output = crate::runtime::command_for("ffmpeg")
                .args(["-hide_banner", "-encoders"])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .ok()
                .filter(|output| output.status.success())?;
            let encoders = parse_ffmpeg_encoders(&String::from_utf8_lossy(&output.stdout));
            // An empty list means an unexpected listing format, not a build
            // without encoders; don't block jobs on it.
            (!encoders.is_empty()).then_some(encoders)
        })
        .as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const LISTING: &str = "Encoders:
 V..... = Video
 A..... = Audio
 ------
 V....D libsvtav1            SVT-AV1(Scalable Video Technology for AV1) encoder (codec av1)
 V....D h264_nvenc           NVIDIA NVENC H.264 encoder (codec h264)
 V....D hevc_nvenc           NVIDIA NVENC hevc encoder (codec hevc)
 A....D aac                  AAC (Advanced Audio Coding)
";

    fn graph(nodes: serde_json::Value, connections: serde_json::Value) -> PipelineGraph {
        serde_json::from_value(json!({"nodes": nodes, "connections": connections}))
            .expect("test workflow should parse")
    }

    fn encoders(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn parses_video_encoders_from_listing() {
        assert_eq!(
            parse_ffmpeg_encoders(LISTING),
            encoders(&["h264_nvenc", "hevc_nvenc", "libsvtav1"])
        );
    }

    #[test]
    fn output_nodes_request_their_default_or_configured_codec() {
        let graph = graph(
            json!([
                {"id": "file", "node_type": "VideoOutput", "params": {}},
                {"id": "stream", "node_type": "StreamOutput", "params": {}},
                {"id": "nvenc", "node_type": "VideoOutput", "params": {"codec": "hevc_nvenc"}},
            ]),
            json!([]),
        );

        let mut codecs: Vec<(String, String)> = requested_encoders(&graph)
            .unwrap()
            .into_iter()
            .map(|request| (request.node_id, request.codec))
            .collect();
        codecs.sort();

        assert_eq!(
            codecs,
            [
                ("file".to_string(), "libx265".to_string()),
                ("nvenc".to_string(), "hevc_nvenc".to_string()),
                ("stream".to_string(), "libx264".to_string()),
            ]
        );
    }

    #[test]
    fn connected_codec_is_left_to_run_time() {
        let graph = graph(
            json!([
                {"id": "input", "node_type": "WorkflowInput", "params": {}},
                {"id": "output", "node_type": "VideoOutput", "params": {}},
            ]),
            json!([{"from_node": "input", "from_port": "codec", "to_node": "output", "to_port": "codec", "port_type": "Str"}]),
        );

        assert!(requested_encoders(&graph).unwrap().is_empty());
    }

    #[test]
    fn missing_encoder_names_the_node_and_available_alternatives() {
        let requested = [RequestedEncoder {
            node_id: "output".to_string(),
            node_type: "VideoOutput".to_string(),
            codec: "libx265".to_string(),
        }];

        let message = check_requested_encoders(&requested, &parse_ffmpeg_encoders(LISTING))
            .unwrap_err()
            .to_string();

        assert!(
            message.contains("VideoOutput 'output' uses encoder 'libx265'"),
            "{message}"
        );
        assert!(
            message.contains("choose an available encoder (hevc_nvenc, h264_nvenc)"),
            "{message}"
        );
    }

    #[test]
    fn available_encoders_pass() {
        let requested = [RequestedEncoder {
            node_id: "output".to_string(),
            node_type: "VideoOutput".to_string(),
            codec: "hevc_nvenc".to_string(),
        }];

        assert!(check_requested_encoders(&requested, &parse_ffmpeg_encoders(LISTING)).is_ok());
    }
}
