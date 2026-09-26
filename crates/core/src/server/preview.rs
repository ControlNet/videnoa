//! Single-frame previews use real processors, without running workflow I/O actions.
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::Stdio;

use anyhow::{anyhow, bail, Context, Result};

use crate::compile::{resolve_inputs, validate_linear_topology};
use crate::graph::PipelineGraph;
use crate::node::{ExecutionContext, FrameProcessor, Node};
use crate::nodes::compile_context::VideoCompileContext;
use crate::nodes::rescale::RescaleNode;
use crate::nodes::resize::ResizeNode;
use crate::nodes::video_input::{extract_metadata, run_ffprobe, VideoDecoder};
use crate::registry::NodeRegistry;
use crate::types::{Frame, PortData, PortType};

pub(super) fn validate(graph: &PipelineGraph, registry: &NodeRegistry) -> Result<()> {
    graph.validate(registry)?;
    let order = graph.execution_order()?;
    validate_linear_topology(graph, registry, &order)?;
    let mut sources = 0;
    let mut sinks = 0;
    for index in order {
        let node = graph.node(index);
        let incoming = graph
            .connections_to(index)
            .iter()
            .filter(|(_, edge)| edge.port_type == PortType::VideoFrames)
            .count();
        let outgoing = graph
            .connections_from(index)
            .iter()
            .filter(|(_, edge)| edge.port_type == PortType::VideoFrames)
            .count();
        let expected = match node.node_type.as_str() {
            "VideoInput" => {
                sources += 1;
                (0, 1)
            }
            "VideoOutput" => {
                sinks += 1;
                (1, 0)
            }
            "SuperResolution" | "Resize" | "Rescale" => (1, 1),
            "Constant" | "WorkflowInput" | "WorkflowOutput" | "PathDivider" | "PathJoiner"
            | "StringTemplate" | "StringReplace" | "TypeConversion" => (0, 0),
            "FrameInterpolation" | "SceneDetect" => bail!(
                "{} requires multiple frames and cannot be previewed as a single image",
                node.node_type
            ),
            other => bail!("single-frame preview does not support node '{other}'"),
        };
        if (incoming, outgoing) != expected {
            bail!(
                "preview node '{}' has invalid VideoFrames connections",
                node.id
            );
        }
    }
    if sources != 1 || sinks != 1 {
        bail!("preview requires one VideoInput and one VideoOutput in a linear pipeline");
    }
    Ok(())
}

pub(super) fn process(
    mut graph: PipelineGraph,
    registry: &NodeRegistry,
    input: &Path,
    output: &Path,
    trt_cache: std::path::PathBuf,
) -> Result<()> {
    graph.inject_workflow_input_params(&HashMap::from([
        ("input".to_owned(), serde_json::json!(input)),
        ("output".to_owned(), serde_json::json!(output)),
    ]));
    let ctx = ExecutionContext::default();
    let compile_ctx = VideoCompileContext::new(trt_cache);
    let mut outputs = HashMap::new();
    let mut frame = None;
    for index in graph.execution_order()? {
        let instance = graph.node(index);
        let values = match instance.node_type.as_str() {
            "VideoInput" => {
                // Use the selected extracted image, never reopen the workflow's input.
                let (info, metadata) = extract_metadata(&run_ffprobe(input)?, input)?;
                let mut decoder = VideoDecoder::new(input, &info, Some("none"))?;
                frame = Some(
                    decoder
                        .next()
                        .ok_or_else(|| anyhow!("preview image has no frame"))??,
                );
                HashMap::from([
                    (
                        "source_path".to_owned(),
                        PortData::Path(input.to_path_buf()),
                    ),
                    ("metadata".to_owned(), PortData::Metadata(metadata)),
                ])
            }
            "VideoOutput" => {
                // No video encoder, output path, codec or remote side effect is executed.
                HashMap::from([(
                    "output_path".to_owned(),
                    PortData::Path(output.to_path_buf()),
                )])
            }
            "SuperResolution" | "Resize" | "Rescale" => {
                let inputs = resolve_inputs(&graph, registry, index, &outputs)?;
                let source = frame
                    .take()
                    .ok_or_else(|| anyhow!("missing preview source frame"))?;
                let Frame::CpuRgb { width, height, .. } = &source else {
                    bail!("preview processor requires RGB input");
                };
                let mut processor: Box<dyn FrameProcessor> = match instance.node_type.as_str() {
                    "SuperResolution" => {
                        Box::new(compile_ctx.create_preview_superres(&inputs, *width, *height)?)
                    }
                    "Resize" => {
                        let mut node = ResizeNode::new();
                        node.execute(&inputs, &ctx)?;
                        Box::new(node)
                    }
                    _ => {
                        let mut node = RescaleNode::new();
                        node.execute(&inputs, &ctx)?;
                        Box::new(node)
                    }
                };
                frame =
                    Some(processor.process_frame(source, &ctx).with_context(|| {
                        format!("preview processing failed for '{}'", instance.id)
                    })?);
                HashMap::new()
            }
            _ => {
                let inputs = resolve_inputs(&graph, registry, index, &outputs)?;
                registry
                    .create(&instance.node_type, instance.params.clone())?
                    .execute(&inputs, &ctx)?
            }
        };
        outputs.insert(instance.id.clone(), values);
    }
    write_png(
        frame.ok_or_else(|| anyhow!("preview produced no frame"))?,
        output,
    )
}

fn write_png(frame: Frame, output: &Path) -> Result<()> {
    let Frame::CpuRgb {
        data,
        width,
        height,
        bit_depth,
    } = frame
    else {
        bail!("preview output must be an RGB image");
    };
    let format = if bit_depth > 8 { "rgb48le" } else { "rgb24" };
    let mut child = crate::runtime::command_for("ffmpeg")
        .args([
            "-v",
            "error",
            "-nostdin",
            "-n",
            "-f",
            "rawvideo",
            "-pix_fmt",
            format,
            "-s",
            &format!("{width}x{height}"),
            "-i",
            "pipe:0",
            "-frames:v",
            "1",
            "-threads",
            "1",
            "-c:v",
            "png",
            "-f",
            "image2",
            "-update",
            "1",
        ])
        .arg(output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to start preview PNG encoder")?;
    let written = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("missing PNG encoder input"))?
        .write_all(&data);
    if written.is_err() {
        let _ = child.kill();
    }
    let result = child.wait_with_output()?;
    written.context("failed to write preview pixels")?;
    if !result.status.success() {
        bail!(
            "preview PNG encoding failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    Ok(())
}
