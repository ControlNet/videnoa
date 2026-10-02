use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use anyhow::{Context, Result};

use crate::types::{Frame, PortData, PortType};

#[derive(Debug, Clone, PartialEq)]
pub struct PortDefinition {
    pub name: String,
    pub port_type: PortType,
    pub required: bool,
    pub default_value: Option<serde_json::Value>,
}

#[derive(Default)]
pub struct ExecutionContext {
    pub total_frames: Option<u64>,
    pub current_frame: u64,
    pub executing_workflows: HashSet<PathBuf>,
    pub nesting_depth: u32,
    pub cancellation: Option<tokio::sync::watch::Receiver<bool>>,
    /// Files this execution creates for itself. A nested execution must share
    /// the outer context's scratch so its files outlive the inner run.
    pub scratch: ExecutionScratch,
}

/// Per-execution directory for files nodes create, such as downloads.
///
/// The directory (`temp_dir()/videnoa/downloads/<uuid>`) is created on first
/// use and removed when the last clone is dropped, i.e. when the top-level
/// execution (one job, CLI run or preview) ends.
#[derive(Clone, Default)]
pub struct ExecutionScratch(Arc<ScratchState>);

#[derive(Default)]
struct ScratchState {
    root: OnceLock<PathBuf>,
    next_dir: AtomicU64,
}

impl ExecutionScratch {
    /// Creates a fresh, empty directory inside this execution's scratch space.
    pub fn allocate_dir(&self) -> Result<PathBuf> {
        let root = self.0.root.get_or_init(|| {
            std::env::temp_dir()
                .join("videnoa")
                .join("downloads")
                .join(uuid::Uuid::new_v4().to_string())
        });
        let dir = root.join(self.0.next_dir.fetch_add(1, Ordering::Relaxed).to_string());
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create scratch dir: {}", dir.display()))?;
        Ok(dir)
    }
}

impl ExecutionScratch {
    /// The scratch root, if anything has been allocated yet.
    #[cfg(test)]
    pub(crate) fn root(&self) -> Option<&std::path::Path> {
        self.0.root.get().map(PathBuf::as_path)
    }
}

impl Drop for ScratchState {
    fn drop(&mut self) {
        let Some(root) = self.root.get() else {
            return;
        };
        if let Err(error) = std::fs::remove_dir_all(root) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    path = %root.display(),
                    error = %error,
                    "failed to remove execution scratch dir"
                );
            }
        }
    }
}

impl ExecutionContext {
    pub fn check_cancelled(&self) -> Result<()> {
        if self.cancellation.as_ref().is_some_and(|rx| *rx.borrow()) {
            anyhow::bail!("workflow execution cancelled");
        }
        Ok(())
    }

    pub fn progress(&self) -> Option<f32> {
        let total = self.total_frames?;
        if total == 0 {
            return Some(0.0);
        }

        Some((self.current_frame as f32 / total as f32).clamp(0.0, 1.0))
    }
}

/// Core node trait that all nodes implement.
pub trait Node: Send + Sync {
    fn node_type(&self) -> &str;
    fn input_ports(&self) -> Vec<PortDefinition>;
    fn output_ports(&self) -> Vec<PortDefinition>;
    fn execute(
        &mut self,
        inputs: &HashMap<String, PortData>,
        ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>>;
}

/// Sub-trait for nodes that process frames one-at-a-time.
pub trait FrameProcessor: Node {
    fn process_frame(&mut self, frame: Frame, ctx: &ExecutionContext) -> Result<Frame>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_definition_creation() {
        let input = PortDefinition {
            name: "input".to_string(),
            port_type: PortType::VideoFrames,
            required: true,
            default_value: None,
        };

        let output = PortDefinition {
            name: "strength".to_string(),
            port_type: PortType::Float,
            required: false,
            default_value: Some(serde_json::json!(1.0)),
        };

        assert_eq!(input.name, "input");
        assert_eq!(input.port_type, PortType::VideoFrames);
        assert!(input.required);
        assert!(input.default_value.is_none());

        assert_eq!(output.name, "strength");
        assert_eq!(output.port_type, PortType::Float);
        assert!(!output.required);
        assert_eq!(output.default_value, Some(serde_json::json!(1.0)));
    }
}
