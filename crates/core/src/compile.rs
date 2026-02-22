use std::collections::HashMap;
use std::fmt;

use anyhow::{anyhow, bail, Context, Result};
use petgraph::stable_graph::NodeIndex;

use crate::debug_event::{build_print_debug_value_event, NodeDebugEventCallback};
use crate::executor::{clone_port_data, port_data_from_json};
use crate::graph::PipelineGraph;
use crate::node::{ExecutionContext, FrameProcessor, Node};
use crate::registry::NodeRegistry;
use crate::streaming_executor::{FrameInterpolator, FrameSink, PipelineStage};
use crate::types::{Frame, PortData, PortType};

/// Compiled pipeline ready for `StreamingExecutor::execute_pipeline_stages()`.
pub struct CompiledPipeline {
    pub decoder: Box<dyn Iterator<Item = Result<Frame>> + Send>,
    pub stages: Vec<PipelineStage>,
    pub encoder: Box<dyn FrameSink>,
    /// Total number of **input** frames (from the decoder / source probe).
    pub total_frames: Option<u64>,
    /// Total number of **output** frames after interpolation expansion.
    /// Equals `total_frames` when no interpolator is present.
    pub total_output_frames: Option<u64>,
}

impl fmt::Debug for CompiledPipeline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompiledPipeline")
            .field("stages", &self.stages.len())
            .field("total_frames", &self.total_frames)
            .finish_non_exhaustive()
    }
}

/// Trait that the caller implements to bridge between `Box<dyn Node>` and the
/// concrete decoder / encoder / processor / interpolator types.
///
/// This keeps the `core` crate independent of the `nodes` crate: compile_graph
/// works with trait objects, and the caller (in `nodes` or `cli`) provides the
/// concrete downcasting or wrapping logic.
pub trait CompileContext {
    /// Turn a source node + its execute() outputs into a frame iterator and
    /// optional total frame count.
    fn create_decoder(
        &self,
        node: &mut dyn Node,
        outputs: &HashMap<String, PortData>,
    ) -> Result<(Box<dyn Iterator<Item = Result<Frame>> + Send>, Option<u64>)>;

    /// Turn a sink node + its execute() outputs into a FrameSink.
    fn create_encoder(
        &self,
        node: &mut dyn Node,
        outputs: &HashMap<String, PortData>,
    ) -> Result<Box<dyn FrameSink>>;

    /// Wrap a processing node into a `FrameProcessor`.
    fn create_processor(
        &self,
        node: Box<dyn Node>,
        inputs: &HashMap<String, PortData>,
    ) -> Result<Box<dyn FrameProcessor>>;

    /// Wrap an interpolation node into a `FrameInterpolator`.
    fn create_interpolator(
        &self,
        node: Box<dyn Node>,
        inputs: &HashMap<String, PortData>,
    ) -> Result<Box<dyn FrameInterpolator>>;

    /// Whether the given node type should be treated as an interpolator rather
    /// than a regular processor.
    fn is_interpolator_type(&self, node_type: &str) -> bool;

    fn total_output_frames(&self) -> Option<u64> {
        None
    }

    /// Create one or more streaming stages for a processing node.
    ///
    /// The default implementation preserves the original one-node -> one-stage
    /// behaviour by delegating to `create_processor()` / `create_interpolator()`.
    /// Context implementations can override this to expand a single node into
    /// multiple micro-stages.
    fn create_stages(
        &self,
        node: Box<dyn Node>,
        inputs: &HashMap<String, PortData>,
        is_interpolator: bool,
    ) -> Result<Vec<PipelineStage>> {
        if is_interpolator {
            Ok(vec![PipelineStage::Interpolator(
                self.create_interpolator(node, inputs)?,
            )])
        } else {
            Ok(vec![PipelineStage::Processor(
                self.create_processor(node, inputs)?,
            )])
        }
    }
}

/// Compile a `PipelineGraph` into the inputs needed by
/// `StreamingExecutor::execute_pipeline_stages()`.
///
/// The function walks the graph in topological order, validates that it
/// represents a linear VideoFrames pipeline (no fan-out / fan-in), resolves
/// parameter inputs for every node, and categorises nodes into source,
/// processing stages, and sink.
pub fn compile_graph(
    graph: &PipelineGraph,
    registry: &NodeRegistry,
    ctx: &dyn CompileContext,
) -> Result<CompiledPipeline> {
    compile_graph_with_debug_hook(graph, registry, ctx, None)
}

