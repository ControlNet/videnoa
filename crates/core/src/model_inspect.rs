use std::path::Path;

use anyhow::{Context, Result};
use prost::Message;
use serde::Serialize;

/// Generated ONNX protobuf types from `proto/onnx.proto3`.
mod onnx_proto {
    include!(concat!(env!("OUT_DIR"), "/onnx.rs"));
}

#[derive(Debug, Clone, Serialize)]
pub struct TensorInfo {
    pub name: String,
    /// Human-readable data type, e.g. "float32", "float16", "int64".
    pub data_type: String,
    /// Dimensions. `-1` represents a dynamic/symbolic dimension.
    pub shape: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub op_type: String,
    pub name: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelInspection {
    pub ir_version: i64,
    pub opset_version: i64,
    pub producer_name: String,
    pub producer_version: String,
    pub domain: String,
    pub model_version: i64,
    pub doc_string: String,
    pub inputs: Vec<TensorInfo>,
    pub outputs: Vec<TensorInfo>,
    pub nodes: Vec<GraphNode>,
    /// Sum of all initializer tensor element counts.
    pub param_count: u64,
    /// Number of graph nodes (operations).
    pub op_count: usize,
}

/// Map ONNX `TensorProto.DataType` enum value to a human-readable string.
fn data_type_name(dt: i32) -> String {
    match dt {
        1 => "float32".into(),
        2 => "uint8".into(),
        3 => "int8".into(),
        4 => "uint16".into(),
        5 => "int16".into(),
        6 => "int32".into(),
        7 => "int64".into(),
        8 => "string".into(),
        9 => "bool".into(),
        10 => "float16".into(),
        11 => "float64".into(),
        12 => "uint32".into(),
        13 => "uint64".into(),
        14 => "complex64".into(),
        15 => "complex128".into(),
        16 => "bfloat16".into(),
        17 => "float8e4m3fn".into(),
        18 => "float8e4m3fnuz".into(),
        19 => "float8e5m2".into(),
        20 => "float8e5m2fnuz".into(),
        21 => "uint4".into(),
        22 => "int4".into(),
        23 => "float4e2m1".into(),
        _ => format!("unknown({dt})"),
    }
}

/// Extract a [`TensorInfo`] from an ONNX `ValueInfoProto`.
fn value_info_to_tensor(vi: &onnx_proto::ValueInfoProto) -> TensorInfo {
    let (data_type, shape) = vi
        .r#type
        .as_ref()
        .and_then(|tp| tp.value.as_ref())
        .map(|val| match val {
            onnx_proto::type_proto::Value::TensorType(t) => {
                let dt = data_type_name(t.elem_type);
                let dims = t
                    .shape
                    .as_ref()
                    .map(|s| {
                        s.dim
                            .iter()
                            .map(|d| match &d.value {
                                Some(
                                    onnx_proto::tensor_shape_proto::dimension::Value::DimValue(v),
                                ) => *v,
                                _ => -1, // symbolic / unknown
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                (dt, dims)
            }
        })
        .unwrap_or_else(|| ("unknown".into(), vec![]));

    TensorInfo {
        name: vi.name.clone(),
        data_type,
        shape,
    }
}

/// Count the total number of elements across all dimensions.
fn tensor_element_count(dims: &[i64]) -> u64 {
    if dims.is_empty() {
        return 0;
    }
    dims.iter()
        .map(|&d| if d > 0 { d as u64 } else { 1 })
        .product()
}

/// Inspect an ONNX model file and extract metadata without loading it into
/// a runtime. Does NOT require a GPU.
pub fn inspect_onnx(path: &Path) -> Result<ModelInspection> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read ONNX file: {}", path.display()))?;

    inspect_onnx_bytes(&bytes)
}

/// Inspect ONNX model from raw bytes (useful for testing).
pub fn inspect_onnx_bytes(bytes: &[u8]) -> Result<ModelInspection> {
    let model = onnx_proto::ModelProto::decode(bytes).context("failed to decode ONNX protobuf")?;

    let opset_version = model
        .opset_import
        .iter()
        .filter(|op| op.domain.is_empty()) // default ONNX domain
        .map(|op| op.version)
        .max()
        .unwrap_or(0);

    let graph = model.graph.as_ref().context("ONNX model has no graph")?;

    let inputs: Vec<TensorInfo> = graph.input.iter().map(value_info_to_tensor).collect();
    let outputs: Vec<TensorInfo> = graph.output.iter().map(value_info_to_tensor).collect();

    let nodes: Vec<GraphNode> = graph
        .node
        .iter()
        .map(|n| GraphNode {
            op_type: n.op_type.clone(),
            name: n.name.clone(),
            inputs: n.input.clone(),
            outputs: n.output.clone(),
        })
        .collect();

    let param_count: u64 = graph
        .initializer
        .iter()
        .map(|t| tensor_element_count(&t.dims))
        .sum();

    let op_count = graph.node.len();

    Ok(ModelInspection {
        ir_version: model.ir_version,
        opset_version,
        producer_name: model.producer_name,
        producer_version: model.producer_version,
        domain: model.domain,
        model_version: model.model_version,
        doc_string: model.doc_string,
        inputs,
        outputs,
        nodes,
        param_count,
        op_count,
    })
}

/// Check that a filename is safe: no path separators, no `..`, non-empty.
pub fn sanitize_model_filename(filename: &str) -> Result<(), &'static str> {
    if filename.is_empty() {
        return Err("filename must not be empty");
    }
    if filename.contains('/') || filename.contains('\\') {
        return Err("filename must not contain path separators");
    }
    if filename.contains("..") {
        return Err("filename must not contain '..'");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid ONNX protobuf: a single Add node with 2 float32
    /// inputs of shape [1, 3] and 1 output of shape [1, 3].
    fn build_test_onnx_bytes() -> Vec<u8> {
        use onnx_proto::*;

        let input_a = ValueInfoProto {
            name: "A".into(),
            r#type: Some(TypeProto {
                value: Some(type_proto::Value::TensorType(type_proto::Tensor {
                    elem_type: 1, // FLOAT
                    shape: Some(TensorShapeProto {
                        dim: vec![
                            tensor_shape_proto::Dimension {
                                value: Some(tensor_shape_proto::dimension::Value::DimValue(1)),
                            },
                            tensor_shape_proto::Dimension {
                                value: Some(tensor_shape_proto::dimension::Value::DimValue(3)),
                            },
                        ],
                    }),
                })),
            }),
        };

        let input_b = ValueInfoProto {
            name: "B".into(),
            r#type: Some(TypeProto {
                value: Some(type_proto::Value::TensorType(type_proto::Tensor {
                    elem_type: 1,
                    shape: Some(TensorShapeProto {
                        dim: vec![
                            tensor_shape_proto::Dimension {
                                value: Some(tensor_shape_proto::dimension::Value::DimValue(1)),
                            },
                            tensor_shape_proto::Dimension {
                                value: Some(tensor_shape_proto::dimension::Value::DimValue(3)),
                            },
                        ],
                    }),
                })),
            }),
        };

        let output_c = ValueInfoProto {
            name: "C".into(),
            r#type: Some(TypeProto {
                value: Some(type_proto::Value::TensorType(type_proto::Tensor {
                    elem_type: 1,
                    shape: Some(TensorShapeProto {
                        dim: vec![
                            tensor_shape_proto::Dimension {
                                value: Some(tensor_shape_proto::dimension::Value::DimValue(1)),
                            },
                            tensor_shape_proto::Dimension {
                                value: Some(tensor_shape_proto::dimension::Value::DimValue(3)),
                            },
                        ],
                    }),
                })),
            }),
        };

        let add_node = NodeProto {
            input: vec!["A".into(), "B".into()],
            output: vec!["C".into()],
            name: "add_0".into(),
            op_type: "Add".into(),
            domain: String::new(),
        };

        // Initializer for input B (3 float32 params)
        let init_b = TensorProto {
            dims: vec![1, 3],
            data_type: 1,
            name: "B".into(),
        };

        let graph = GraphProto {
            node: vec![add_node],
            name: "test_graph".into(),
            initializer: vec![init_b],
            input: vec![input_a, input_b],
            output: vec![output_c],
        };

        let model = ModelProto {
            ir_version: 8,
            opset_import: vec![OperatorSetIdProto {
                domain: String::new(),
                version: 17,
            }],
            producer_name: "test".into(),
            producer_version: "1.0".into(),
            domain: "test.domain".into(),
            model_version: 1,
            doc_string: "Test model".into(),
            graph: Some(graph),
        };

        model.encode_to_vec()
    }

    #[test]
    fn test_inspect_minimal_onnx() {
        let bytes = build_test_onnx_bytes();
        let info = inspect_onnx_bytes(&bytes).expect("inspect should succeed");

        assert_eq!(info.ir_version, 8);
        assert_eq!(info.opset_version, 17);
        assert_eq!(info.producer_name, "test");
        assert_eq!(info.producer_version, "1.0");
        assert_eq!(info.domain, "test.domain");
        assert_eq!(info.model_version, 1);
        assert_eq!(info.doc_string, "Test model");

        // 2 graph inputs (A and B)
        assert_eq!(info.inputs.len(), 2);
        assert_eq!(info.inputs[0].name, "A");
        assert_eq!(info.inputs[0].data_type, "float32");
        assert_eq!(info.inputs[0].shape, vec![1, 3]);
        assert_eq!(info.inputs[1].name, "B");

        // 1 output
        assert_eq!(info.outputs.len(), 1);
        assert_eq!(info.outputs[0].name, "C");
        assert_eq!(info.outputs[0].shape, vec![1, 3]);

        // 1 node
        assert_eq!(info.op_count, 1);
        assert_eq!(info.nodes.len(), 1);
        assert_eq!(info.nodes[0].op_type, "Add");
        assert_eq!(info.nodes[0].name, "add_0");
        assert_eq!(info.nodes[0].inputs, vec!["A", "B"]);
        assert_eq!(info.nodes[0].outputs, vec!["C"]);

        // param_count: initializer B has dims [1, 3] → 3 elements
        assert_eq!(info.param_count, 3);
    }

    #[test]
    fn test_inspect_file_roundtrip() {
        let bytes = build_test_onnx_bytes();
        let dir = std::env::temp_dir().join(format!(
            "videnoa_inspect_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test_model.onnx");
        std::fs::write(&path, &bytes).unwrap();

        let info = inspect_onnx(&path).expect("inspect file should succeed");
        assert_eq!(info.op_count, 1);
        assert_eq!(info.inputs.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_inspect_invalid_bytes() {
        let result = inspect_onnx_bytes(b"not a valid protobuf");
        assert!(result.is_err());
    }

    #[test]
    fn test_data_type_names() {
        assert_eq!(data_type_name(1), "float32");
        assert_eq!(data_type_name(2), "uint8");
        assert_eq!(data_type_name(3), "int8");
        assert_eq!(data_type_name(7), "int64");
        assert_eq!(data_type_name(10), "float16");
        assert_eq!(data_type_name(11), "float64");
        assert_eq!(data_type_name(16), "bfloat16");
        assert_eq!(data_type_name(999), "unknown(999)");
    }

    #[test]
    fn test_sanitize_filename_valid() {
        assert!(sanitize_model_filename("model.onnx").is_ok());
        assert!(sanitize_model_filename("my-model_v2.onnx").is_ok());
    }

    #[test]
    fn test_sanitize_filename_empty() {
        assert!(sanitize_model_filename("").is_err());
    }

    #[test]
    fn test_sanitize_filename_path_separator() {
        assert!(sanitize_model_filename("../etc/passwd").is_err());
        assert!(sanitize_model_filename("subdir/model.onnx").is_err());
        assert!(sanitize_model_filename("sub\\model.onnx").is_err());
    }

    #[test]
    fn test_sanitize_filename_dotdot() {
        assert!(sanitize_model_filename("..model.onnx").is_err());
        assert!(sanitize_model_filename("model..onnx").is_err());
    }

    #[test]
    fn test_dynamic_shape() {
        use onnx_proto::*;

        let vi = ValueInfoProto {
            name: "x".into(),
            r#type: Some(TypeProto {
                value: Some(type_proto::Value::TensorType(type_proto::Tensor {
                    elem_type: 1,
                    shape: Some(TensorShapeProto {
                        dim: vec![
                            tensor_shape_proto::Dimension {
                                value: Some(tensor_shape_proto::dimension::Value::DimParam(
                                    "batch".into(),
                                )),
                            },
                            tensor_shape_proto::Dimension {
                                value: Some(tensor_shape_proto::dimension::Value::DimValue(3)),
                            },
                        ],
                    }),
                })),
            }),
        };

        let ti = value_info_to_tensor(&vi);
        assert_eq!(ti.name, "x");
        assert_eq!(ti.data_type, "float32");
        assert_eq!(ti.shape, vec![-1, 3]); // batch dim → -1
    }
}
