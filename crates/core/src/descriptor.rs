//! Node descriptors: static metadata for all registered node types.
//!
//! These descriptors provide display names, categories, colors, icons,
//! and full port definitions (both stream and param) for the frontend
//! node editor. They are a **separate data path** from the runtime
//! `Node::input_ports()`/`output_ports()` — the runtime trait is
//! unchanged.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct NodeDescriptor {
    pub node_type: String,
    pub display_name: String,
    /// "input", "processing", "output", "utility"
    pub category: String,
    /// Hex color, e.g. "#F97316"
    pub accent_color: String,
    /// Icon name, e.g. "microscope", "film"
    pub icon: String,
    pub inputs: Vec<PortDescriptor>,
    pub outputs: Vec<PortDescriptor>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortDescriptor {
    pub name: String,
    /// "VideoFrames", "Metadata", "Int", "Str", etc.
    pub port_type: String,
    /// "stream" or "param"
    pub direction: String,
    pub required: bool,
    pub default_value: Option<serde_json::Value>,
    /// "model_selector", "enum", "path_picker", etc.
    pub ui_hint: Option<String>,
    pub enum_options: Option<Vec<String>>,
    /// For future Constant node
    pub dynamic_type_param: Option<String>,
}

/// Helper to build a stream port descriptor.
fn stream(name: &str, port_type: &str) -> PortDescriptor {
    PortDescriptor {
        name: name.to_string(),
        port_type: port_type.to_string(),
        direction: "stream".to_string(),
        required: true,
        default_value: None,
        ui_hint: None,
        enum_options: None,
        dynamic_type_param: None,
    }
}

/// Helper to build a required param port descriptor.
fn param_required(name: &str, port_type: &str) -> PortDescriptor {
    PortDescriptor {
        name: name.to_string(),
        port_type: port_type.to_string(),
        direction: "param".to_string(),
        required: true,
        default_value: None,
        ui_hint: None,
        enum_options: None,
        dynamic_type_param: None,
    }
}

/// Helper to build an optional param port descriptor with a default value.
fn param_opt(name: &str, port_type: &str, default: serde_json::Value) -> PortDescriptor {
    PortDescriptor {
        name: name.to_string(),
        port_type: port_type.to_string(),
        direction: "param".to_string(),
        required: false,
        default_value: Some(default),
        ui_hint: None,
        enum_options: None,
        dynamic_type_param: None,
    }
}