pub fn compile_graph_with_debug_hook(
    graph: &PipelineGraph,
    registry: &NodeRegistry,
    ctx: &dyn CompileContext,
    mut node_debug_callback: Option<&mut NodeDebugEventCallback<'_>>,
) -> Result<CompiledPipeline> {
    let execution_order = graph.execution_order()?;

    if !has_video_frames_ports(graph, registry, &execution_order)? {
        bail!("compile_graph only handles VideoFrames pipelines");
    }

    validate_linear_topology(graph, registry, &execution_order)?;

    let mut source_idx: Option<NodeIndex> = None;
    let mut sink_idx: Option<NodeIndex> = None;
    let mut processing_order: Vec<NodeIndex> = Vec::new();

    for &node_idx in &execution_order {
        let incoming_vf = count_video_frames_edges(graph, node_idx, Direction::Incoming);
        let outgoing_vf = count_video_frames_edges(graph, node_idx, Direction::Outgoing);

        if incoming_vf == 0 && outgoing_vf > 0 {
            if source_idx.is_some() {
                bail!(
                    "multiple source nodes detected — compile_graph only supports linear pipelines"
                );
            }
            source_idx = Some(node_idx);
        } else if incoming_vf > 0 && outgoing_vf == 0 {
            if sink_idx.is_some() {
                bail!(
                    "multiple sink nodes detected — compile_graph only supports linear pipelines"
                );
            }
            sink_idx = Some(node_idx);
        } else if incoming_vf > 0 && outgoing_vf > 0 {
            processing_order.push(node_idx);
        }
    }

    let source_idx =
        source_idx.ok_or_else(|| anyhow!("no source node found in VideoFrames pipeline"))?;
    let sink_idx = sink_idx.ok_or_else(|| anyhow!("no sink node found in VideoFrames pipeline"))?;

    let exec_ctx = ExecutionContext::default();
    let mut outputs_by_node: HashMap<String, HashMap<String, PortData>> = HashMap::new();

    for &node_idx in &execution_order {
        let incoming_vf = count_video_frames_edges(graph, node_idx, Direction::Incoming);
        let outgoing_vf = count_video_frames_edges(graph, node_idx, Direction::Outgoing);
        if incoming_vf > 0 || outgoing_vf > 0 {
            continue;
        }
        let instance = graph.node(node_idx);
        let mut node = registry
            .create(&instance.node_type, instance.params.clone())
            .with_context(|| {
                format!(
                    "failed to instantiate param node '{}' of type '{}'",
                    instance.id, instance.node_type
                )
            })?;
        let inputs = resolve_inputs(graph, registry, node_idx, &outputs_by_node)?;
        let node_outputs = node
            .execute(&inputs, &exec_ctx)
            .with_context(|| format!("execution failed for param node '{}'", instance.id))?;
        emit_print_debug_event(
            &instance.id,
            &instance.node_type,
            &node_outputs,
            &mut node_debug_callback,
        );
        outputs_by_node.insert(instance.id.clone(), node_outputs);
    }

    let source_instance = graph.node(source_idx);
    let mut source_node = registry
        .create(&source_instance.node_type, source_instance.params.clone())
        .with_context(|| {
            format!(
                "failed to instantiate source node '{}' of type '{}'",
                source_instance.id, source_instance.node_type
            )
        })?;
    let source_inputs = resolve_inputs(graph, registry, source_idx, &outputs_by_node)?;
    let source_outputs = source_node
        .execute(&source_inputs, &exec_ctx)
        .with_context(|| format!("execution failed for source node '{}'", source_instance.id))?;
    emit_print_debug_event(
        &source_instance.id,
        &source_instance.node_type,
        &source_outputs,
        &mut node_debug_callback,
    );
    let (decoder, total_frames) = ctx.create_decoder(source_node.as_mut(), &source_outputs)?;
    outputs_by_node.insert(source_instance.id.clone(), source_outputs);

    let mut stages: Vec<PipelineStage> = Vec::new();

    for &node_idx in &processing_order {
        let instance = graph.node(node_idx);
        let mut node = registry
            .create(&instance.node_type, instance.params.clone())
            .with_context(|| {
                format!(
                    "failed to instantiate node '{}' of type '{}'",
                    instance.id, instance.node_type
                )
            })?;
        let inputs = resolve_inputs(graph, registry, node_idx, &outputs_by_node)?;
        let outputs = node
            .execute(&inputs, &exec_ctx)
            .with_context(|| format!("execution failed for node '{}'", instance.id))?;
        emit_print_debug_event(
            &instance.id,
            &instance.node_type,
            &outputs,
            &mut node_debug_callback,
        );
        outputs_by_node.insert(instance.id.clone(), outputs);

        let is_interpolator = ctx.is_interpolator_type(&instance.node_type);
        let node_stages = ctx.create_stages(node, &inputs, is_interpolator)?;
        stages.extend(node_stages);
    }

    let sink_instance = graph.node(sink_idx);
    let mut sink_node = registry
        .create(&sink_instance.node_type, sink_instance.params.clone())
        .with_context(|| {
            format!(
                "failed to instantiate sink node '{}' of type '{}'",
                sink_instance.id, sink_instance.node_type
            )
        })?;
    let sink_inputs = resolve_inputs(graph, registry, sink_idx, &outputs_by_node)?;
    let sink_outputs = match sink_node.execute(&sink_inputs, &exec_ctx) {
        Ok(outputs) => {
            emit_print_debug_event(
                &sink_instance.id,
                &sink_instance.node_type,
                &outputs,
                &mut node_debug_callback,
            );
            outputs
        }
        Err(_) => {
            let mut fallback = HashMap::new();
            for (key, value) in &sink_instance.params {
                if let Ok(pd) = port_data_from_json(&PortType::Path, value)
                    .or_else(|_| port_data_from_json(&PortType::Str, value))
                    .or_else(|_| port_data_from_json(&PortType::Int, value))
                    .or_else(|_| port_data_from_json(&PortType::Bool, value))
                {
                    fallback.insert(key.clone(), pd);
                }
            }
            for (source_idx, conn) in graph.connections_to(sink_idx) {
                if conn.port_type == PortType::VideoFrames {
                    continue;
                }
                if let Some(src_out) = outputs_by_node.get(&graph.node(source_idx).id) {
                    if let Some(data) = src_out.get(&conn.source_port) {
                        fallback.insert(conn.target_port.clone(), clone_port_data(data));
                    }
                }
            }
            fallback
        }
    };
    outputs_by_node.insert(sink_instance.id.clone(), sink_outputs);

    let encoder = ctx.create_encoder(
        sink_node.as_mut(),
        outputs_by_node
            .get(&sink_instance.id)
            .expect("sink outputs just inserted"),
    )?;

    let total_output_frames = ctx.total_output_frames().or(total_frames);

    Ok(CompiledPipeline {
        decoder,
        stages,
        encoder,
        total_frames,
        total_output_frames,
    })
}

