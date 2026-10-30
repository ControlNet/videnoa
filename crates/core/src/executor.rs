use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};

use crate::compile::{compile_graph_with_execution_context, CompileContext, CompiledPipeline};
use crate::debug_event::{build_print_debug_value_event, NodeDebugEventCallback};
use crate::graph::PipelineGraph;
use crate::node::ExecutionContext;
use crate::registry::NodeRegistry;
use crate::streaming_executor::{
    FrameSink, PipelineFrameCounts, ProgressCallback, StreamingExecutor, DEFAULT_BUFFER_SIZE,
};
use crate::types::{Chapter, Frame, MediaMetadata, PortData, PortType, StreamInfo};

impl FrameSink for Box<dyn FrameSink> {
    fn write_frame(&mut self, frame: &Frame) -> Result<()> {
        (**self).write_frame(frame)
    }

    fn finish(&mut self) -> Result<()> {
        (**self).finish()
    }

    fn finish_unless_cancelled(&mut self, cancelled: &std::sync::atomic::AtomicBool) -> Result<()> {
        (**self).finish_unless_cancelled(cancelled)
    }

    fn release_frame(&mut self, frame: Frame) {
        (**self).release_frame(frame)
    }
}

pub struct SequentialExecutor;

impl SequentialExecutor {
    pub fn execute(
        graph: &PipelineGraph,
        registry: &NodeRegistry,
    ) -> Result<HashMap<String, HashMap<String, PortData>>> {
        Self::execute_with_context(graph, registry, None, None, None)
    }

    pub fn execute_with_context(
        graph: &PipelineGraph,
        registry: &NodeRegistry,
        compile_ctx: Option<&dyn CompileContext>,
        progress_callback: Option<ProgressCallback>,
        cancel_rx: Option<tokio::sync::watch::Receiver<bool>>,
    ) -> Result<HashMap<String, HashMap<String, PortData>>> {
        Self::execute_with_context_and_debug_hook(
            graph,
            registry,
            compile_ctx,
            progress_callback,
            cancel_rx,
            None,
        )
    }

    pub fn execute_with_context_and_debug_hook(
        graph: &PipelineGraph,
        registry: &NodeRegistry,
        compile_ctx: Option<&dyn CompileContext>,
        progress_callback: Option<ProgressCallback>,
        cancel_rx: Option<tokio::sync::watch::Receiver<bool>>,
        mut node_debug_callback: Option<&mut NodeDebugEventCallback<'_>>,
    ) -> Result<HashMap<String, HashMap<String, PortData>>> {
        let ctx = ExecutionContext {
            cancellation: cancel_rx.clone(),
            ..Default::default()
        };
        ctx.check_cancelled()?;
        graph.validate(registry)?;

        let execution_order = graph.execution_order()?;
        if pipeline_uses_video_frames(graph, registry, &execution_order)? {
            let compile_ctx = compile_ctx.ok_or_else(|| {
                anyhow!(
                    "VideoFrames pipeline requires a CompileContext — \
                     use execute_with_context() instead of execute()"
                )
            })?;
            let CompiledPipeline {
                decoder,
                stages,
                encoder,
                total_frames,
                total_output_frames,
                after_stream,
            } = compile_graph_with_execution_context(
                graph,
                registry,
                compile_ctx,
                &ctx,
                node_debug_callback.as_deref_mut(),
            )?;

            let executor = StreamingExecutor::new(DEFAULT_BUFFER_SIZE);
            // A closed, false watch means no external cancellation. The watcher
            // exits on closure; keeping its sender alive would leak one per run.
            let cancel_rx = cancel_rx.unwrap_or_else(|| tokio::sync::watch::channel(false).1);

            let future = executor.execute_pipeline_stages(
                decoder,
                stages,
                encoder,
                PipelineFrameCounts::new(total_frames, total_output_frames),
                cancel_rx,
                progress_callback,
            );

            // block_in_place is required — plain block_on panics inside a tokio runtime.
            match tokio::runtime::Handle::try_current() {
                Ok(handle) => {
                    tokio::task::block_in_place(|| handle.block_on(future))?;
                }
                Err(_) => {
                    let rt = tokio::runtime::Runtime::new()
                        .context("failed to create tokio runtime for video pipeline")?;
                    rt.block_on(future)?;
                }
            }

            // The stream finished and the output is published: only now may
            // the nodes that consume the sink's outputs run.
            after_stream.run(graph, registry, &ctx, node_debug_callback)?;

            return Ok(HashMap::new());
        }

        let mut outputs_by_node: HashMap<String, HashMap<String, PortData>> = HashMap::new();
        for node_idx in execution_order {
            ctx.check_cancelled()?;
            let instance = graph.node(node_idx);
            let mut node = registry
                .create(&instance.node_type, instance.params.clone())
                .with_context(|| {
                    format!(
                        "failed to instantiate node '{}' of type '{}'",
                        instance.id, instance.node_type
                    )
                })?;

            let input_port_defs = node.input_ports();
            let mut inputs: HashMap<String, PortData> = HashMap::new();

            for (source_idx, connection) in graph.connections_to(node_idx) {
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
                    let decoded = port_data_from_json(&input_port.port_type, param_value)
                        .with_context(|| {
                            format!(
                                "failed to decode param value for '{}.{}'",
                                instance.id, input_port.name
                            )
                        })?;
                    inputs.insert(input_port.name.clone(), decoded);
                    continue;
                }

                if let Some(default_value) = input_port.default_value {
                    let decoded = port_data_from_json(&input_port.port_type, &default_value)
                        .with_context(|| {
                            format!(
                                "failed to decode default value for '{}.{}'",
                                instance.id, input_port.name
                            )
                        })?;
                    inputs.insert(input_port.name, decoded);
                }
            }

            let node_outputs = node
                .execute(&inputs, &ctx)
                .with_context(|| format!("execution failed for node '{}'", instance.id))?;
            ctx.check_cancelled()?;

            emit_print_debug_event(
                &instance.id,
                &instance.node_type,
                &node_outputs,
                &mut node_debug_callback,
            );

            outputs_by_node.insert(instance.id.clone(), node_outputs);
        }

