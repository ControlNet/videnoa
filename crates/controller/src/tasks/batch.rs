use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::{
    FieldErrorCode, InputPath, OutputPath, TaskCreateRequest, TaskSource, WorkflowName,
};

use super::error::TaskApiError;
use super::intake::{validate_workflow_priority, TaskService};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BatchPreviewRequest {
    input_pattern: String,
    output_mode: OutputMode,
    output_directory: Option<String>,
    naming_mode: NamingMode,
    middle_extension: String,
    workflow: String,
    priority: i32,
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum OutputMode {
    BesideInput,
    Directory,
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum NamingMode {
    InsertExtension,
    Original,
}

#[derive(Serialize)]
pub(super) struct BatchPreview {
    items: Vec<BatchPreviewRow>,
}

#[derive(Serialize)]
struct BatchPreviewRow {
    request: TaskCreateRequest,
    error: Option<&'static str>,
}

impl TaskService {
    pub(super) async fn preview_batch(
        &self,
        request: BatchPreviewRequest,
    ) -> Result<BatchPreview, TaskApiError> {
        let service = self.clone();
        tokio::task::spawn_blocking(move || service.build_preview(&request))
            .await
            .map_err(|_| TaskApiError::Internal)?
    }

    fn build_preview(&self, options: &BatchPreviewRequest) -> Result<BatchPreview, TaskApiError> {
        validate_options(options)?;
        let inputs = self
            .paths
            .match_inputs(&options.input_pattern)
            .map_err(|message| invalid("input_pattern", message))?;
        let mut items = Vec::with_capacity(inputs.len());
        let mut outputs = HashMap::new();
        for input in inputs {
            let (mut output, naming_error) = output_path(&input, options);
            let mut request = TaskCreateRequest {
                input_path: InputPath::new(input.to_str().ok_or(TaskApiError::Internal)?),
                output_path: OutputPath::new(output.to_str().ok_or(TaskApiError::Internal)?),
                workflow: WorkflowName::new(&options.workflow),
                priority: options.priority,
                source: TaskSource::Manual,
                source_reference: None,
            };
            let mut error = naming_error;
            if error.is_none() && self.paths.check_preview_input(&input).is_err() {
                error = Some("Input changed or is no longer a safe, accessible regular file.");
            }
            match self.paths.open_output(&output) {
                Ok(checked) => {
                    output = checked.display_path().to_path_buf();
                    request.output_path =
                        OutputPath::new(output.to_str().ok_or(TaskApiError::Internal)?);
                    if checked.revalidate_missing().is_err() {
                        error = Some("Output changed or already exists.");
                    }
                }
                Err(crate::paths::PathError::OutputExists { .. }) => {
                    error = Some("Output already exists and will not be overwritten.");
                }
                Err(_) => error = Some("Output path is unsafe, private, or unavailable."),
            }
            let key = if cfg!(windows) {
                output.to_string_lossy().to_lowercase()
            } else {
                output.to_string_lossy().into_owned()
            };
            if let Some(previous) = outputs.insert(key, items.len()) {
                let previous: &mut BatchPreviewRow = &mut items[previous];
                previous.error = Some("Multiple inputs map to this output path.");
                error = previous.error;
            }
            items.push(BatchPreviewRow { request, error });
        }
        Ok(BatchPreview { items })
    }
}

fn invalid(field: &'static str, message: &'static str) -> TaskApiError {
    TaskApiError::invalid(field, FieldErrorCode::InvalidValue, message)
}

fn validate_options(options: &BatchPreviewRequest) -> Result<(), TaskApiError> {
    validate_workflow_priority(&options.workflow, options.priority)?;
    if options.output_mode == OutputMode::BesideInput && options.naming_mode == NamingMode::Original
    {
        return Err(invalid(
            "naming_mode",
            "Original filenames require a separate output directory.",
        ));
    }
    if options.output_mode == OutputMode::Directory
        && options
            .output_directory
            .as_ref()
            .map_or(true, String::is_empty)
    {
        return Err(invalid("output_directory", "Enter an output directory."));
    }
    if options.naming_mode == NamingMode::InsertExtension {
        let middle = &options.middle_extension;
        if middle.is_empty()
            || middle.len() > 64
            || middle.starts_with('.')
            || middle.ends_with(['.', ' '])
            || middle
                .chars()
                .any(|ch| ch.is_control() || "/\\:*?\"<>|".contains(ch))
        {
            return Err(invalid("middle_extension", "Enter 1 to 64 bytes without path separators, reserved filename characters, or leading/trailing dots."));
        }
    }
    Ok(())
}

fn output_path(input: &Path, options: &BatchPreviewRequest) -> (PathBuf, Option<&'static str>) {
    let directory = if options.output_mode == OutputMode::Directory {
        Path::new(options.output_directory.as_deref().unwrap_or_default())
    } else {
        input.parent().unwrap_or_else(|| Path::new(""))
    };
    let filename = input.file_name().unwrap_or_default();
    let Some(extension) = input.extension().filter(|extension| !extension.is_empty()) else {
        return (
            directory.join(filename),
            Some("Input filename must have an extension."),
        );
    };
    if options.naming_mode == NamingMode::Original {
        return (directory.join(filename), None);
    }
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    (
        directory.join(format!(
            "{stem}.{}.{}",
            options.middle_extension,
            extension.to_string_lossy()
        )),
        None,
    )
}