/// Returns descriptors for all registered node types.
///
/// Port data is hardcoded to match the runtime `Node` implementations.
/// Stream ports (VideoFrames in/out) are listed separately from param
/// ports (which correspond to `Node::input_ports()` / `output_ports()`).
pub fn all_node_descriptors() -> Vec<NodeDescriptor> {
    vec![
        // ---------------------------------------------------------------
        // 1. VideoInput
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "VideoInput".to_string(),
            display_name: "Video Input".to_string(),
            category: "input".to_string(),
            accent_color: "#A855F7".to_string(),
            icon: "file-video".to_string(),
            inputs: vec![
                // param: from VideoInputNode::input_ports()
                param_required("path", "Path"),
            ],
            outputs: vec![
                // stream
                stream("frames", "VideoFrames"),
                // param: from VideoInputNode::output_ports()
                stream("metadata", "Metadata"),
                PortDescriptor {
                    direction: "param".to_string(),
                    ..param_required("source_path", "Path")
                },
            ],
        },
        // ---------------------------------------------------------------
        // 2. SuperResolution
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "SuperResolution".to_string(),
            display_name: "Super Resolution".to_string(),
            category: "processing".to_string(),
            accent_color: "#F97316".to_string(),
            icon: "microscope".to_string(),
            inputs: vec![
                // stream
                stream("frames", "VideoFrames"),
                // param: from SuperResNode::input_ports()
                PortDescriptor {
                    ui_hint: Some("model_selector".to_string()),
                    ..param_required("model_path", "Path")
                },
                param_opt("scale", "Int", serde_json::json!(4)),
                param_opt("tile_size", "Int", serde_json::json!(0)),
                param_opt("device_id", "Int", serde_json::json!(0)),
                PortDescriptor {
                    enum_options: Some(vec!["cuda".to_string(), "tensorrt".to_string()]),
                    ..param_opt("backend", "Str", serde_json::json!("cuda"))
                },
            ],
            outputs: vec![
                // stream
                stream("frames", "VideoFrames"),
            ],
        },
        // ---------------------------------------------------------------
        // 3. FrameInterpolation
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "FrameInterpolation".to_string(),
            display_name: "Frame Interpolation".to_string(),
            category: "processing".to_string(),
            accent_color: "#06B6D4".to_string(),
            icon: "film".to_string(),
            inputs: vec![
                // stream
                stream("frames", "VideoFrames"),
                // param: from FrameInterpolationNode::input_ports()
                PortDescriptor {
                    ui_hint: Some("model_selector".to_string()),
                    ..param_required("model_path", "Path")
                },
                param_opt("multiplier", "Int", serde_json::json!(2)),
                param_opt("device_id", "Int", serde_json::json!(0)),
                PortDescriptor {
                    enum_options: Some(vec!["cuda".to_string(), "tensorrt".to_string()]),
                    ..param_opt("backend", "Str", serde_json::json!("cuda"))
                },
            ],
            outputs: vec![stream("frames", "VideoFrames")],
        },
        // ---------------------------------------------------------------
        // 4. VideoOutput
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "VideoOutput".to_string(),
            display_name: "Video Output".to_string(),
            category: "output".to_string(),
            accent_color: "#10B981".to_string(),
            icon: "hard-drive".to_string(),
            inputs: vec![
                // stream
                stream("frames", "VideoFrames"),
                // param: from VideoOutputNode::input_ports()
                param_required("source_path", "Path"),
                param_required("output_path", "Path"),
                PortDescriptor {
                    enum_options: Some(vec!["libx265".to_string(), "libx264".to_string()]),
                    ..param_opt("codec", "Str", serde_json::json!("libx265"))
                },
                param_opt("crf", "Int", serde_json::json!(18)),
                PortDescriptor {
                    enum_options: Some(vec!["yuv420p10le".to_string(), "yuv420p".to_string()]),
                    ..param_opt("pixel_format", "Str", serde_json::json!("yuv420p10le"))
                },
                param_required("width", "Int"),
                param_required("height", "Int"),
                param_required("fps", "Str"),
            ],
            outputs: vec![
                // param: from VideoOutputNode::output_ports()
                PortDescriptor {
                    direction: "param".to_string(),
                    ..param_required("output_path", "Path")
                },
            ],
        },
        // ---------------------------------------------------------------
        // 5. Resize
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "Resize".to_string(),
            display_name: "Resize".to_string(),
            category: "processing".to_string(),
            accent_color: "#3B82F6".to_string(),
            icon: "scaling".to_string(),
            inputs: vec![
                // stream
                stream("frames", "VideoFrames"),
                // param: from ResizeNode::input_ports()
                param_required("width", "Int"),
                param_required("height", "Int"),
                PortDescriptor {
                    enum_options: Some(vec!["bilinear".to_string(), "nearest".to_string()]),
                    ..param_opt("algorithm", "Str", serde_json::json!("bilinear"))
                },
            ],
            outputs: vec![stream("frames", "VideoFrames")],
        },
        NodeDescriptor {
            node_type: "Rescale".to_string(),
            display_name: "Rescale".to_string(),
            category: "processing".to_string(),
            accent_color: "#3B82F6".to_string(),
            icon: "scaling".to_string(),
            inputs: vec![
                stream("frames", "VideoFrames"),
                param_required("scale_factor", "Float"),
                PortDescriptor {
                    enum_options: Some(vec!["bilinear".to_string(), "nearest".to_string()]),
                    ..param_opt("algorithm", "Str", serde_json::json!("bilinear"))
                },
            ],
            outputs: vec![stream("frames", "VideoFrames")],
        },
        // ---------------------------------------------------------------
        // 6. ColorSpace
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "ColorSpace".to_string(),
            display_name: "Color Space".to_string(),
            category: "processing".to_string(),
            accent_color: "#EAB308".to_string(),
            icon: "palette".to_string(),
            inputs: vec![
                // param-only node: from ColorSpaceNode::input_ports()
                PortDescriptor {
                    enum_options: Some(vec![
                        "bt709".to_string(),
                        "bt601".to_string(),
                        "bt2020".to_string(),
                    ]),
                    ..param_opt("matrix", "Str", serde_json::json!("bt709"))
                },
                PortDescriptor {
                    enum_options: Some(vec!["limited".to_string(), "full".to_string()]),
                    ..param_opt("range", "Str", serde_json::json!("limited"))
                },
                param_opt("transfer", "Str", serde_json::json!("bt709")),
                param_opt("primaries", "Str", serde_json::json!("bt709")),
                param_opt("dither", "Str", serde_json::json!("error_diffusion")),
            ],
            outputs: vec![
                // param: from ColorSpaceNode::output_ports()
                PortDescriptor {
                    direction: "param".to_string(),
                    ..param_required("config", "Str")
                },
            ],
        },
        // ---------------------------------------------------------------
        // 7. SceneDetect
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "SceneDetect".to_string(),
            display_name: "Scene Detect".to_string(),
            category: "processing".to_string(),
            accent_color: "#EF4444".to_string(),
            icon: "scissors".to_string(),
            inputs: vec![
                // stream
                stream("frames", "VideoFrames"),
                // param: from SceneDetectNode::input_ports()
                param_opt("threshold", "Float", serde_json::json!(0.3)),
            ],
            outputs: vec![
                // stream
                stream("frames", "VideoFrames"),
                // param: from SceneDetectNode::output_ports()
                PortDescriptor {
                    direction: "param".to_string(),
                    ..param_required("is_scene_change", "Bool")
                },
            ],
        },
        // ---------------------------------------------------------------
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "Downloader".to_string(),
            display_name: "Downloader".to_string(),
            category: "input".to_string(),
            accent_color: "#A855F7".to_string(),
            icon: "download".to_string(),
            inputs: vec![param_required("url", "Str")],
            outputs: vec![PortDescriptor {
                direction: "param".to_string(),
                ..param_required("path", "Path")
            }],
        },
        // ---------------------------------------------------------------
        // 9. StreamOutput
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "StreamOutput".to_string(),
            display_name: "Stream Output".to_string(),
            category: "output".to_string(),
            accent_color: "#10B981".to_string(),
            icon: "radio".to_string(),
            inputs: vec![
                // param: from StreamOutputNode::input_ports()
                param_required("url", "Str"),
                PortDescriptor {
                    enum_options: Some(vec!["libx265".to_string(), "libx264".to_string()]),
                    ..param_opt("codec", "Str", serde_json::json!("libx264"))
                },
                param_opt("bitrate", "Str", serde_json::json!("5M")),
                PortDescriptor {
                    enum_options: Some(vec![
                        "flv".to_string(),
                        "mpegts".to_string(),
                        "rtsp".to_string(),
                    ]),
                    ..param_opt("format", "Str", serde_json::json!("flv"))
                },
                param_opt("source_url", "Str", serde_json::json!("")),
            ],
            outputs: vec![
                // param: from StreamOutputNode::output_ports()
                PortDescriptor {
                    direction: "param".to_string(),
                    ..param_required("output_url", "Str")
                },
            ],
        },
        // ---------------------------------------------------------------
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "JellyfinVideo".to_string(),
            display_name: "Jellyfin Video".to_string(),
            category: "input".to_string(),
            accent_color: "#A855F7".to_string(),
            icon: "tv".to_string(),
            inputs: vec![
                param_required("jellyfin_url", "Str"),
                param_required("api_key", "Str"),
                param_required("item_id", "Str"),
            ],
            outputs: vec![PortDescriptor {
                direction: "param".to_string(),
                ..param_required("video_url", "Str")
            }],
        },
        // ---------------------------------------------------------------
        // 11. Constant
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "Constant".to_string(),
            display_name: "Constant".to_string(),
            category: "utility".to_string(),
            accent_color: "#6366F1".to_string(),
            icon: "hash".to_string(),
            inputs: vec![
                PortDescriptor {
                    enum_options: Some(vec![
                        "Int".to_string(),
                        "Float".to_string(),
                        "Str".to_string(),
                        "Bool".to_string(),
                        "Path".to_string(),
                    ]),
                    ..param_opt("type", "Str", serde_json::json!("Int"))
                },
                param_opt("value", "Str", serde_json::json!("0")),
            ],
            outputs: vec![PortDescriptor {
                dynamic_type_param: Some("type".to_string()),
                ..param_required("value", "Int")
            }],
        },
        // ---------------------------------------------------------------
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "Print".to_string(),
            display_name: "Print".to_string(),
            category: "utility".to_string(),
            accent_color: "#6366F1".to_string(),
            icon: "hash".to_string(),
            inputs: vec![
                PortDescriptor {
                    enum_options: Some(vec![
                        "Int".to_string(),
                        "Float".to_string(),
                        "Str".to_string(),
                        "Bool".to_string(),
                        "Path".to_string(),
                    ]),
                    ..param_opt("value_type", "Str", serde_json::json!("Str"))
                },
                PortDescriptor {
                    dynamic_type_param: Some("value_type".to_string()),
                    ..param_required("value", "Str")
                },
            ],
            outputs: vec![PortDescriptor {
                dynamic_type_param: Some("value_type".to_string()),
                ..param_required("value", "Str")
            }],
        },
        // ---------------------------------------------------------------
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "PathDivider".to_string(),
            display_name: "Path Divider".to_string(),
            category: "utility".to_string(),
            accent_color: "#6366F1".to_string(),
            icon: "split".to_string(),
            inputs: vec![param_required("path", "Path")],
            outputs: vec![
                PortDescriptor {
                    direction: "param".to_string(),
                    ..param_required("parent_path", "Path")
                },
                PortDescriptor {
                    direction: "param".to_string(),
                    ..param_required("file_name", "Str")
                },
                PortDescriptor {
                    direction: "param".to_string(),
                    ..param_required("file_stem", "Str")
                },
                PortDescriptor {
                    direction: "param".to_string(),
                    ..param_required("file_extension", "Str")
                },
            ],
        },
        // ---------------------------------------------------------------
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "PathJoiner".to_string(),
            display_name: "Path Joiner".to_string(),
            category: "utility".to_string(),
            accent_color: "#6366F1".to_string(),
            icon: "split".to_string(),
            inputs: vec![
                param_required("parent_path", "Path"),
                param_opt("sub_path", "Path", serde_json::json!("")),
                param_opt("file_name", "Str", serde_json::json!("")),
            ],
            outputs: vec![PortDescriptor {
                direction: "param".to_string(),
                ..param_required("path", "Path")
            }],
        },
        // ---------------------------------------------------------------
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "StringTemplate".to_string(),
            display_name: "String Template".to_string(),
            category: "utility".to_string(),
            accent_color: "#6366F1".to_string(),
            icon: "braces".to_string(),
            inputs: vec![
                param_opt("num_input", "Int", serde_json::json!(0)),
                param_opt("template", "Str", serde_json::json!("")),
                param_opt("strict", "Bool", serde_json::json!(true)),
            ],
            outputs: vec![PortDescriptor {
                direction: "param".to_string(),
                ..param_required("value", "Str")
            }],
        },
        // ---------------------------------------------------------------
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "StringReplace".to_string(),
            display_name: "String Replace".to_string(),
            category: "utility".to_string(),
            accent_color: "#6366F1".to_string(),
            icon: "replace".to_string(),
            inputs: vec![
                param_required("input", "Str"),
                param_required("old", "Str"),
                param_required("new", "Str"),
            ],
            outputs: vec![PortDescriptor {
                direction: "param".to_string(),
                ..param_required("output", "Str")
            }],
        },
        // ---------------------------------------------------------------
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "TypeConversion".to_string(),
            display_name: "Type Conversion".to_string(),
            category: "utility".to_string(),
            accent_color: "#6366F1".to_string(),
            icon: "arrow-left-right".to_string(),
            inputs: vec![
                PortDescriptor {
                    enum_options: Some(vec![
                        "Int".to_string(),
                        "Float".to_string(),
                        "Str".to_string(),
                        "Bool".to_string(),
                        "Path".to_string(),
                    ]),
                    ..param_opt("input_type", "Str", serde_json::json!("Int"))
                },
                PortDescriptor {
                    enum_options: Some(vec![
                        "Int".to_string(),
                        "Float".to_string(),
                        "Str".to_string(),
                        "Bool".to_string(),
                        "Path".to_string(),
                    ]),
                    ..param_opt("output_type", "Str", serde_json::json!("Int"))
                },
                PortDescriptor {
                    dynamic_type_param: Some("input_type".to_string()),
                    ..param_required("value", "Int")
                },
            ],
            outputs: vec![PortDescriptor {
                direction: "param".to_string(),
                dynamic_type_param: Some("output_type".to_string()),
                ..param_required("value", "Int")
            }],
        },
        // ---------------------------------------------------------------
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "HttpRequest".to_string(),
            display_name: "HTTP Request".to_string(),
            category: "utility".to_string(),
            accent_color: "#6366F1".to_string(),
            icon: "globe".to_string(),
            inputs: vec![
                param_opt("method", "Str", serde_json::json!("GET")),
                param_required("url", "Str"),
                param_opt("headers_json", "Str", serde_json::json!("{}")),
                param_opt("body", "Str", serde_json::json!("")),
                param_opt("timeout_ms", "Int", serde_json::json!(30000)),
                param_opt("max_retries", "Int", serde_json::json!(2)),
                param_opt("retry_backoff_ms", "Int", serde_json::json!(250)),
                param_opt("max_response_bytes", "Int", serde_json::json!(1048576)),
            ],
            outputs: vec![
                PortDescriptor {
                    direction: "param".to_string(),
                    ..param_required("status_code", "Int")
                },
                PortDescriptor {
                    direction: "param".to_string(),
                    ..param_required("ok", "Bool")
                },
                PortDescriptor {
                    direction: "param".to_string(),
                    ..param_required("response_body", "Str")
                },
                PortDescriptor {
                    direction: "param".to_string(),
                    ..param_required("response_url", "Str")
                },
                PortDescriptor {
                    direction: "param".to_string(),
                    ..param_required("content_type", "Str")
                },
            ],
        },
        // ---------------------------------------------------------------
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "WorkflowInput".to_string(),
            display_name: "Workflow Input".to_string(),
            category: "workflow".to_string(),
            accent_color: "#EAB308".to_string(),
            icon: "arrow-down-to-line".to_string(),
            inputs: vec![],
            outputs: vec![],
        },
        // ---------------------------------------------------------------
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "WorkflowOutput".to_string(),
            display_name: "Workflow Output".to_string(),
            category: "workflow".to_string(),
            accent_color: "#EAB308".to_string(),
            icon: "arrow-up-from-line".to_string(),
            inputs: vec![],
            outputs: vec![],
        },
        // ---------------------------------------------------------------
        // ---------------------------------------------------------------
        NodeDescriptor {
            node_type: "Workflow".to_string(),
            display_name: "Workflow".to_string(),
            category: "workflow".to_string(),
            accent_color: "#EAB308".to_string(),
            icon: "workflow".to_string(),
            inputs: vec![PortDescriptor {
                ui_hint: Some("workflow_picker".to_string()),
                ..param_required("workflow_path", "WorkflowPath")
            }],
            outputs: vec![],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_node_descriptors_count() {
        let descs = all_node_descriptors();
        assert_eq!(descs.len(), 22);
    }

    #[test]
    fn test_all_node_types_unique() {
        let descs = all_node_descriptors();
        let mut types: Vec<&str> = descs.iter().map(|d| d.node_type.as_str()).collect();
        types.sort();
        types.dedup();
        assert_eq!(types.len(), 22);
    }

    #[test]
    fn test_path_joiner_descriptor_ports() {
        let descs = all_node_descriptors();
        let path_joiner = descs
            .iter()
            .find(|d| d.node_type == "PathJoiner")
            .expect("PathJoiner descriptor should exist");

        assert_eq!(path_joiner.inputs.len(), 3);
        let parent = path_joiner
            .inputs
            .iter()
            .find(|p| p.name == "parent_path")
            .expect("parent_path input should exist");
        assert!(parent.required);

        let sub = path_joiner
            .inputs
            .iter()
            .find(|p| p.name == "sub_path")
            .expect("sub_path input should exist");
        assert!(!sub.required);
        assert_eq!(sub.default_value, Some(serde_json::json!("")));

        let file_name = path_joiner
            .inputs
            .iter()
            .find(|p| p.name == "file_name")
            .expect("file_name input should exist");
        assert!(!file_name.required);
        assert_eq!(file_name.default_value, Some(serde_json::json!("")));

        assert_eq!(path_joiner.outputs.len(), 1);
        assert_eq!(path_joiner.outputs[0].name, "path");
        assert_eq!(path_joiner.outputs[0].port_type, "Path");
    }

    #[test]
    fn test_descriptors_serialize() {
        let descs = all_node_descriptors();
        let json = serde_json::to_string(&descs).expect("should serialize");
        assert!(json.contains("VideoInput"));
        assert!(json.contains("SuperResolution"));
    }

    #[test]
    fn test_video_input_descriptor() {
        let descs = all_node_descriptors();
        let vi = descs.iter().find(|d| d.node_type == "VideoInput").unwrap();
        assert_eq!(vi.display_name, "Video Input");
        assert_eq!(vi.category, "input");
        assert_eq!(vi.inputs.len(), 1);
        assert_eq!(vi.outputs.len(), 3);
    }

    #[test]
    fn test_super_res_descriptor() {
        let descs = all_node_descriptors();
        let sr = descs
            .iter()
            .find(|d| d.node_type == "SuperResolution")
            .unwrap();
        assert_eq!(sr.display_name, "Super Resolution");
        assert_eq!(sr.category, "processing");
        assert_eq!(sr.inputs.len(), 6);
        assert_eq!(sr.outputs.len(), 1);
        let backend = sr.inputs.iter().find(|p| p.name == "backend").unwrap();
        assert!(backend.enum_options.is_some());
    }

    #[test]
    fn test_downloader_descriptor_output_port() {
        let descs = all_node_descriptors();
        let downloader = descs
            .iter()
            .find(|d| d.node_type == "Downloader")
            .expect("downloader descriptor should exist");

        assert_eq!(downloader.outputs.len(), 1);
        let output = &downloader.outputs[0];
        assert_eq!(output.name, "path");
        assert_eq!(output.port_type, "Path");
        assert_eq!(output.direction, "param");
        assert!(output.required);
    }

    #[test]
    fn test_directions_valid() {
        let descs = all_node_descriptors();
        for desc in &descs {
            for port in desc.inputs.iter().chain(desc.outputs.iter()) {
                assert!(
                    port.direction == "stream" || port.direction == "param",
                    "invalid direction '{}' on port '{}' of node '{}'",
                    port.direction,
                    port.name,
                    desc.node_type
                );
            }
        }
    }
}
