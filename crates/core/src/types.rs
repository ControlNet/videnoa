use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Frame representation at different pipeline stages.
pub enum Frame {
    /// Raw CPU bytes from FFmpeg (RGB24 or RGB48).
    CpuRgb {
        data: Vec<u8>,
        width: u32,
        height: u32,
        bit_depth: u8,
    },
    /// Normalized float32 tensor ready for inference (NCHW).
    CpuTensor {
        data: Vec<f32>,
        channels: u32,
        height: u32,
        width: u32,
    },
    /// FP32 NCHW tensor on CPU — 3 channels, [0,255] range (Real-ESRGAN output).
    NchwF32 {
        data: Vec<f32>,
        height: u32,
        width: u32,
    },
    /// FP16 NCHW tensor on CPU — 3 channels, stored as raw u16 bits (half crate).
    /// Used for zero-copy pass-through between FP16 SuperRes and frame interpolation.
    NchwF16 {
        data: Vec<u16>,
        height: u32,
        width: u32,
    },
    // GpuTensor variant will be added later when ort is integrated into core.
}

/// Stream info for non-video streams.
pub struct StreamInfo {
    pub index: usize,
    pub codec_name: String,
    pub codec_type: String, // "audio", "subtitle", "attachment", "data"
    pub language: Option<String>,
    pub title: Option<String>,
    pub metadata: HashMap<String, String>,
}

/// Chapter marker.
pub struct Chapter {
    pub start_time: f64,
    pub end_time: f64,
    pub title: Option<String>,
}

/// Media metadata passthrough.
pub struct MediaMetadata {
    pub source_path: PathBuf,
    pub audio_streams: Vec<StreamInfo>,
    pub subtitle_streams: Vec<StreamInfo>,
    pub attachment_streams: Vec<StreamInfo>,
    pub chapters: Vec<Chapter>,
    pub global_metadata: HashMap<String, String>,
    pub container_format: String,
}

/// Port type identifier for connection validation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PortType {
    VideoFrames,
    Metadata,
    Model,
    Int,
    Float,
    Str,
    Bool,
    Path,
    WorkflowPath,
}

impl PortType {
    pub fn is_compatible(&self, other: &PortType) -> bool {
        self == other
    }
}

/// Data types that can flow between node ports.
pub enum PortData {
    // VideoFrames will be defined more concretely when executor is built.
    Metadata(MediaMetadata),
    // Model(Arc<ort::Session>), // Uncomment when ort is added to core deps.
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Path(PathBuf),
}

#[cfg(test)]
mod tests {
    use super::{Chapter, Frame, MediaMetadata, PortType, StreamInfo};
    use std::collections::HashMap;

    #[test]
    fn test_port_type_compatibility() {
        assert!(PortType::VideoFrames.is_compatible(&PortType::VideoFrames));
        assert!(!PortType::VideoFrames.is_compatible(&PortType::Metadata));
        assert!(!PortType::Int.is_compatible(&PortType::Float));
    }

    #[test]
    fn test_port_type_serde() {
        let port_type = PortType::Metadata;
        let json = serde_json::to_string(&port_type).expect("port type should serialize");
        let deserialized: PortType =
            serde_json::from_str(&json).expect("port type should deserialize");
        assert_eq!(port_type, deserialized);
    }

    #[test]
    fn test_media_metadata_creation() {
        let mut stream_metadata = HashMap::new();
        stream_metadata.insert("handler_name".to_string(), "English Audio".to_string());

        let audio_stream = StreamInfo {
            index: 1,
            codec_name: "aac".to_string(),
            codec_type: "audio".to_string(),
            language: Some("eng".to_string()),
            title: Some("Stereo".to_string()),
            metadata: stream_metadata,
        };

        let chapter = Chapter {
            start_time: 0.0,
            end_time: 60.0,
            title: Some("Intro".to_string()),
        };

        let mut global_metadata = HashMap::new();
        global_metadata.insert("title".to_string(), "Episode 01".to_string());

        let source_path = std::env::temp_dir().join("input.mkv");
        let media_metadata = MediaMetadata {
            source_path: source_path.clone(),
            audio_streams: vec![audio_stream],
            subtitle_streams: vec![],
            attachment_streams: vec![],
            chapters: vec![chapter],
            global_metadata,
            container_format: "matroska".to_string(),
        };

        assert_eq!(media_metadata.source_path, source_path);
        assert_eq!(media_metadata.audio_streams.len(), 1);
        assert_eq!(media_metadata.audio_streams[0].codec_name, "aac");
        assert_eq!(media_metadata.audio_streams[0].codec_type, "audio");
        assert_eq!(
            media_metadata.audio_streams[0].language.as_deref(),
            Some("eng")
        );
        assert_eq!(media_metadata.chapters.len(), 1);
        assert_eq!(media_metadata.chapters[0].title.as_deref(), Some("Intro"));
        assert_eq!(
            media_metadata
                .global_metadata
                .get("title")
                .map(String::as_str),
            Some("Episode 01")
        );
        assert_eq!(media_metadata.container_format, "matroska");
    }

    #[test]
    fn test_frame_nchw_variants() {
        let f32_frame = Frame::NchwF32 {
            data: vec![0.0f32; 3 * 4 * 4],
            height: 4,
            width: 4,
        };
        match &f32_frame {
            Frame::NchwF32 {
                data,
                height,
                width,
            } => {
                assert_eq!(data.len(), 3 * 4 * 4);
                assert_eq!(*height, 4);
                assert_eq!(*width, 4);
            }
            _ => panic!("expected NchwF32"),
        }

        let f16_frame = Frame::NchwF16 {
            data: vec![0u16; 3 * 8 * 8],
            height: 8,
            width: 8,
        };
        match &f16_frame {
            Frame::NchwF16 {
                data,
                height,
                width,
            } => {
                assert_eq!(data.len(), 3 * 8 * 8);
                assert_eq!(*height, 8);
                assert_eq!(*width, 8);
            }
            _ => panic!("expected NchwF16"),
        }
    }
}