        Ok(outputs_by_node)
    }

    pub fn execute_with_params(
        graph: &PipelineGraph,
        registry: &NodeRegistry,
        params: HashMap<String, PortData>,
        outer_ctx: &ExecutionContext,
    ) -> Result<HashMap<String, HashMap<String, PortData>>> {
        Self::execute_with_params_and_debug_hook(graph, registry, params, outer_ctx, None)
    }

    pub fn execute_with_params_and_debug_hook(
        graph: &PipelineGraph,
        registry: &NodeRegistry,
        params: HashMap<String, PortData>,
        outer_ctx: &ExecutionContext,
        mut node_debug_callback: Option<&mut NodeDebugEventCallback<'_>>,
    ) -> Result<HashMap<String, HashMap<String, PortData>>> {
        outer_ctx.check_cancelled()?;
        graph.validate(registry)?;

        let execution_order = graph.execution_order()?;

        let mut outputs_by_node: HashMap<String, HashMap<String, PortData>> = HashMap::new();
        let ctx = ExecutionContext {
            executing_workflows: outer_ctx.executing_workflows.clone(),
            nesting_depth: outer_ctx.nesting_depth,
            cancellation: outer_ctx.cancellation.clone(),
            scratch: outer_ctx.scratch.clone(),
            ..Default::default()
        };

        for node_idx in execution_order {
            ctx.check_cancelled()?;
            let instance = graph.node(node_idx);
            let mut node = registry
                .create(&instance.node_type, instance.params.clone())
                .with_context(|| {
                    format!(
                        "failed to instantiate node '{}' of type '{}'",
                        instance.id, instance.node_type
                    )
                })?;

            let input_port_defs = node.input_ports();
            let mut inputs: HashMap<String, PortData> = HashMap::new();

            if node.node_type() == "WorkflowInput" {
                for (key, value) in &params {
                    inputs.insert(key.clone(), clone_port_data(value));
                }
            }

            for (source_idx, connection) in graph.connections_to(node_idx) {
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
                    let decoded = port_data_from_json(&input_port.port_type, param_value)
                        .with_context(|| {
                            format!(
                                "failed to decode param value for '{}.{}'",
                                instance.id, input_port.name
                            )
                        })?;
                    inputs.insert(input_port.name.clone(), decoded);
                    continue;
                }

                if let Some(default_value) = input_port.default_value {
                    let decoded = port_data_from_json(&input_port.port_type, &default_value)
                        .with_context(|| {
                            format!(
                                "failed to decode default value for '{}.{}'",
                                instance.id, input_port.name
                            )
                        })?;
                    inputs.insert(input_port.name, decoded);
                }
            }

            let node_outputs = node
                .execute(&inputs, &ctx)
                .with_context(|| format!("execution failed for node '{}'", instance.id))?;
            ctx.check_cancelled()?;

            emit_print_debug_event(
                &instance.id,
                &instance.node_type,
                &node_outputs,
                &mut node_debug_callback,
            );

            outputs_by_node.insert(instance.id.clone(), node_outputs);
        }

        Ok(outputs_by_node)
    }
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

fn pipeline_uses_video_frames(
    graph: &PipelineGraph,
    _registry: &NodeRegistry,
    _execution_order: &[petgraph::stable_graph::NodeIndex],
) -> Result<bool> {
    Ok(graph.has_video_frames_edges())
}