fn emit_print_debug_event(
    node_id: &str,
    node_type: &str,
    outputs: &HashMap<String, PortData>,
    node_debug_callback: &mut Option<&mut NodeDebugEventCallback<'_>>,
) {
    let Some(event) = build_print_debug_value_event(node_id, node_type, outputs) else {
        return;
    };

    if let Some(callback) = node_debug_callback.as_mut() {
        (**callback)(event);
    }
}

use petgraph::Direction;

/// Check whether any node in the graph has a VideoFrames port.
fn has_video_frames_ports(
    graph: &PipelineGraph,
    _registry: &NodeRegistry,
    _execution_order: &[NodeIndex],
) -> Result<bool> {
    Ok(graph.has_video_frames_edges())
}

/// Validate that the VideoFrames sub-graph is strictly linear: every node has
/// at most 1 incoming VF edge and at most 1 outgoing VF edge.
fn validate_linear_topology(
    graph: &PipelineGraph,
    _registry: &NodeRegistry,
    execution_order: &[NodeIndex],
) -> Result<()> {
    for &node_idx in execution_order {
        let incoming_vf = count_video_frames_edges(graph, node_idx, Direction::Incoming);
        let outgoing_vf = count_video_frames_edges(graph, node_idx, Direction::Outgoing);

        if incoming_vf > 1 {
            let instance = graph.node(node_idx);
            bail!(
                "node '{}' has {} incoming VideoFrames edges — \
                 compile_graph only supports linear pipelines (fan-in detected)",
                instance.id,
                incoming_vf
            );
        }
        if outgoing_vf > 1 {
            let instance = graph.node(node_idx);
            bail!(
                "node '{}' has {} outgoing VideoFrames edges — \
                 compile_graph only supports linear pipelines (fan-out detected)",
                instance.id,
                outgoing_vf
            );
        }
    }
    Ok(())
}

/// Count VideoFrames-typed edges in the given direction for a node.
fn count_video_frames_edges(
    graph: &PipelineGraph,
    node_idx: NodeIndex,
    direction: Direction,
) -> usize {
    let edges = match direction {
        Direction::Incoming => graph.connections_to(node_idx),
        Direction::Outgoing => graph.connections_from(node_idx),
    };
    edges
        .iter()
        .filter(|(_, conn)| conn.port_type == PortType::VideoFrames)
        .count()
}

