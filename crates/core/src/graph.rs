use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, bail, Context, Result};
use petgraph::algo::toposort;
use petgraph::stable_graph::{NodeIndex, StableDiGraph};
use petgraph::visit::{EdgeRef, IntoEdgeReferences};
use petgraph::Direction;
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::node::PortDefinition;
use crate::registry::NodeRegistry;
use crate::types::PortType;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowInterface {
    pub inputs: Vec<WorkflowPort>,
    pub outputs: Vec<WorkflowPort>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowPort {
    pub name: String,
    pub port_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeInstance {
    pub id: String,
    pub node_type: String,
    pub params: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortConnection {
    pub source_port: String,
    pub target_port: String,
    pub port_type: PortType,
}

#[derive(Debug, Clone)]
pub struct PipelineGraph {
    graph: StableDiGraph<NodeInstance, PortConnection>,
    node_ids: HashMap<String, NodeIndex>,
    pub interface: Option<WorkflowInterface>,
}

impl PipelineGraph {
    pub fn new() -> Self {
        Self {
            graph: StableDiGraph::new(),
            node_ids: HashMap::new(),
            interface: None,
        }
    }

    pub fn add_node(&mut self, instance: NodeInstance) -> Result<NodeIndex> {
        if self.node_ids.contains_key(&instance.id) {
            bail!("duplicate node id: {}", instance.id);
        }

        let node_id = instance.id.clone();
        let index = self.graph.add_node(instance);
        self.node_ids.insert(node_id, index);
        Ok(index)
    }

    pub fn add_connection(
        &mut self,
        from_id: &str,
        connection: PortConnection,
        to_id: &str,
    ) -> Result<()> {
        let from_idx = self
            .node_ids
            .get(from_id)
            .copied()
            .ok_or_else(|| anyhow!("unknown source node id: {from_id}"))?;
        let to_idx = self
            .node_ids
            .get(to_id)
            .copied()
            .ok_or_else(|| anyhow!("unknown target node id: {to_id}"))?;

        self.graph.add_edge(from_idx, to_idx, connection);
        Ok(())
    }

    pub fn has_video_frames_edges(&self) -> bool {
        self.graph
            .edge_references()
            .any(|e| e.weight().port_type == PortType::VideoFrames)
    }

    pub fn inject_workflow_input_params(
        &mut self,
        params: &HashMap<String, serde_json::Value>,
    ) -> bool {
        let mut injected = false;
        let node_indices: Vec<_> = self.graph.node_indices().collect();

        for idx in node_indices {
            let Some(node) = self.graph.node_weight_mut(idx) else {
                continue;
            };
            if node.node_type != "WorkflowInput" {
                continue;
            }

            for (key, value) in params {
                if matches!(
                    key.as_str(),
                    "ports" | "interface_inputs" | "interface_outputs"
                ) {
                    continue;
                }

                node.params.insert(key.clone(), value.clone());
                injected = true;
            }
        }

        injected
    }

    pub fn validate(&self, registry: &NodeRegistry) -> Result<()> {
        self.execution_order()?;

        let definitions = self.collect_port_definitions(registry)?;

        for edge in self.graph.edge_references() {
            let source_idx = edge.source();
            let target_idx = edge.target();
            let connection = edge.weight();

            // VideoFrames connections are virtual topology markers handled by
            // compile_graph / StreamingExecutor — skip port-level validation.
            if connection.port_type == PortType::VideoFrames {
                continue;
            }

            let source_node = self.node(source_idx);
            let target_node = self.node(target_idx);

            let source_outputs = &definitions
                .get(&source_idx)
                .expect("source node should be present")
                .1;
            let target_inputs = &definitions
                .get(&target_idx)
                .expect("target node should be present")
                .0;

            let source_port = source_outputs
                .iter()
                .find(|port| port.name == connection.source_port)
                .ok_or_else(|| {
                    anyhow!(
                        "node '{}' has no output port '{}'",
                        source_node.id,
                        connection.source_port
                    )
                })?;

            let target_port = target_inputs
                .iter()
                .find(|port| port.name == connection.target_port)
                .ok_or_else(|| {
                    anyhow!(
                        "node '{}' has no input port '{}'",
                        target_node.id,
                        connection.target_port
                    )
                })?;

            if !source_port.port_type.is_compatible(&target_port.port_type) {
                bail!(
                    "incompatible port types: '{}:{}' ({:?}) -> '{}:{}' ({:?})",
                    source_node.id,
                    connection.source_port,
                    source_port.port_type,
                    target_node.id,
                    connection.target_port,
                    target_port.port_type
                );
            }

            if !connection.port_type.is_compatible(&source_port.port_type)
                || !connection.port_type.is_compatible(&target_port.port_type)
            {
                bail!(
                    "connection '{}:{}' -> '{}:{}' declares {:?}, but node ports are {:?} -> {:?}",
                    source_node.id,
                    connection.source_port,
                    target_node.id,
                    connection.target_port,
                    connection.port_type,
                    source_port.port_type,
                    target_port.port_type
                );
            }
        }

        for (idx, (input_ports, _)) in &definitions {
            let has_vf_edge = self
                .graph
                .edges_directed(*idx, petgraph::Direction::Incoming)
                .chain(
                    self.graph
                        .edges_directed(*idx, petgraph::Direction::Outgoing),
                )
                .any(|e| e.weight().port_type == PortType::VideoFrames);

            if has_vf_edge {
                continue;
            }

            let connected_inputs: HashSet<String> = self
                .connections_to(*idx)
                .into_iter()
                .map(|(_, conn)| conn.target_port.clone())
                .collect();

            let node = self.node(*idx);
            let has_param = |name: &str| -> bool { node.params.contains_key(name) };
            for input in input_ports {
                if input.required
                    && input.default_value.is_none()
                    && !connected_inputs.contains(&input.name)
                    && !has_param(&input.name)
                {
                    bail!(
                        "node '{}' missing required input port '{}'",
                        node.id,
                        input.name
                    );
                }
            }
        }

        Ok(())
    }

    pub fn execution_order(&self) -> Result<Vec<NodeIndex>> {
        toposort(&self.graph, None).map_err(|_| anyhow!("cycle detected in pipeline graph"))
    }

    pub fn node(&self, idx: NodeIndex) -> &NodeInstance {
        self.graph
            .node_weight(idx)
            .expect("node index should be valid")
    }

    pub fn connections_to(&self, idx: NodeIndex) -> Vec<(NodeIndex, &PortConnection)> {
        self.graph
            .edges_directed(idx, Direction::Incoming)
            .map(|edge| (edge.source(), edge.weight()))
            .collect()
    }

    pub fn connections_from(&self, node_idx: NodeIndex) -> Vec<(NodeIndex, &PortConnection)> {
        self.graph
            .edges_directed(node_idx, Direction::Outgoing)
            .map(|edge| (edge.target(), edge.weight()))
            .collect()
    }

    fn collect_port_definitions(
        &self,
        registry: &NodeRegistry,
    ) -> Result<HashMap<NodeIndex, (Vec<PortDefinition>, Vec<PortDefinition>)>> {
        let mut definitions = HashMap::new();

        for idx in self.graph.node_indices() {
            let instance = self.node(idx);
            let node = registry
                .create(&instance.node_type, instance.params.clone())
                .with_context(|| {
                    format!(
                        "failed to instantiate node '{}' of type '{}'",
                        instance.id, instance.node_type
                    )
                })?;

            definitions.insert(idx, (node.input_ports(), node.output_ports()));
        }

        Ok(definitions)
    }
}

impl Default for PipelineGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct PipelineGraphSerde {
    nodes: Vec<NodeInstance>,
    connections: Vec<PipelineConnectionSerde>,
    #[serde(skip_serializing_if = "Option::is_none")]
    interface: Option<WorkflowInterface>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PipelineConnectionSerde {
    from_node: String,
    from_port: String,
    to_node: String,
    to_port: String,
    port_type: PortType,
}

impl Serialize for PipelineGraph {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut nodes: Vec<NodeInstance> = self
            .graph
            .node_indices()
            .map(|idx| self.node(idx).clone())
            .collect();
        nodes.sort_by(|a, b| a.id.cmp(&b.id));

        let mut connections: Vec<PipelineConnectionSerde> = self
            .graph
            .edge_references()
            .map(|edge| {
                let from_node = self.node(edge.source()).id.clone();
                let to_node = self.node(edge.target()).id.clone();
                let weight = edge.weight();

                PipelineConnectionSerde {
                    from_node,
                    from_port: weight.source_port.clone(),
                    to_node,
                    to_port: weight.target_port.clone(),
                    port_type: weight.port_type.clone(),
                }
            })
            .collect();

        connections.sort_by(|a, b| {
            a.from_node
                .cmp(&b.from_node)
                .then_with(|| a.from_port.cmp(&b.from_port))
                .then_with(|| a.to_node.cmp(&b.to_node))
                .then_with(|| a.to_port.cmp(&b.to_port))
                .then_with(|| {
                    port_type_sort_key(&a.port_type).cmp(&port_type_sort_key(&b.port_type))
                })
        });

        PipelineGraphSerde {
            nodes,
            connections,
            interface: self.interface.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PipelineGraph {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let serialized = PipelineGraphSerde::deserialize(deserializer)?;
        let mut graph = PipelineGraph::new();
        graph.interface = serialized.interface;

        for node in serialized.nodes {
            graph.add_node(node).map_err(D::Error::custom)?;
        }

        for connection in serialized.connections {
            graph
                .add_connection(
                    &connection.from_node,
                    PortConnection {
                        source_port: connection.from_port,
                        target_port: connection.to_port,
                        port_type: connection.port_type,
                    },
                    &connection.to_node,
                )
                .map_err(D::Error::custom)?;
        }

        Ok(graph)
    }
}

fn port_type_sort_key(port_type: &PortType) -> u8 {
    match port_type {
        PortType::VideoFrames => 0,
        PortType::Metadata => 1,
        PortType::Model => 2,
        PortType::Int => 3,
        PortType::Float => 4,
        PortType::Str => 5,
        PortType::Bool => 6,
        PortType::Path => 7,
        PortType::WorkflowPath => 8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{ExecutionContext, Node};
    use crate::registry::build_default_registry;
    use crate::types::PortData;

    struct StaticNode {
        node_type: String,
        inputs: Vec<PortDefinition>,
        outputs: Vec<PortDefinition>,
    }

    impl Node for StaticNode {
        fn node_type(&self) -> &str {
            &self.node_type
        }

        fn input_ports(&self) -> Vec<PortDefinition> {
            self.inputs.clone()
        }

        fn output_ports(&self) -> Vec<PortDefinition> {
            self.outputs.clone()
        }

        fn execute(
            &mut self,
            _inputs: &HashMap<String, PortData>,
            _ctx: &ExecutionContext,
        ) -> Result<HashMap<String, PortData>> {
            Ok(HashMap::new())
        }
    }

    fn register_static_node(
        registry: &mut NodeRegistry,
        node_type: &str,
        inputs: Vec<PortDefinition>,
        outputs: Vec<PortDefinition>,
    ) {
        let type_name = node_type.to_string();
        registry.register(node_type, move |_| {
            Ok(Box::new(StaticNode {
                node_type: type_name.clone(),
                inputs: inputs.clone(),
                outputs: outputs.clone(),
            }))
        });
    }

    fn required_port(name: &str, port_type: PortType) -> PortDefinition {
        PortDefinition {
            name: name.to_string(),
            port_type,
            required: true,
            default_value: None,
        }
    }

    #[test]
    fn test_duplicate_node_id_rejected() {
        let mut graph = PipelineGraph::new();
        graph
            .add_node(NodeInstance {
                id: "node".to_string(),
                node_type: "source".to_string(),
                params: HashMap::new(),
            })
            .expect("first node should be added");

        let err = graph
            .add_node(NodeInstance {
                id: "node".to_string(),
                node_type: "sink".to_string(),
                params: HashMap::new(),
            })
            .expect_err("duplicate node id should error");

        assert!(err.to_string().contains("duplicate node id"));
    }

    #[test]
    fn test_cycle_rejection() {
        let mut registry = NodeRegistry::new();
        register_static_node(
            &mut registry,
            "passthrough_int",
            vec![required_port("in", PortType::Int)],
            vec![required_port("out", PortType::Int)],
        );

        let mut graph = PipelineGraph::new();
        for node_id in ["a", "b", "c"] {
            graph
                .add_node(NodeInstance {
                    id: node_id.to_string(),
                    node_type: "passthrough_int".to_string(),
                    params: HashMap::new(),
                })
                .expect("node should be added");
        }

        graph
            .add_connection(
                "a",
                PortConnection {
                    source_port: "out".to_string(),
                    target_port: "in".to_string(),
                    port_type: PortType::Int,
                },
                "b",
            )
            .expect("connection should be added");
        graph
            .add_connection(
                "b",
                PortConnection {
                    source_port: "out".to_string(),
                    target_port: "in".to_string(),
                    port_type: PortType::Int,
                },
                "c",
            )
            .expect("connection should be added");
        graph
            .add_connection(
                "c",
                PortConnection {
                    source_port: "out".to_string(),
                    target_port: "in".to_string(),
                    port_type: PortType::Int,
                },
                "a",
            )
            .expect("connection should be added");

        let err = graph
            .validate(&registry)
            .expect_err("cyclic graph should fail validation");
        assert!(err.to_string().contains("cycle"));
    }

    #[test]
    fn test_type_mismatch_rejection() {
        let mut registry = NodeRegistry::new();
        register_static_node(
            &mut registry,
            "str_source",
            vec![],
            vec![required_port("out", PortType::Str)],
        );
        register_static_node(
            &mut registry,
            "int_sink",
            vec![required_port("in", PortType::Int)],
            vec![],
        );

        let mut graph = PipelineGraph::new();
        graph
            .add_node(NodeInstance {
                id: "source".to_string(),
                node_type: "str_source".to_string(),
                params: HashMap::new(),
            })
            .expect("source node should be added");
        graph
            .add_node(NodeInstance {
                id: "sink".to_string(),
                node_type: "int_sink".to_string(),
                params: HashMap::new(),
            })
            .expect("sink node should be added");
        graph
            .add_connection(
                "source",
                PortConnection {
                    source_port: "out".to_string(),
                    target_port: "in".to_string(),
                    port_type: PortType::Str,
                },
                "sink",
            )
            .expect("connection should be added");

        let err = graph
            .validate(&registry)
            .expect_err("type mismatch should fail validation");
        assert!(err.to_string().contains("incompatible port types"));
    }

    #[test]
    fn test_validate_rejects_legacy_downloader_video_path_output_port() {
        let registry = build_default_registry();

        let mut graph = PipelineGraph::new();
        graph
            .add_node(NodeInstance {
                id: "downloader".to_string(),
                node_type: "Downloader".to_string(),
                params: HashMap::new(),
            })
            .expect("downloader node should be added");
        graph
            .add_node(NodeInstance {
                id: "video_input".to_string(),
                node_type: "VideoInput".to_string(),
                params: HashMap::new(),
            })
            .expect("video input node should be added");
        graph
            .add_connection(
                "downloader",
                PortConnection {
                    source_port: "video_path".to_string(),
                    target_port: "path".to_string(),
                    port_type: PortType::Path,
                },
                "video_input",
            )
            .expect("connection should be added");

        let err = graph
            .validate(&registry)
            .expect_err("legacy downloader output port should fail validation");
        assert_eq!(
            err.to_string(),
            "node 'downloader' has no output port 'video_path'"
        );
    }

    #[test]
    fn test_validate_constant_params_type_drives_output_port_before_execute() {
        let registry = build_default_registry();

        let mut graph = PipelineGraph::new();
        graph
            .add_node(NodeInstance {
                id: "constant".to_string(),
                node_type: "Constant".to_string(),
                params: HashMap::from([
                    ("type".to_string(), serde_json::json!("Str")),
                    ("value".to_string(), serde_json::json!("episode-01")),
                ]),
            })
            .expect("constant node should be added");
        graph
            .add_node(NodeInstance {
                id: "print".to_string(),
                node_type: "Print".to_string(),
                params: HashMap::new(),
            })
            .expect("print node should be added");
        graph
            .add_connection(
                "constant",
                PortConnection {
                    source_port: "value".to_string(),
                    target_port: "value".to_string(),
                    port_type: PortType::Str,
                },
                "print",
            )
            .expect("connection should be added");

        graph
            .validate(&registry)
            .expect("validation should use params.type=Str for Constant output port definition");
    }

    #[test]
    fn test_validate_constant_rejects_invalid_params_type_deterministically() {
        let registry = build_default_registry();

        let mut graph = PipelineGraph::new();
        graph
            .add_node(NodeInstance {
                id: "constant".to_string(),
                node_type: "Constant".to_string(),
                params: HashMap::from([("type".to_string(), serde_json::json!("VideoFrames"))]),
            })
            .expect("constant node should be added");

        let err = match graph.validate(&registry) {
            Ok(_) => panic!("invalid Constant params.type should fail validation"),
            Err(err) => err,
        };

        let full_err = format!("{err:#}");
        assert!(full_err.contains("failed to instantiate node 'constant' of type 'Constant'"));
        assert!(full_err.contains(
            "Constant: unsupported type 'VideoFrames', expected one of Int|Float|Str|Bool|Path"
        ));
    }

    #[test]
    fn test_workflow_json_roundtrip() {
        let mut graph = PipelineGraph::new();
        graph
            .add_node(NodeInstance {
                id: "source".to_string(),
                node_type: "source_node".to_string(),
                params: HashMap::from([(String::from("seed"), serde_json::json!(42))]),
            })
            .expect("source node should be added");
        graph
            .add_node(NodeInstance {
                id: "sink".to_string(),
                node_type: "sink_node".to_string(),
                params: HashMap::new(),
            })
            .expect("sink node should be added");
        graph
            .add_connection(
                "source",
                PortConnection {
                    source_port: "out".to_string(),
                    target_port: "in".to_string(),
                    port_type: PortType::Int,
                },
                "sink",
            )
            .expect("connection should be added");

        let serialized = serde_json::to_value(&graph).expect("graph should serialize");
        assert!(serialized.get("nodes").is_some());
        assert!(serialized.get("connections").is_some());

        let restored: PipelineGraph =
            serde_json::from_value(serialized.clone()).expect("graph should deserialize");
        let reserialized = serde_json::to_value(&restored).expect("graph should reserialize");

        assert_eq!(serialized, reserialized);
    }

    #[test]
    fn test_connections_from_linear_graph_returns_outgoing_edge() {
        let mut graph = PipelineGraph::new();
        let source_idx = graph
            .add_node(NodeInstance {
                id: "a".to_string(),
                node_type: "source".to_string(),
                params: HashMap::new(),
            })
            .expect("source node should be added");
        let middle_idx = graph
            .add_node(NodeInstance {
                id: "b".to_string(),
                node_type: "middle".to_string(),
                params: HashMap::new(),
            })
            .expect("middle node should be added");
        let sink_idx = graph
            .add_node(NodeInstance {
                id: "c".to_string(),
                node_type: "sink".to_string(),
                params: HashMap::new(),
            })
            .expect("sink node should be added");

        graph
            .add_connection(
                "a",
                PortConnection {
                    source_port: "out".to_string(),
                    target_port: "in".to_string(),
                    port_type: PortType::Int,
                },
                "b",
            )
            .expect("connection should be added");
        graph
            .add_connection(
                "b",
                PortConnection {
                    source_port: "out".to_string(),
                    target_port: "in".to_string(),
                    port_type: PortType::Int,
                },
                "c",
            )
            .expect("connection should be added");

        let outgoing = graph.connections_from(source_idx);
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].0, middle_idx);
        assert_eq!(outgoing[0].1.source_port, "out");
        assert_eq!(outgoing[0].1.target_port, "in");
        assert_eq!(outgoing[0].1.port_type, PortType::Int);
        assert_ne!(outgoing[0].0, sink_idx);
    }

    #[test]
    fn test_connections_from_returns_empty_for_node_without_outgoing_edges() {
        let mut graph = PipelineGraph::new();
        let source_idx = graph
            .add_node(NodeInstance {
                id: "a".to_string(),
                node_type: "source".to_string(),
                params: HashMap::new(),
            })
            .expect("source node should be added");
        let middle_idx = graph
            .add_node(NodeInstance {
                id: "b".to_string(),
                node_type: "middle".to_string(),
                params: HashMap::new(),
            })
            .expect("middle node should be added");
        let sink_idx = graph
            .add_node(NodeInstance {
                id: "c".to_string(),
                node_type: "sink".to_string(),
                params: HashMap::new(),
            })
            .expect("sink node should be added");

        graph
            .add_connection(
                "a",
                PortConnection {
                    source_port: "out".to_string(),
                    target_port: "in".to_string(),
                    port_type: PortType::Int,
                },
                "b",
            )
            .expect("connection should be added");
        graph
            .add_connection(
                "b",
                PortConnection {
                    source_port: "out".to_string(),
                    target_port: "in".to_string(),
                    port_type: PortType::Int,
                },
                "c",
            )
            .expect("connection should be added");

        assert_eq!(graph.connections_from(source_idx).len(), 1);
        assert_eq!(graph.connections_from(middle_idx).len(), 1);
        assert!(graph.connections_from(sink_idx).is_empty());
    }

    #[test]
    fn test_inject_workflow_input_params_updates_only_workflow_input_nodes() {
        let mut graph = PipelineGraph::new();
        let workflow_input_idx = graph
            .add_node(NodeInstance {
                id: "wi".to_string(),
                node_type: "WorkflowInput".to_string(),
                params: HashMap::from([(
                    "ports".to_string(),
                    serde_json::json!([{"name": "name", "port_type": "Str"}]),
                )]),
            })
            .expect("workflow input node should be added");
        let sink_idx = graph
            .add_node(NodeInstance {
                id: "sink".to_string(),
                node_type: "Print".to_string(),
                params: HashMap::new(),
            })
            .expect("sink node should be added");

        let injected = graph.inject_workflow_input_params(&HashMap::from([
            ("name".to_string(), serde_json::json!("episode-01")),
            ("ports".to_string(), serde_json::json!("ignored")),
        ]));

        assert!(injected, "workflow input params should be injected");
        assert_eq!(
            graph.node(workflow_input_idx).params.get("name"),
            Some(&serde_json::json!("episode-01"))
        );
        assert_eq!(
            graph.node(workflow_input_idx).params.get("ports"),
            Some(&serde_json::json!([{"name": "name", "port_type": "Str"}]))
        );
        assert!(
            graph.node(sink_idx).params.get("name").is_none(),
            "non-WorkflowInput nodes should remain unchanged"
        );
    }
}