pub fn port_data_from_json(port_type: &PortType, value: &serde_json::Value) -> Result<PortData> {
    match port_type {
        PortType::Int => value
            .as_i64()
            .map(PortData::Int)
            .ok_or_else(|| anyhow!("expected integer JSON value")),
        PortType::Float => value
            .as_f64()
            .map(PortData::Float)
            .ok_or_else(|| anyhow!("expected float JSON value")),
        PortType::Str => value
            .as_str()
            .map(|v| PortData::Str(v.to_string()))
            .ok_or_else(|| anyhow!("expected string JSON value")),
        PortType::Bool => value
            .as_bool()
            .map(PortData::Bool)
            .ok_or_else(|| anyhow!("expected bool JSON value")),
        PortType::Path | PortType::WorkflowPath => value
            .as_str()
            .map(|v| PortData::Path(PathBuf::from(v)))
            .ok_or_else(|| anyhow!("expected string JSON value for path")),
        PortType::Metadata => bail!("metadata default values are not supported"),
        PortType::Model => bail!("model default values are not supported"),
        PortType::VideoFrames => bail!("video frame default values are not supported"),
    }
}

pub fn clone_port_data(data: &PortData) -> PortData {
    match data {
        PortData::Metadata(metadata) => PortData::Metadata(clone_media_metadata(metadata)),
        PortData::Int(value) => PortData::Int(*value),
        PortData::Float(value) => PortData::Float(*value),
        PortData::Str(value) => PortData::Str(value.clone()),
        PortData::Bool(value) => PortData::Bool(*value),
        PortData::Path(value) => PortData::Path(value.clone()),
    }
}

fn clone_media_metadata(metadata: &MediaMetadata) -> MediaMetadata {
    MediaMetadata {
        source_path: metadata.source_path.clone(),
        audio_streams: metadata
            .audio_streams
            .iter()
            .map(clone_stream_info)
            .collect(),
        subtitle_streams: metadata
            .subtitle_streams
            .iter()
            .map(clone_stream_info)
            .collect(),
        attachment_streams: metadata
            .attachment_streams
            .iter()
            .map(clone_stream_info)
            .collect(),
        chapters: metadata.chapters.iter().map(clone_chapter).collect(),
        global_metadata: metadata.global_metadata.clone(),
        container_format: metadata.container_format.clone(),
    }
}

fn clone_stream_info(stream: &StreamInfo) -> StreamInfo {
    StreamInfo {
        index: stream.index,
        codec_name: stream.codec_name.clone(),
        codec_type: stream.codec_type.clone(),
        language: stream.language.clone(),
        title: stream.title.clone(),
        metadata: stream.metadata.clone(),
    }
}

