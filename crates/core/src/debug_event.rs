use std::collections::HashMap;

use crate::types::PortData;

pub const PRINT_PREVIEW_MAX_CHARS: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeDebugValueEvent {
    pub node_id: String,
    pub node_type: String,
    pub value_preview: String,
    pub truncated: bool,
    pub preview_max_chars: usize,
}

pub type NodeDebugEventCallback<'a> = dyn FnMut(NodeDebugValueEvent) + Send + 'a;

pub fn build_print_debug_value_event(
    node_id: &str,
    node_type: &str,
    outputs: &HashMap<String, PortData>,
) -> Option<NodeDebugValueEvent> {
    if node_type != "Print" {
        return None;
    }

    let value = outputs.get("value")?;
    let (value_preview, truncated) = format_port_data_preview(value, PRINT_PREVIEW_MAX_CHARS);

    Some(NodeDebugValueEvent {
        node_id: node_id.to_string(),
        node_type: node_type.to_string(),
        value_preview,
        truncated,
        preview_max_chars: PRINT_PREVIEW_MAX_CHARS,
    })
}

pub fn format_port_data_preview(value: &PortData, max_chars: usize) -> (String, bool) {
    let raw = match value {
        PortData::Metadata(metadata) => format!(
            "MediaMetadata(source_path={}, audio_streams={}, subtitle_streams={}, attachment_streams={}, chapters={}, global_metadata={}, container_format={})",
            metadata.source_path.display(),
            metadata.audio_streams.len(),
            metadata.subtitle_streams.len(),
            metadata.attachment_streams.len(),
            metadata.chapters.len(),
            metadata.global_metadata.len(),
            metadata.container_format,
        ),
        PortData::Int(v) => v.to_string(),
        PortData::Float(v) => v.to_string(),
        PortData::Str(v) => v.clone(),
        PortData::Bool(v) => v.to_string(),
        PortData::Path(v) => v.display().to_string(),
    };

    truncate_preview(&raw, max_chars)
}

fn truncate_preview(value: &str, max_chars: usize) -> (String, bool) {
    let char_count = value.chars().count();
    if char_count <= max_chars {
        return (value.to_string(), false);
    }

    (value.chars().take(max_chars).collect(), true)
}
