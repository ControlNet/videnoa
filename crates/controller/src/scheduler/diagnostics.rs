//! Safe, operation-specific diagnostics for local transfer failures.

use super::TransferError;
use crate::domain::{AttemptId, TaskId, TaskStatus};
use crate::paths::PathError;

#[derive(Debug)]
pub(super) struct OperationError {
    pub operation: &'static str,
    pub source: TransferError,
}

impl OperationError {
    pub fn new(operation: &'static str, source: impl Into<TransferError>) -> Self {
        Self {
            operation,
            source: source.into(),
        }
    }

    pub fn conflict(operation: &'static str) -> Self {
        Self::new(operation, TransferError::Conflict)
    }

    pub fn summary(&self) -> String {
        let detail = match &self.source {
            TransferError::Path(error) => match error {
                PathError::Io { source, .. } => io_summary(source),
                PathError::InvalidPath { .. } => "invalid_path".into(),
                PathError::OutsideRoots { .. } => "private_storage_boundary".into(),
                PathError::SymlinkComponent { .. } => "symlink_component".into(),
                PathError::RootChanged { .. } => "root_changed".into(),
                PathError::InputNotRegular { .. } => "not_regular_file".into(),
                PathError::InputChanged { .. } => "input_changed".into(),
                PathError::OutputExists { .. } => "output_exists_or_identity_changed".into(),
                PathError::OutputParentChanged { .. } => "output_parent_changed".into(),
                PathError::CrossFilesystemPublication { .. } => "cross_filesystem".into(),
            },
            TransferError::Io(error) => io_summary(error),
            TransferError::Conflict => "evidence_conflict".into(),
            // TransferError Display and remote error Display use controlled messages,
            // never raw HTTP responses, URLs, SQL, or nested custom error strings.
            TransferError::Remote(error) => error.to_string(),
            error => error.to_string(),
        };
        format!("{}: {detail}", self.operation)
    }

    pub fn log(&self, task_id: TaskId, attempt_id: Option<AttemptId>, stage: TaskStatus) {
        let io = match &self.source {
            TransferError::Io(error) | TransferError::Path(PathError::Io { source: error, .. }) => {
                Some(error)
            }
            _ => None,
        };
        tracing::warn!(%task_id, ?attempt_id, ?stage, operation = self.operation,
            io_kind = ?io.map(std::io::Error::kind),
            raw_os_error = ?io.and_then(std::io::Error::raw_os_error),
            reason = %self.summary(), "Task stage operation failed");
    }

    pub fn log_failure(&self, task_id: TaskId, attempt_id: AttemptId, stage: TaskStatus) {
        let io = match &self.source {
            TransferError::Io(error) | TransferError::Path(PathError::Io { source: error, .. }) => {
                Some(error)
            }
            _ => None,
        };
        tracing::error!(%task_id, %attempt_id, ?stage, operation = self.operation,
            io_kind = ?io.map(std::io::Error::kind),
            raw_os_error = ?io.and_then(std::io::Error::raw_os_error),
            reason = %self.summary(), "Task stage failed");
    }

    pub fn logged(
        self,
        task_id: TaskId,
        attempt_id: AttemptId,
        stage: TaskStatus,
    ) -> TransferError {
        self.log(task_id, Some(attempt_id), stage);
        self.source
    }
}

pub(super) trait Diagnose<T> {
    fn at(self, operation: &'static str) -> Result<T, OperationError>;
}

impl<T, E: Into<TransferError>> Diagnose<T> for Result<T, E> {
    fn at(self, operation: &'static str) -> Result<T, OperationError> {
        self.map_err(|error| OperationError::new(operation, error))
    }
}

fn io_summary(error: &std::io::Error) -> String {
    match error.raw_os_error() {
        Some(code) => format!(
            "io_kind={:?}, os_error={code} ({})",
            error.kind(),
            std::io::Error::from_raw_os_error(code)
        ),
        None => format!("io_kind={:?}", error.kind()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_nested_os_error_without_disclosing_path_or_custom_error_payload() {
        let diagnostic = OperationError::new(
            "copy.sync_destination_parent",
            PathError::Io {
                path: "private-path-sentinel".into(),
                source: std::io::Error::from_raw_os_error(13),
            },
        );
        let summary = diagnostic.summary();
        assert!(summary.contains("copy.sync_destination_parent"));
        assert!(summary.contains("os_error=13"));
        assert!(!summary.contains("private-path-sentinel"));
        let custom = OperationError::new(
            "copy.write",
            std::io::Error::other("private-error-sentinel"),
        );
        assert_eq!(custom.summary(), "copy.write: io_kind=Other");
    }
    #[derive(Clone, Default)]
    struct Capture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn failure_logs_show_operation_ids_and_os_cause_without_private_payloads() {
        let capture = Capture::default();
        let writer = capture.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_env_filter("warn,videnoa_controller=info")
            .with_writer(move || writer.clone())
            .finish();
        let task_id = TaskId::random();
        let attempt_id = AttemptId::random();
        tracing::subscriber::with_default(subscriber, || {
            OperationError::new(
                "copy.sync_destination_parent",
                PathError::Io {
                    path: "private-path-sentinel".into(),
                    source: std::io::Error::from_raw_os_error(13),
                },
            )
            .log_failure(task_id, attempt_id, TaskStatus::Publishing);
            OperationError::new(
                "cleanup.remove_local_workspace",
                std::io::Error::other("private-error-sentinel"),
            )
            .log(task_id, Some(attempt_id), TaskStatus::RemoteCleanup);
        });
        let log = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
        for expected in [
            "ERROR",
            "copy.sync_destination_parent",
            "os_error=13",
            "io_kind=",
            "WARN",
            "cleanup.remove_local_workspace",
            &task_id.to_string(),
            &attempt_id.to_string(),
        ] {
            assert!(log.contains(expected), "missing {expected}: {log}");
        }
        assert!(!log.contains("private-path-sentinel"));
        assert!(!log.contains("private-error-sentinel"));
    }
}