fn clone_chapter(chapter: &Chapter) -> Chapter {
    Chapter {
        start_time: chapter.start_time,
        end_time: chapter.end_time,
        title: chapter.title.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug_event::{format_port_data_preview, PRINT_PREVIEW_MAX_CHARS};
    use crate::graph::{NodeInstance, PortConnection};
    use crate::node::{Node, PortDefinition};

    struct InputNode {
        value: i64,
    }

    impl Node for InputNode {
        fn node_type(&self) -> &str {
            "input"
        }

        fn accepts_config_param(&self, key: &str) -> bool {
            key == "value"
        }

        fn input_ports(&self) -> Vec<PortDefinition> {
            vec![]
        }

        fn output_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "out".to_string(),
                port_type: PortType::Int,
                required: true,
                default_value: None,
            }]
        }

        fn execute(
            &mut self,
            _inputs: &HashMap<String, PortData>,
            _ctx: &ExecutionContext,
        ) -> Result<HashMap<String, PortData>> {
            Ok(HashMap::from([(
                String::from("out"),
                PortData::Int(self.value),
            )]))
        }
    }

    struct ProcessNode {
        increment: i64,
    }

    impl Node for ProcessNode {
        fn node_type(&self) -> &str {
            "process"
        }

        fn accepts_config_param(&self, key: &str) -> bool {
            key == "increment"
        }

        fn input_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "in".to_string(),
                port_type: PortType::Int,
                required: true,
                default_value: None,
            }]
        }

        fn output_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "out".to_string(),
                port_type: PortType::Int,
                required: true,
                default_value: None,
            }]
        }

        fn execute(
            &mut self,
            inputs: &HashMap<String, PortData>,
            _ctx: &ExecutionContext,
        ) -> Result<HashMap<String, PortData>> {
            let value = match inputs.get("in") {
                Some(PortData::Int(value)) => *value,
                _ => bail!("expected integer input on port 'in'"),
            };

            Ok(HashMap::from([(
                String::from("out"),
                PortData::Int(value + self.increment),
            )]))
        }
    }

    struct OutputNode;

    impl Node for OutputNode {
        fn node_type(&self) -> &str {
            "output"
        }

        fn input_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "in".to_string(),
                port_type: PortType::Int,
                required: true,
                default_value: None,
            }]
        }

        fn output_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "result".to_string(),
                port_type: PortType::Int,
                required: true,
                default_value: None,
            }]
        }

        fn execute(
            &mut self,
            inputs: &HashMap<String, PortData>,
            _ctx: &ExecutionContext,
        ) -> Result<HashMap<String, PortData>> {
            let value = match inputs.get("in") {
                Some(PortData::Int(value)) => *value,
                _ => bail!("expected integer input on port 'in'"),
            };

            Ok(HashMap::from([(
                String::from("result"),
                PortData::Int(value),
            )]))
        }
    }

    struct PrecedenceProbeNode;

    impl Node for PrecedenceProbeNode {
        fn node_type(&self) -> &str {
            "precedence_probe"
        }

        fn input_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "value".to_string(),
                port_type: PortType::Int,
                required: false,
                default_value: Some(serde_json::json!(3)),
            }]
        }

        fn output_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "result".to_string(),
                port_type: PortType::Int,
                required: true,
                default_value: None,
            }]
        }

        fn execute(
            &mut self,
            inputs: &HashMap<String, PortData>,
            _ctx: &ExecutionContext,
        ) -> Result<HashMap<String, PortData>> {
            let value = match inputs.get("value") {
                Some(PortData::Int(value)) => *value,
                _ => bail!("expected integer input on port 'value'"),
            };

            Ok(HashMap::from([(
                String::from("result"),
                PortData::Int(value),
            )]))
        }
    }

    struct WorkflowInputNode;

    impl Node for WorkflowInputNode {
        fn node_type(&self) -> &str {
            "WorkflowInput"
        }

        fn input_ports(&self) -> Vec<PortDefinition> {
            vec![]
        }

        fn output_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "value".to_string(),
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
            let value = match inputs.get("value") {
                Some(PortData::Str(value)) => value.clone(),
                _ => bail!("expected string input on port 'value'"),
            };
            Ok(HashMap::from([(
                String::from("value"),
                PortData::Str(value),
            )]))
        }
    }

    struct PrintDebugNode;

    impl Node for PrintDebugNode {
        fn node_type(&self) -> &str {
            "Print"
        }

        fn input_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "value".to_string(),
                port_type: PortType::Str,
                required: true,
                default_value: None,
            }]
        }

        fn output_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "value".to_string(),
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
            let value = match inputs.get("value") {
                Some(PortData::Str(value)) => value.clone(),
                _ => bail!("expected string input on port 'value'"),
            };
            Ok(HashMap::from([(
                String::from("value"),
                PortData::Str(value),
            )]))
        }
    }

    struct StringOutputNode;

    impl Node for StringOutputNode {
        fn node_type(&self) -> &str {
            "string_output"
        }

        fn input_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "value".to_string(),
                port_type: PortType::Str,
                required: true,
                default_value: None,
            }]
        }

        fn output_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "result".to_string(),
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
            let value = match inputs.get("value") {
                Some(PortData::Str(value)) => value.clone(),
                _ => bail!("expected string input on port 'value'"),
            };

            Ok(HashMap::from([(
                String::from("result"),
                PortData::Str(value),
            )]))
        }
    }

    fn build_registry() -> NodeRegistry {
        let mut registry = NodeRegistry::new();

        registry.register("input", |params| {
            let value = params
                .get("value")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            Ok(Box::new(InputNode { value }))
        });

        registry.register("process", |params| {
            let increment = params
                .get("increment")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            Ok(Box::new(ProcessNode { increment }))
        });

        registry.register("output", |_| Ok(Box::new(OutputNode)));
        registry.register("precedence_probe", |_| Ok(Box::new(PrecedenceProbeNode)));
        registry.register("WorkflowInput", |_| Ok(Box::new(WorkflowInputNode)));
        registry.register("Print", |_| Ok(Box::new(PrintDebugNode)));
        registry.register("string_output", |_| Ok(Box::new(StringOutputNode)));

        registry
    }

    fn execute_precedence_graph_with_context(
        node_params: HashMap<String, serde_json::Value>,
        edge_value: Option<i64>,
    ) -> i64 {
        let registry = build_registry();
        let mut graph = PipelineGraph::new();

        graph
            .add_node(NodeInstance {
                id: "probe".to_string(),
                node_type: "precedence_probe".to_string(),
                params: node_params,
            })
            .expect("precedence probe should be added");

        if let Some(value) = edge_value {
            graph
                .add_node(NodeInstance {
                    id: "input".to_string(),
                    node_type: "input".to_string(),
                    params: HashMap::from([(String::from("value"), serde_json::json!(value))]),
                })
                .expect("input node should be added");
            graph
                .add_connection(
                    "input",
                    PortConnection {
                        source_port: "out".to_string(),
                        target_port: "value".to_string(),
                        port_type: PortType::Int,
                    },
                    "probe",
                )
                .expect("input -> probe connection should be added");
        }

        let outputs = SequentialExecutor::execute_with_context_and_debug_hook(
            &graph, &registry, None, None, None, None,
        )
        .expect("precedence graph should execute with context entrypoint");

        match outputs
            .get("probe")
            .and_then(|ports| ports.get("result"))
            .expect("probe should expose result")
        {
            PortData::Int(value) => *value,
            _ => panic!("probe result should be integer"),
        }
    }

    fn execute_precedence_graph_with_params(
        node_params: HashMap<String, serde_json::Value>,
        edge_value: Option<i64>,
    ) -> i64 {
        let registry = build_registry();
        let mut graph = PipelineGraph::new();

        graph
            .add_node(NodeInstance {
                id: "probe".to_string(),
                node_type: "precedence_probe".to_string(),
                params: node_params,
            })
            .expect("precedence probe should be added");

        if let Some(value) = edge_value {
            graph
                .add_node(NodeInstance {
                    id: "input".to_string(),
                    node_type: "input".to_string(),
                    params: HashMap::from([(String::from("value"), serde_json::json!(value))]),
                })
                .expect("input node should be added");
            graph
                .add_connection(
                    "input",
                    PortConnection {
                        source_port: "out".to_string(),
                        target_port: "value".to_string(),
                        port_type: PortType::Int,
                    },
                    "probe",
                )
                .expect("input -> probe connection should be added");
        }

        let outputs = SequentialExecutor::execute_with_params_and_debug_hook(
            &graph,
            &registry,
            HashMap::new(),
            &ExecutionContext::default(),
            None,
        )
        .expect("precedence graph should execute with params entrypoint");

        match outputs
            .get("probe")
            .and_then(|ports| ports.get("result"))
            .expect("probe should expose result")
        {
            PortData::Int(value) => *value,
            _ => panic!("probe result should be integer"),
        }
    }

    #[test]
    fn test_linear_graph_execution() {
        let registry = build_registry();

        let mut graph = PipelineGraph::new();
        graph
            .add_node(NodeInstance {
                id: "input".to_string(),
                node_type: "input".to_string(),
                params: HashMap::from([(String::from("value"), serde_json::json!(40))]),
            })
            .expect("input node should be added");
        graph
            .add_node(NodeInstance {
                id: "process".to_string(),
                node_type: "process".to_string(),
                params: HashMap::from([(String::from("increment"), serde_json::json!(2))]),
            })
            .expect("process node should be added");
        graph
            .add_node(NodeInstance {
                id: "output".to_string(),
                node_type: "output".to_string(),
                params: HashMap::new(),
            })
            .expect("output node should be added");

        graph
            .add_connection(
                "input",
                PortConnection {
                    source_port: "out".to_string(),
                    target_port: "in".to_string(),
                    port_type: PortType::Int,
                },
                "process",
            )
            .expect("input -> process connection should be added");
        graph
            .add_connection(
                "process",
                PortConnection {
                    source_port: "out".to_string(),
                    target_port: "in".to_string(),
                    port_type: PortType::Int,
                },
                "output",
            )
            .expect("process -> output connection should be added");

        let outputs = SequentialExecutor::execute(&graph, &registry)
            .expect("linear integer graph should execute successfully");

        let output_node_ports = outputs
            .get("output")
            .expect("output node should have produced data");
        let result = output_node_ports
            .get("result")
            .expect("output node should expose result port");

        match result {
            PortData::Int(value) => assert_eq!(*value, 42),
            _ => panic!("result should be integer"),
        }
    }

    #[test]
    fn test_execute_with_context_precedence_edge_over_param_and_default() {
        let result = execute_precedence_graph_with_context(
            HashMap::from([(String::from("value"), serde_json::json!(20))]),
            Some(7),
        );

        assert_eq!(
            result, 7,
            "connected edge input must win over params/default"
        );
    }

    #[test]
    fn test_execute_with_context_precedence_param_over_default() {
        let result = execute_precedence_graph_with_context(
            HashMap::from([(String::from("value"), serde_json::json!(20))]),
            None,
        );

        assert_eq!(result, 20, "node params must win over default when no edge");
    }

    #[test]
    fn test_execute_with_context_precedence_default_when_missing_edge_and_param() {
        let result = execute_precedence_graph_with_context(HashMap::new(), None);

        assert_eq!(
            result, 3,
            "default must be used when edge/param are missing"
        );
    }

    #[test]
    fn test_execute_with_params_precedence_matches_context_entrypoint() {
        let edge_wins = execute_precedence_graph_with_params(
            HashMap::from([(String::from("value"), serde_json::json!(20))]),
            Some(7),
        );
        assert_eq!(edge_wins, 7, "edge should still win in params entrypoint");

        let param_wins = execute_precedence_graph_with_params(
            HashMap::from([(String::from("value"), serde_json::json!(20))]),
            None,
        );
        assert_eq!(
            param_wins, 20,
            "params should win over default without edge"
        );

        let default_used = execute_precedence_graph_with_params(HashMap::new(), None);
        assert_eq!(
            default_used, 3,
            "default should be used when edge and params are missing"
        );
    }

    #[test]
    fn test_print_node_emits_debug_event() {
        let registry = build_registry();

        let mut graph = PipelineGraph::new();
        graph
            .add_node(NodeInstance {
                id: "workflow_input".to_string(),
                node_type: "WorkflowInput".to_string(),
                params: HashMap::new(),
            })
            .expect("workflow input should be added");
        graph
            .add_node(NodeInstance {
                id: "print_1".to_string(),
                node_type: "Print".to_string(),
                params: HashMap::new(),
            })
            .expect("print node should be added");
        graph
            .add_node(NodeInstance {
                id: "sink".to_string(),
                node_type: "string_output".to_string(),
                params: HashMap::new(),
            })
            .expect("sink should be added");

        graph
            .add_connection(
                "workflow_input",
                PortConnection {
                    source_port: "value".to_string(),
                    target_port: "value".to_string(),
                    port_type: PortType::Str,
                },
                "print_1",
            )
            .expect("workflow_input -> print should be added");
        graph
            .add_connection(
                "print_1",
                PortConnection {
                    source_port: "value".to_string(),
                    target_port: "value".to_string(),
                    port_type: PortType::Str,
                },
                "sink",
            )
            .expect("print -> sink should be added");

        let mut events = Vec::new();
        let mut callback = |event| events.push(event);
        let outputs = SequentialExecutor::execute_with_params_and_debug_hook(
            &graph,
            &registry,
            HashMap::from([(String::from("value"), PortData::Str(String::from("hello")))]),
            &ExecutionContext::default(),
            Some(&mut callback),
        )
        .expect("graph with print should execute successfully");

        assert_eq!(events.len(), 1, "print should emit exactly one debug event");
        assert_eq!(events[0].node_id, "print_1");
        assert_eq!(events[0].node_type, "Print");
        assert_eq!(events[0].value_preview, "hello");
        assert!(!events[0].truncated);
        assert_eq!(events[0].preview_max_chars, PRINT_PREVIEW_MAX_CHARS);

        let sink_outputs = outputs
            .get("sink")
            .expect("sink node should have produced data");
        assert!(
            matches!(sink_outputs.get("result"), Some(PortData::Str(v)) if v == "hello"),
            "sink should receive print pass-through output"
        );
    }

    #[test]
    fn test_print_preview_truncates_with_flag() {
        let long_value = "x".repeat(PRINT_PREVIEW_MAX_CHARS + 17);
        let (preview, truncated) =
            format_port_data_preview(&PortData::Str(long_value.clone()), PRINT_PREVIEW_MAX_CHARS);

        assert!(
            truncated,
            "preview should mark truncation for oversized value"
        );
        assert_eq!(preview.chars().count(), PRINT_PREVIEW_MAX_CHARS);
        assert_eq!(preview, "x".repeat(PRINT_PREVIEW_MAX_CHARS));
        assert!(long_value.starts_with(&preview));
    }

    #[test]
    fn test_non_print_nodes_do_not_emit_debug_event() {
        let registry = build_registry();

        let mut graph = PipelineGraph::new();
        graph
            .add_node(NodeInstance {
                id: "input".to_string(),
                node_type: "input".to_string(),
                params: HashMap::from([(String::from("value"), serde_json::json!(40))]),
            })
            .expect("input node should be added");
        graph
            .add_node(NodeInstance {
                id: "process".to_string(),
                node_type: "process".to_string(),
                params: HashMap::from([(String::from("increment"), serde_json::json!(2))]),
            })
            .expect("process node should be added");
        graph
            .add_node(NodeInstance {
                id: "output".to_string(),
                node_type: "output".to_string(),
                params: HashMap::new(),
            })
            .expect("output node should be added");

        graph
            .add_connection(
                "input",
                PortConnection {
                    source_port: "out".to_string(),
                    target_port: "in".to_string(),
                    port_type: PortType::Int,
                },
                "process",
            )
            .expect("input -> process connection should be added");
        graph
            .add_connection(
                "process",
                PortConnection {
                    source_port: "out".to_string(),
                    target_port: "in".to_string(),
                    port_type: PortType::Int,
                },
                "output",
            )
            .expect("process -> output connection should be added");

        let mut events = Vec::new();
        let mut callback = |event| events.push(event);
        SequentialExecutor::execute_with_context_and_debug_hook(
            &graph,
            &registry,
            None,
            None,
            None,
            Some(&mut callback),
        )
        .expect("non-print graph should execute successfully");

        assert!(
            events.is_empty(),
            "non-Print nodes should not emit debug events"
        );
    }

    /// Whether the published output existed each time the node that consumes
    /// it ran.
    type Observations = std::sync::Arc<std::sync::Mutex<Vec<bool>>>;

    struct FrameSourceNode;

    impl Node for FrameSourceNode {
        fn node_type(&self) -> &str {
            "frame_source"
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

    /// Like VideoOutput: reports the path it publishes the stream to.
    struct PublishingSinkNode;

    impl Node for PublishingSinkNode {
        fn node_type(&self) -> &str {
            "publishing_sink"
        }

        fn input_ports(&self) -> Vec<PortDefinition> {
            vec![
                PortDefinition {
                    name: "frames".to_string(),
                    port_type: PortType::VideoFrames,
                    required: true,
                    default_value: None,
                },
                PortDefinition {
                    name: "output_path".to_string(),
                    port_type: PortType::Path,
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
            let output_path = inputs
                .get("output_path")
                .map(clone_port_data)
                .ok_or_else(|| anyhow!("missing output_path"))?;
            Ok(HashMap::from([("output_path".to_string(), output_path)]))
        }
    }

    /// A "done" notification: records whether the path it is given exists.
    struct ObserveOutputNode {
        observations: Observations,
    }

    impl Node for ObserveOutputNode {
        fn node_type(&self) -> &str {
            "observe_output"
        }

        fn input_ports(&self) -> Vec<PortDefinition> {
            vec![
                PortDefinition {
                    name: "path".to_string(),
                    port_type: PortType::Path,
                    required: true,
                    default_value: None,
                },
                PortDefinition {
                    name: "fail".to_string(),
                    port_type: PortType::Bool,
                    required: false,
                    default_value: Some(serde_json::json!(false)),
                },
            ]
        }

        fn output_ports(&self) -> Vec<PortDefinition> {
            vec![PortDefinition {
                name: "path".to_string(),
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
            let Some(PortData::Path(path)) = inputs.get("path") else {
                bail!("missing path");
            };
            self.observations.lock().unwrap().push(path.exists());
            if matches!(inputs.get("fail"), Some(PortData::Bool(true))) {
                bail!("notification endpoint refused the request");
            }
            Ok(HashMap::from([(
                "path".to_string(),
                PortData::Path(path.clone()),
            )]))
        }
    }

    /// Publishes its output file when the stream finishes, unless cancelled.
    struct PublishingSink {
        path: PathBuf,
        cancel_on_first_frame: Option<tokio::sync::watch::Sender<bool>>,
    }

    impl FrameSink for PublishingSink {
        fn write_frame(&mut self, _frame: &Frame) -> Result<()> {
            if let Some(cancel) = &self.cancel_on_first_frame {
                cancel.send_replace(true);
            }
            Ok(())
        }

        fn finish(&mut self) -> Result<()> {
            std::fs::write(&self.path, b"published")?;
            Ok(())
        }

        fn finish_unless_cancelled(
            &mut self,
            cancelled: &std::sync::atomic::AtomicBool,
        ) -> Result<()> {
            if cancelled.load(std::sync::atomic::Ordering::SeqCst) {
                return Ok(());
            }
            self.finish()
        }
    }

    #[derive(Default)]
    struct StreamCompileContext {
        fail_decoding_at: Option<usize>,
        cancel_on_first_frame: Option<tokio::sync::watch::Sender<bool>>,
    }

    impl CompileContext for StreamCompileContext {
        fn create_decoder(
            &self,
            _node: &mut dyn Node,
            _outputs: &HashMap<String, PortData>,
        ) -> crate::compile::DecoderResult {
            let fail_at = self.fail_decoding_at;
            let frames = (0..3).map(move |index| {
                if Some(index) == fail_at {
                    bail!("decoder failed at frame {index}");
                }
                Ok(Frame::CpuRgb {
                    data: vec![0; 3],
                    width: 1,
                    height: 1,
                    bit_depth: 8,
                })
            });
            Ok((Box::new(frames), Some(3)))
        }

        fn create_encoder(
            &self,
            _node: &mut dyn Node,
            _inputs: &HashMap<String, PortData>,
            outputs: &HashMap<String, PortData>,
        ) -> Result<Box<dyn FrameSink>> {
            let Some(PortData::Path(path)) = outputs.get("output_path") else {
                bail!("sink did not report output_path");
            };
            Ok(Box::new(PublishingSink {
                path: path.clone(),
                cancel_on_first_frame: self.cancel_on_first_frame.clone(),
            }))
        }

        fn create_processor(
            &self,
            node: Box<dyn Node>,
            _inputs: &HashMap<String, PortData>,
        ) -> Result<Box<dyn crate::node::FrameProcessor>> {
            bail!("unexpected processor '{}'", node.node_type())
        }

        fn create_interpolator(
            &self,
            node: Box<dyn Node>,
            _inputs: &HashMap<String, PortData>,
        ) -> Result<Box<dyn crate::streaming_executor::FrameInterpolator>> {
            bail!("unexpected interpolator '{}'", node.node_type())
        }

        fn is_interpolator_type(&self, _node_type: &str) -> bool {
            false
        }
    }

    /// `source -> sink`, with `sink.output_path -> notify` and, when `chained`,
    /// `notify.path -> notify_again`: a consumer of the published output that
    /// is one node further away.
    fn graph_notifying_on_output(output: &std::path::Path, fail: bool) -> PipelineGraph {
        let mut graph = PipelineGraph::new();
        for (id, node_type, params) in [
            ("source", "frame_source", HashMap::new()),
            (
                "sink",
                "publishing_sink",
                HashMap::from([("output_path".to_string(), serde_json::json!(output))]),
            ),
            ("notify", "observe_output", HashMap::new()),
            (
                "notify_again",
                "observe_output",
                HashMap::from([("fail".to_string(), serde_json::json!(fail))]),
            ),
        ] {
            graph
                .add_node(NodeInstance {
                    id: id.to_string(),
                    node_type: node_type.to_string(),
                    params,
                })
                .unwrap();
        }
        for (from, from_port, to, to_port, port_type) in [
            ("source", "frames", "sink", "frames", PortType::VideoFrames),
            ("sink", "output_path", "notify", "path", PortType::Path),
            ("notify", "path", "notify_again", "path", PortType::Path),
        ] {
            graph
                .add_connection(
                    from,
                    PortConnection {
                        source_port: from_port.to_string(),
                        target_port: to_port.to_string(),
                        port_type,
                    },
                    to,
                )
                .unwrap();
        }
        graph
    }

    fn stream_registry(observations: &Observations) -> NodeRegistry {
        let mut registry = NodeRegistry::new();
        registry.register("frame_source", |_| Ok(Box::new(FrameSourceNode)));
        registry.register("publishing_sink", |_| Ok(Box::new(PublishingSinkNode)));
        let observations = observations.clone();
        registry.register("observe_output", move |_| {
            Ok(Box::new(ObserveOutputNode {
                observations: observations.clone(),
            }))
        });
        registry
    }

    fn run_stream(
        graph: &PipelineGraph,
        registry: &NodeRegistry,
        compile_ctx: &StreamCompileContext,
        cancel_rx: Option<tokio::sync::watch::Receiver<bool>>,
    ) -> Result<()> {
        SequentialExecutor::execute_with_context(
            graph,
            registry,
            Some(compile_ctx),
            None,
            cancel_rx,
        )
        .map(drop)
    }

    #[test]
    fn nodes_that_consume_the_sink_output_run_after_it_is_published() {
        let dir = tempfile::TempDir::new().unwrap();
        let output = dir.path().join("out.mkv");
        let observations = Observations::default();
        let registry = stream_registry(&observations);
        let graph = graph_notifying_on_output(&output, false);

        run_stream(&graph, &registry, &StreamCompileContext::default(), None)
            .expect("the stream should complete");

        assert_eq!(
            *observations.lock().unwrap(),
            vec![true, true],
            "both consumers must run, and only once the output exists"
        );
    }

    #[test]
    fn nodes_that_consume_the_sink_output_do_not_run_when_the_stream_fails() {
        let dir = tempfile::TempDir::new().unwrap();
        let output = dir.path().join("out.mkv");
        let observations = Observations::default();
        let registry = stream_registry(&observations);
        let graph = graph_notifying_on_output(&output, false);
        let compile_ctx = StreamCompileContext {
            fail_decoding_at: Some(1),
            ..Default::default()
        };

        let err = run_stream(&graph, &registry, &compile_ctx, None)
            .expect_err("a failed decode must fail the run");

        assert!(format!("{err:#}").contains("decoder failed"), "{err:#}");
        assert!(observations.lock().unwrap().is_empty());
        assert!(!output.exists());
    }

    #[test]
    fn nodes_that_consume_the_sink_output_do_not_run_when_the_stream_is_cancelled() {
        let dir = tempfile::TempDir::new().unwrap();
        let output = dir.path().join("out.mkv");
        let observations = Observations::default();
        let registry = stream_registry(&observations);
        let graph = graph_notifying_on_output(&output, false);
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let compile_ctx = StreamCompileContext {
            cancel_on_first_frame: Some(cancel_tx),
            ..Default::default()
        };

        let err = run_stream(&graph, &registry, &compile_ctx, Some(cancel_rx))
            .expect_err("a cancelled stream must not complete");

        assert!(crate::node::is_cancellation(&err), "{err:#}");
        assert!(observations.lock().unwrap().is_empty());
    }

    #[test]
    fn a_failing_consumer_of_the_sink_output_fails_the_run() {
        let dir = tempfile::TempDir::new().unwrap();
        let output = dir.path().join("out.mkv");
        let observations = Observations::default();
        let registry = stream_registry(&observations);
        let graph = graph_notifying_on_output(&output, true);

        let err = run_stream(&graph, &registry, &StreamCompileContext::default(), None)
            .expect_err("a failing notification must fail the run");

        let message = format!("{err:#}");
        assert!(message.contains("notify_again"), "{message}");
        assert!(message.contains("refused the request"), "{message}");
        assert_eq!(*observations.lock().unwrap(), vec![true, true]);
    }
}