/// Resolve input port data for a node by reading upstream outputs and applying
/// default values. Mirrors the pattern in `SequentialExecutor::execute()`.
fn resolve_inputs(
    graph: &PipelineGraph,
    registry: &NodeRegistry,
    node_idx: NodeIndex,
    outputs_by_node: &HashMap<String, HashMap<String, PortData>>,
) -> Result<HashMap<String, PortData>> {
    let instance = graph.node(node_idx);
    let node = registry
        .create(&instance.node_type, instance.params.clone())
        .with_context(|| {
            format!(
                "failed to instantiate node '{}' for input resolution",
                instance.id
            )
        })?;
    let input_port_defs = node.input_ports();
    let mut inputs: HashMap<String, PortData> = HashMap::new();

    for (source_idx, connection) in graph.connections_to(node_idx) {
        // Skip VideoFrames connections — those flow through the streaming
        // pipeline, not through execute() parameter passing.
        if connection.port_type == PortType::VideoFrames {
            continue;
        }

        let source_id = &graph.node(source_idx).id;
        let source_outputs = outputs_by_node
            .get(source_id)
            .ok_or_else(|| anyhow!("missing outputs for upstream node '{source_id}'"))?;

        let data = source_outputs.get(&connection.source_port).ok_or_else(|| {
            anyhow!(
                "upstream node '{}' did not produce output '{}'",
                source_id,
                connection.source_port
            )
        })?;

        inputs.insert(connection.target_port.clone(), clone_port_data(data));
    }

    for input_port in input_port_defs {
        if inputs.contains_key(&input_port.name) {
            continue;
        }

        if let Some(param_value) = instance.params.get(&input_port.name) {
            let decoded =
                port_data_from_json(&input_port.port_type, param_value).with_context(|| {
                    format!(
                        "failed to decode param value for '{}.{}'",
                        instance.id, input_port.name
                    )
                })?;
            inputs.insert(input_port.name.clone(), decoded);
            continue;
        }

        if let Some(default_value) = input_port.default_value {
            let decoded =
                port_data_from_json(&input_port.port_type, &default_value).with_context(|| {
                    format!(
                        "failed to decode default value for '{}.{}'",
                        instance.id, input_port.name
                    )
                })?;
            inputs.insert(input_port.name, decoded);
        }
    }

    Ok(inputs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug_event::NodeDebugValueEvent;
    use crate::graph::{NodeInstance, PortConnection};
    use crate::node::PortDefinition;

    struct MockSourceNode {
        #[allow(dead_code)]
        total_frames: Option<u64>,
    }

    impl Node for MockSourceNode {
        fn node_type(&self) -> &str {
            "mock_source"
        }

        fn input_ports(&self) -> Vec<PortDefinition> {
            vec![]
        }

        fn output_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "frames".to_string(),
                port_type: PortType::VideoFrames,
                required: true,
                default_value: None,
            }]
        }

        fn execute(
            &mut self,
            _inputs: &HashMap<String, PortData>,
            _ctx: &ExecutionContext,
        ) -> Result<HashMap<String, PortData>> {
            Ok(HashMap::new())
        }
    }

    struct MockProcessorNode;

    impl Node for MockProcessorNode {
        fn node_type(&self) -> &str {
            "mock_processor"
        }

        fn input_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "frames".to_string(),
                port_type: PortType::VideoFrames,
                required: true,
                default_value: None,
            }]
        }

        fn output_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "frames".to_string(),
                port_type: PortType::VideoFrames,
                required: true,
                default_value: None,
            }]
        }

        fn execute(
            &mut self,
            _inputs: &HashMap<String, PortData>,
            _ctx: &ExecutionContext,
        ) -> Result<HashMap<String, PortData>> {
            Ok(HashMap::new())
        }
    }

    impl FrameProcessor for MockProcessorNode {
        fn process_frame(&mut self, frame: Frame, _ctx: &ExecutionContext) -> Result<Frame> {
            Ok(frame)
        }
    }

    struct MockInterpolatorNode;

    impl Node for MockInterpolatorNode {
        fn node_type(&self) -> &str {
            "mock_interpolator"
        }

        fn input_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "frames".to_string(),
                port_type: PortType::VideoFrames,
                required: true,
                default_value: None,
            }]
        }

        fn output_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "frames".to_string(),
                port_type: PortType::VideoFrames,
                required: true,
                default_value: None,
            }]
        }

        fn execute(
            &mut self,
            _inputs: &HashMap<String, PortData>,
            _ctx: &ExecutionContext,
        ) -> Result<HashMap<String, PortData>> {
            Ok(HashMap::new())
        }
    }

    struct MockSinkNode;

    impl Node for MockSinkNode {
        fn node_type(&self) -> &str {
            "mock_sink"
        }

        fn input_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "frames".to_string(),
                port_type: PortType::VideoFrames,
                required: true,
                default_value: None,
            }]
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

    #[derive(Clone, Copy)]
    enum PrintCompileRole {
        Param,
        Source,
        Processing,
        Sink,
    }

    impl PrintCompileRole {
        fn from_params(params: &HashMap<String, serde_json::Value>) -> Self {
            match params
                .get("role")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("processing")
            {
                "param" => Self::Param,
                "source" => Self::Source,
                "sink" => Self::Sink,
                _ => Self::Processing,
            }
        }
    }

    struct PrintCompileNode {
        role: PrintCompileRole,
        default_value: String,
    }

    impl Node for PrintCompileNode {
        fn node_type(&self) -> &str {
            "Print"
        }

        fn input_ports(&self) -> Vec<PortDefinition> {
            match self.role {
                PrintCompileRole::Param => vec![],
                PrintCompileRole::Source => vec![PortDefinition {
                    name: "value".to_string(),
                    port_type: PortType::Str,
                    required: true,
                    default_value: None,
                }],
                PrintCompileRole::Processing => vec![
                    PortDefinition {
                        name: "frames".to_string(),
                        port_type: PortType::VideoFrames,
                        required: true,
                        default_value: None,
                    },
                    PortDefinition {
                        name: "value".to_string(),
                        port_type: PortType::Str,
                        required: true,
                        default_value: None,
                    },
                ],
                PrintCompileRole::Sink => vec![
                    PortDefinition {
                        name: "frames".to_string(),
                        port_type: PortType::VideoFrames,
                        required: true,
                        default_value: None,
                    },
                    PortDefinition {
                        name: "value".to_string(),
                        port_type: PortType::Str,
                        required: true,
                        default_value: None,
                    },
                ],
            }
        }

        fn output_ports(&self) -> Vec<PortDefinition> {
            match self.role {
                PrintCompileRole::Param => vec![PortDefinition {
                    name: "value".to_string(),
                    port_type: PortType::Str,
                    required: true,
                    default_value: None,
                }],
                PrintCompileRole::Source => vec![
                    PortDefinition {
                        name: "frames".to_string(),
                        port_type: PortType::VideoFrames,
                        required: true,
                        default_value: None,
                    },
                    PortDefinition {
                        name: "value".to_string(),
                        port_type: PortType::Str,
                        required: true,
                        default_value: None,
                    },
                ],
                PrintCompileRole::Processing => vec![
                    PortDefinition {
                        name: "frames".to_string(),
                        port_type: PortType::VideoFrames,
                        required: true,
                        default_value: None,
                    },
                    PortDefinition {
                        name: "value".to_string(),
                        port_type: PortType::Str,
                        required: true,
                        default_value: None,
                    },
                ],
                PrintCompileRole::Sink => vec![PortDefinition {
                    name: "value".to_string(),
                    port_type: PortType::Str,
                    required: true,
                    default_value: None,
                }],
            }
        }

        fn execute(
            &mut self,
            inputs: &HashMap<String, PortData>,
            _ctx: &ExecutionContext,
        ) -> Result<HashMap<String, PortData>> {
            let value = match self.role {
                PrintCompileRole::Param => self.default_value.clone(),
                PrintCompileRole::Source
                | PrintCompileRole::Processing
                | PrintCompileRole::Sink => match inputs.get("value") {
                    Some(PortData::Str(value)) => value.clone(),
                    _ => self.default_value.clone(),
                },
            };

            Ok(HashMap::from([(
                String::from("value"),
                PortData::Str(value),
            )]))
        }
    }

    struct IntOnlyNode {
        node_type_name: String,
        has_input: bool,
        has_output: bool,
    }

    impl Node for IntOnlyNode {
        fn node_type(&self) -> &str {
            &self.node_type_name
        }

        fn input_ports(&self) -> Vec<PortDefinition> {
            if self.has_input {
                vec![PortDefinition {
                    name: "in".to_string(),
                    port_type: PortType::Int,
                    required: true,
                    default_value: None,
                }]
            } else {
                vec![]
            }
        }

        fn output_ports(&self) -> Vec<PortDefinition> {
            if self.has_output {
                vec![PortDefinition {
                    name: "out".to_string(),
                    port_type: PortType::Int,
                    required: true,
                    default_value: None,
                }]
            } else {
                vec![]
            }
        }

        fn execute(
            &mut self,
            _inputs: &HashMap<String, PortData>,
            _ctx: &ExecutionContext,
        ) -> Result<HashMap<String, PortData>> {
            Ok(HashMap::new())
        }
    }

    struct PassthroughProcessor;

    impl Node for PassthroughProcessor {
        fn node_type(&self) -> &str {
            "passthrough"
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

    impl FrameProcessor for PassthroughProcessor {
        fn process_frame(&mut self, frame: Frame, _ctx: &ExecutionContext) -> Result<Frame> {
            Ok(frame)
        }
    }

    struct DuplicateInterpolator;

    impl FrameInterpolator for DuplicateInterpolator {
        fn interpolate(
            &mut self,
            previous: &Frame,
            _current: &Frame,
            _is_scene_change: bool,
            _ctx: &ExecutionContext,
        ) -> Result<Vec<Frame>> {
            let cloned = match previous {
                Frame::CpuRgb {
                    data,
                    width,
                    height,
                    bit_depth,
                } => Frame::CpuRgb {
                    data: data.clone(),
                    width: *width,
                    height: *height,
                    bit_depth: *bit_depth,
                },
                _ => return Err(anyhow!("unexpected frame type in mock interpolator")),
            };
            Ok(vec![cloned])
        }
    }

    struct MockSink;

    impl FrameSink for MockSink {
        fn write_frame(&mut self, _frame: &Frame) -> Result<()> {
            Ok(())
        }
        fn finish(&mut self) -> Result<()> {
            Ok(())
        }
    }

    struct MockCompileContext {
        decoder_frames: Vec<Frame>,
        total_frames: Option<u64>,
    }

    impl MockCompileContext {
        fn new(num_frames: usize) -> Self {
            let frames: Vec<Frame> = (0..num_frames)
                .map(|i| Frame::CpuRgb {
                    data: vec![i as u8; 3],
                    width: 1,
                    height: 1,
                    bit_depth: 8,
                })
                .collect();
            Self {
                total_frames: Some(num_frames as u64),
                decoder_frames: frames,
            }
        }
    }

    impl CompileContext for MockCompileContext {
        fn create_decoder(
            &self,
            _node: &mut dyn Node,
            _outputs: &HashMap<String, PortData>,
        ) -> Result<(Box<dyn Iterator<Item = Result<Frame>> + Send>, Option<u64>)> {
            let frames: Vec<Result<Frame>> = self
                .decoder_frames
                .iter()
                .map(|f| match f {
                    Frame::CpuRgb {
                        data,
                        width,
                        height,
                        bit_depth,
                    } => Ok(Frame::CpuRgb {
                        data: data.clone(),
                        width: *width,
                        height: *height,
                        bit_depth: *bit_depth,
                    }),
                    _ => Err(anyhow!("unsupported frame type")),
                })
                .collect();
            Ok((Box::new(frames.into_iter()), self.total_frames))
        }

        fn create_encoder(
            &self,
            _node: &mut dyn Node,
            _outputs: &HashMap<String, PortData>,
        ) -> Result<Box<dyn FrameSink>> {
            Ok(Box::new(MockSink))
        }

        fn create_processor(
            &self,
            _node: Box<dyn Node>,
            _inputs: &HashMap<String, PortData>,
        ) -> Result<Box<dyn FrameProcessor>> {
            Ok(Box::new(PassthroughProcessor))
        }

        fn create_interpolator(
            &self,
            _node: Box<dyn Node>,
            _inputs: &HashMap<String, PortData>,
        ) -> Result<Box<dyn FrameInterpolator>> {
            Ok(Box::new(DuplicateInterpolator))
        }

        fn is_interpolator_type(&self, node_type: &str) -> bool {
            node_type == "mock_interpolator"
        }
    }

    fn build_video_registry() -> NodeRegistry {
        let mut registry = NodeRegistry::new();

        registry.register("mock_source", |_| {
            Ok(Box::new(MockSourceNode {
                total_frames: Some(10),
            }))
        });

        registry.register("mock_processor", |_| Ok(Box::new(MockProcessorNode)));
        registry.register("mock_interpolator", |_| Ok(Box::new(MockInterpolatorNode)));
        registry.register("mock_sink", |_| Ok(Box::new(MockSinkNode)));

        registry.register("int_source", |_| {
            Ok(Box::new(IntOnlyNode {
                node_type_name: "int_source".to_string(),
                has_input: false,
                has_output: true,
            }))
        });

        registry.register("int_process", |_| {
            Ok(Box::new(IntOnlyNode {
                node_type_name: "int_process".to_string(),
                has_input: true,
                has_output: true,
            }))
        });

        registry.register("int_sink", |_| {
            Ok(Box::new(IntOnlyNode {
                node_type_name: "int_sink".to_string(),
                has_input: true,
                has_output: false,
            }))
        });

        registry
    }

    fn build_print_compile_registry() -> NodeRegistry {
        let mut registry = build_video_registry();
        registry.register("Print", |params| {
            let role = PrintCompileRole::from_params(&params);
            let default_value = params
                .get("default_value")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("print-default")
                .to_string();

            Ok(Box::new(PrintCompileNode {
                role,
                default_value,
            }))
        });
        registry
    }

    #[test]
    fn test_compile_linear_three_node_graph() {
        let registry = build_video_registry();
        let compile_ctx = MockCompileContext::new(5);

        let mut graph = PipelineGraph::new();
        graph
            .add_node(NodeInstance {
                id: "source".to_string(),
                node_type: "mock_source".to_string(),
                params: HashMap::new(),
            })
            .unwrap();
        graph
            .add_node(NodeInstance {
                id: "processor".to_string(),
                node_type: "mock_processor".to_string(),
                params: HashMap::new(),
            })
            .unwrap();
        graph
            .add_node(NodeInstance {
                id: "sink".to_string(),
                node_type: "mock_sink".to_string(),
                params: HashMap::new(),
            })
            .unwrap();

        graph
            .add_connection(
                "source",
                PortConnection {
                    source_port: "frames".to_string(),
                    target_port: "frames".to_string(),
                    port_type: PortType::VideoFrames,
                },
                "processor",
            )
            .unwrap();
        graph
            .add_connection(
                "processor",
                PortConnection {
                    source_port: "frames".to_string(),
                    target_port: "frames".to_string(),
                    port_type: PortType::VideoFrames,
                },
                "sink",
            )
            .unwrap();

        let compiled = compile_graph(&graph, &registry, &compile_ctx)
            .expect("linear 3-node graph should compile");

        assert_eq!(
            compiled.stages.len(),
            1,
            "should have exactly 1 processing stage"
        );
        assert!(
            matches!(compiled.stages[0], PipelineStage::Processor(_)),
            "stage should be a Processor"
        );
        assert_eq!(compiled.total_frames, Some(5));
    }

    #[test]
    fn test_compile_graph_with_interpolator() {
        let registry = build_video_registry();
        let compile_ctx = MockCompileContext::new(10);

        let mut graph = PipelineGraph::new();
        graph
            .add_node(NodeInstance {
                id: "source".to_string(),
                node_type: "mock_source".to_string(),
                params: HashMap::new(),
            })
            .unwrap();
        graph
            .add_node(NodeInstance {
                id: "processor".to_string(),
                node_type: "mock_processor".to_string(),
                params: HashMap::new(),
            })
            .unwrap();
        graph
            .add_node(NodeInstance {
                id: "interpolator".to_string(),
                node_type: "mock_interpolator".to_string(),
                params: HashMap::new(),
            })
            .unwrap();
        graph
            .add_node(NodeInstance {
                id: "sink".to_string(),
                node_type: "mock_sink".to_string(),
                params: HashMap::new(),
            })
            .unwrap();

        graph
            .add_connection(
                "source",
                PortConnection {
                    source_port: "frames".to_string(),
                    target_port: "frames".to_string(),
                    port_type: PortType::VideoFrames,
                },
                "processor",
            )
            .unwrap();
        graph
            .add_connection(
                "processor",
                PortConnection {
                    source_port: "frames".to_string(),
                    target_port: "frames".to_string(),
                    port_type: PortType::VideoFrames,
                },
                "interpolator",
            )
            .unwrap();
        graph
            .add_connection(
                "interpolator",
                PortConnection {
                    source_port: "frames".to_string(),
                    target_port: "frames".to_string(),
                    port_type: PortType::VideoFrames,
                },
                "sink",
            )
            .unwrap();

        let compiled = compile_graph(&graph, &registry, &compile_ctx)
            .expect("graph with interpolator should compile");

        assert_eq!(compiled.stages.len(), 2, "should have 2 processing stages");
        assert!(
            matches!(compiled.stages[0], PipelineStage::Processor(_)),
            "first stage should be a Processor"
        );
        assert!(
            matches!(compiled.stages[1], PipelineStage::Interpolator(_)),
            "second stage should be an Interpolator"
        );
        assert_eq!(compiled.total_frames, Some(10));
    }

    #[test]
    fn test_compile_rejects_fan_out() {
        let registry = build_video_registry();
        let compile_ctx = MockCompileContext::new(5);

        let mut graph = PipelineGraph::new();
        graph
            .add_node(NodeInstance {
                id: "source".to_string(),
                node_type: "mock_source".to_string(),
                params: HashMap::new(),
            })
            .unwrap();
        graph
            .add_node(NodeInstance {
                id: "proc_a".to_string(),
                node_type: "mock_processor".to_string(),
                params: HashMap::new(),
            })
            .unwrap();
        graph
            .add_node(NodeInstance {
                id: "proc_b".to_string(),
                node_type: "mock_processor".to_string(),
                params: HashMap::new(),
            })
            .unwrap();
        graph
            .add_node(NodeInstance {
                id: "sink".to_string(),
                node_type: "mock_sink".to_string(),
                params: HashMap::new(),
            })
            .unwrap();

        graph
            .add_connection(
                "source",
                PortConnection {
                    source_port: "frames".to_string(),
                    target_port: "frames".to_string(),
                    port_type: PortType::VideoFrames,
                },
                "proc_a",
            )
            .unwrap();
        graph
            .add_connection(
                "source",
                PortConnection {
                    source_port: "frames".to_string(),
                    target_port: "frames".to_string(),
                    port_type: PortType::VideoFrames,
                },
                "proc_b",
            )
            .unwrap();
        graph
            .add_connection(
                "proc_a",
                PortConnection {
                    source_port: "frames".to_string(),
                    target_port: "frames".to_string(),
                    port_type: PortType::VideoFrames,
                },
                "sink",
            )
            .unwrap();

        let err = compile_graph(&graph, &registry, &compile_ctx)
            .expect_err("fan-out graph should be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("fan-out"),
            "error should mention fan-out, got: {msg}"
        );
    }

    #[test]
    fn test_compile_rejects_non_video_frames_graph() {
        let registry = build_video_registry();
        let compile_ctx = MockCompileContext::new(5);

        let mut graph = PipelineGraph::new();
        graph
            .add_node(NodeInstance {
                id: "src".to_string(),
                node_type: "int_source".to_string(),
                params: HashMap::new(),
            })
            .unwrap();
        graph
            .add_node(NodeInstance {
                id: "proc".to_string(),
                node_type: "int_process".to_string(),
                params: HashMap::new(),
            })
            .unwrap();
        graph
            .add_node(NodeInstance {
                id: "snk".to_string(),
                node_type: "int_sink".to_string(),
                params: HashMap::new(),
            })
            .unwrap();

        graph
            .add_connection(
                "src",
                PortConnection {
                    source_port: "out".to_string(),
                    target_port: "in".to_string(),
                    port_type: PortType::Int,
                },
                "proc",
            )
            .unwrap();
        graph
            .add_connection(
                "proc",
                PortConnection {
                    source_port: "out".to_string(),
                    target_port: "in".to_string(),
                    port_type: PortType::Int,
                },
                "snk",
            )
            .unwrap();

        let err = compile_graph(&graph, &registry, &compile_ctx)
            .expect_err("non-VideoFrames graph should be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("VideoFrames"),
            "error should mention VideoFrames, got: {msg}"
        );
    }

    #[test]
    fn test_compile_rejects_missing_source() {
        let registry = build_video_registry();
        let compile_ctx = MockCompileContext::new(5);

        let mut graph = PipelineGraph::new();
        graph
            .add_node(NodeInstance {
                id: "sink".to_string(),
                node_type: "mock_sink".to_string(),
                params: HashMap::new(),
            })
            .unwrap();

        let err = compile_graph(&graph, &registry, &compile_ctx)
            .expect_err("graph without source should be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("no source node") || msg.contains("only handles VideoFrames"),
            "error should reject graph, got: {msg}"
        );
    }

    #[test]
    fn test_compile_source_to_sink_no_processing_stages() {
        let registry = build_video_registry();
        let compile_ctx = MockCompileContext::new(3);

        let mut graph = PipelineGraph::new();
        graph
            .add_node(NodeInstance {
                id: "source".to_string(),
                node_type: "mock_source".to_string(),
                params: HashMap::new(),
            })
            .unwrap();
        graph
            .add_node(NodeInstance {
                id: "sink".to_string(),
                node_type: "mock_sink".to_string(),
                params: HashMap::new(),
            })
            .unwrap();

        graph
            .add_connection(
                "source",
                PortConnection {
                    source_port: "frames".to_string(),
                    target_port: "frames".to_string(),
                    port_type: PortType::VideoFrames,
                },
                "sink",
            )
            .unwrap();

        let compiled = compile_graph(&graph, &registry, &compile_ctx)
            .expect("source-to-sink graph should compile");

        assert!(
            compiled.stages.is_empty(),
            "should have no processing stages"
        );
        assert_eq!(compiled.total_frames, Some(3));
    }

    #[test]
    fn test_compile_graph_print_nodes_emit_debug_events_for_all_execution_sites() {
        let registry = build_print_compile_registry();
        let compile_ctx = MockCompileContext::new(2);

        let mut graph = PipelineGraph::new();
        graph
            .add_node(NodeInstance {
                id: "print_param".to_string(),
                node_type: "Print".to_string(),
                params: HashMap::from([
                    (String::from("role"), serde_json::json!("param")),
                    (
                        String::from("default_value"),
                        serde_json::json!("preview-value"),
                    ),
                ]),
            })
            .expect("param print node should be added");
        graph
            .add_node(NodeInstance {
                id: "print_source".to_string(),
                node_type: "Print".to_string(),
                params: HashMap::from([(String::from("role"), serde_json::json!("source"))]),
            })
            .expect("source print node should be added");
        graph
            .add_node(NodeInstance {
                id: "print_processing".to_string(),
                node_type: "Print".to_string(),
                params: HashMap::from([(String::from("role"), serde_json::json!("processing"))]),
            })
            .expect("processing print node should be added");
        graph
            .add_node(NodeInstance {
                id: "print_sink".to_string(),
                node_type: "Print".to_string(),
                params: HashMap::from([(String::from("role"), serde_json::json!("sink"))]),
            })
            .expect("sink print node should be added");

        graph
            .add_connection(
                "print_param",
                PortConnection {
                    source_port: "value".to_string(),
                    target_port: "value".to_string(),
                    port_type: PortType::Str,
                },
                "print_source",
            )
            .expect("param -> source value connection should be added");
        graph
            .add_connection(
                "print_source",
                PortConnection {
                    source_port: "value".to_string(),
                    target_port: "value".to_string(),
                    port_type: PortType::Str,
                },
                "print_processing",
            )
            .expect("source -> processing value connection should be added");
        graph
            .add_connection(
                "print_processing",
                PortConnection {
                    source_port: "value".to_string(),
                    target_port: "value".to_string(),
                    port_type: PortType::Str,
                },
                "print_sink",
            )
            .expect("processing -> sink value connection should be added");

        graph
            .add_connection(
                "print_source",
                PortConnection {
                    source_port: "frames".to_string(),
                    target_port: "frames".to_string(),
                    port_type: PortType::VideoFrames,
                },
                "print_processing",
            )
            .expect("source -> processing frames connection should be added");
        graph
            .add_connection(
                "print_processing",
                PortConnection {
                    source_port: "frames".to_string(),
                    target_port: "frames".to_string(),
                    port_type: PortType::VideoFrames,
                },
                "print_sink",
            )
            .expect("processing -> sink frames connection should be added");

        let mut events: Vec<NodeDebugValueEvent> = Vec::new();
        let mut callback = |event| events.push(event);

        let compiled =
            compile_graph_with_debug_hook(&graph, &registry, &compile_ctx, Some(&mut callback))
                .expect("print compile graph should compile");

        assert_eq!(compiled.stages.len(), 1, "one processing stage expected");
        assert_eq!(
            events.len(),
            4,
            "param/source/processing/sink should emit events"
        );
        assert_eq!(
            events
                .iter()
                .map(|event| event.node_id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "print_param",
                "print_source",
                "print_processing",
                "print_sink"
            ]
        );
        assert!(events.iter().all(|event| event.node_type == "Print"));
        assert!(events
            .iter()
            .all(|event| event.value_preview == "preview-value"));
    }
}
