//! The editor builds nodes from the descriptors; the engine runs them from the
//! nodes' own port definitions. The two must describe the same ports.
use std::collections::HashMap;

use videnoa_core::{
    descriptor::{all_node_descriptors, PortDescriptor},
    node::PortDefinition,
    registry::build_default_registry,
    types::PortType,
};

/// Ports that differ between descriptor and runtime on purpose, as
/// `(node_type, side, port)`.
const INTENTIONAL_DIFFERENCES: &[(&str, &str, &str)] = &[
    // The node's runtime inputs are the target's interface, read from
    // `interface_inputs`; `workflow_path` is its configuration, shown as a
    // workflow picker.
    ("Workflow", "input", "workflow_path"),
];

fn is_intentional(node_type: &str, side: &str, port: &str) -> bool {
    INTENTIONAL_DIFFERENCES.contains(&(node_type, side, port))
}

fn compare(
    node_type: &str,
    side: &str,
    runtime: &[PortDefinition],
    described: &[PortDescriptor],
    drift: &mut Vec<String>,
) {
    for d in described {
        // VideoFrames ports are topology markers the runtime does not declare.
        if d.port_type == "VideoFrames" || is_intentional(node_type, side, &d.name) {
            continue;
        }
        match runtime.iter().find(|p| p.name == d.name) {
            None => drift.push(format!(
                "{node_type} {side} '{}': in the descriptor ({}) but not at runtime",
                d.name, d.port_type
            )),
            Some(p) => {
                if format!("{:?}", p.port_type) != d.port_type {
                    drift.push(format!(
                        "{node_type} {side} '{}': type descriptor={} runtime={:?}",
                        d.name, d.port_type, p.port_type
                    ));
                }
                if p.required != d.required {
                    drift.push(format!(
                        "{node_type} {side} '{}': required descriptor={} runtime={}",
                        d.name, d.required, p.required
                    ));
                }
                if p.default_value != d.default_value {
                    drift.push(format!(
                        "{node_type} {side} '{}': default descriptor={:?} runtime={:?}",
                        d.name, d.default_value, p.default_value
                    ));
                }
            }
        }
    }
    for p in runtime {
        if p.port_type == PortType::VideoFrames || is_intentional(node_type, side, &p.name) {
            continue;
        }
        if !described.iter().any(|d| d.name == p.name) {
            drift.push(format!(
                "{node_type} {side} '{}': at runtime ({:?}) but not in the descriptor",
                p.name, p.port_type
            ));
        }
    }
}

#[test]
fn descriptor_ports_match_runtime_ports_for_every_registered_node() {
    let registry = build_default_registry();
    let descriptors = all_node_descriptors();
    let mut drift = Vec::new();

    for desc in &descriptors {
        let node = registry
            .create(&desc.node_type, HashMap::new())
            .unwrap_or_else(|e| panic!("{}: cannot instantiate: {e:#}", desc.node_type));
        compare(
            &desc.node_type,
            "input",
            &node.input_ports(),
            &desc.inputs,
            &mut drift,
        );
        compare(
            &desc.node_type,
            "output",
            &node.output_ports(),
            &desc.outputs,
            &mut drift,
        );
    }

    // ColorSpace and SceneDetect stay registered so old workflows parse, but
    // the editor no longer offers them.
    let hidden = ["ColorSpace", "SceneDetect"];
    for node_type in registry.list_node_types() {
        if !hidden.contains(&node_type) && !descriptors.iter().any(|d| d.node_type == node_type) {
            drift.push(format!("{node_type}: registered without a descriptor"));
        }
    }

    assert!(drift.is_empty(), "descriptor drift:\n{}", drift.join("\n"));
}
