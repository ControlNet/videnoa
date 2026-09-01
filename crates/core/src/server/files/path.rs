use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use super::super::AppError;

#[derive(Clone, Copy)]
pub(super) enum PathRequirement {
    Existing,
    AllowMissing,
}

pub(super) struct ResolvedWorkspacePath {
    pub(super) absolute: PathBuf,
    pub(super) relative: String,
}

struct ParsedWorkspacePath {
    path: PathBuf,
    display: String,
}

pub(super) async fn resolve_workspace_path(
    workspace_root: &Path,
    relative_path: &str,
    requirement: PathRequirement,
) -> Result<ResolvedWorkspacePath, AppError> {
    let parsed = parse_workspace_relative_path(relative_path)?;
    let root_metadata = tokio::fs::symlink_metadata(workspace_root)
        .await
        .map_err(|error| workspace_io_error("inspect workspace root", workspace_root, error))?;
    if root_metadata.file_type().is_symlink() {
        return Err(AppError::BadRequest(
            "workspace root must not be a symlink".to_string(),
        ));
    }
    if !root_metadata.is_dir() {
        return Err(AppError::Internal(format!(
            "workspace root is not a directory: {}",
            workspace_root.display()
        )));
    }

    let canonical_root = tokio::fs::canonicalize(workspace_root)
        .await
        .map_err(|error| {
            workspace_io_error("canonicalize workspace root", workspace_root, error)
        })?;
    let component_count = parsed.path.components().count();
    let mut candidate = canonical_root.clone();
    let mut existing_ancestor = canonical_root.clone();
    let mut missing_component = false;

    for (index, component) in parsed.path.components().enumerate() {
        candidate.push(component.as_os_str());
        if missing_component {
            continue;
        }

        match tokio::fs::symlink_metadata(&candidate).await {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(AppError::BadRequest(format!(
                        "symlinks are not allowed in file API paths: {}",
                        parsed.display
                    )));
                }
                if index + 1 < component_count && !metadata.is_dir() {
                    return Err(AppError::BadRequest(format!(
                        "path component is not a directory: {}",
                        candidate.display()
                    )));
                }
                existing_ancestor = candidate.clone();
            }
            Err(error) if error.kind() == ErrorKind::NotFound => match requirement {
                PathRequirement::Existing => {
                    return Err(AppError::NotFound(format!(
                        "path not found: {}",
                        parsed.display
                    )));
                }
                PathRequirement::AllowMissing => missing_component = true,
            },
            Err(error) => {
                return Err(workspace_io_error(
                    "inspect workspace path",
                    &candidate,
                    error,
                ));
            }
        }
    }

    let canonical_existing =
        tokio::fs::canonicalize(&existing_ancestor)
            .await
            .map_err(|error| {
                workspace_io_error("canonicalize workspace path", &existing_ancestor, error)
            })?;
    if !canonical_existing.starts_with(&canonical_root) {
        return Err(AppError::BadRequest(
            "path resolves outside the workspace".to_string(),
        ));
    }

    let absolute = if missing_component {
        canonical_root.join(&parsed.path)
    } else {
        let canonical_target = tokio::fs::canonicalize(&candidate).await.map_err(|error| {
            workspace_io_error("canonicalize workspace target", &candidate, error)
        })?;
        if !canonical_target.starts_with(&canonical_root) {
            return Err(AppError::BadRequest(
                "path resolves outside the workspace".to_string(),
            ));
        }
        canonical_target
    };

    Ok(ResolvedWorkspacePath {
        absolute,
        relative: parsed.display,
    })
}

fn parse_workspace_relative_path(relative_path: &str) -> Result<ParsedWorkspacePath, AppError> {
    if relative_path.is_empty() {
        return Err(AppError::BadRequest(
            "workspace-relative path is required".to_string(),
        ));
    }
    if relative_path.starts_with('/')
        || relative_path.starts_with('\\')
        || relative_path.contains('\\')
        || has_windows_drive_prefix(relative_path)
    {
        return Err(AppError::BadRequest(
            "absolute paths are not allowed".to_string(),
        ));
    }

    let mut path = PathBuf::new();
    let mut display_components = Vec::new();
    for component in Path::new(relative_path).components() {
        match component {
            Component::Normal(value) => {
                let value = value
                    .to_str()
                    .ok_or_else(|| AppError::BadRequest("path must be valid UTF-8".to_string()))?;
                if display_components.is_empty() && value.starts_with('~') {
                    return Err(AppError::BadRequest(
                        "home-relative paths are not allowed".to_string(),
                    ));
                }
                path.push(value);
                display_components.push(value.to_string());
            }
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                return Err(AppError::BadRequest(
                    "path traversal is not allowed".to_string(),
                ));
            }
        }
    }
    if display_components.is_empty() {
        return Err(AppError::BadRequest(
            "workspace-relative path is required".to_string(),
        ));
    }

    Ok(ParsedWorkspacePath {
        path,
        display: display_components.join("/"),
    })
}

fn has_windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn workspace_io_error(action: &str, path: &Path, error: std::io::Error) -> AppError {
    if error.kind() == ErrorKind::NotFound {
        AppError::NotFound(format!("path not found: {}", path.display()))
    } else {
        AppError::Internal(format!("failed to {action} {}: {error}", path.display()))
    }
}
