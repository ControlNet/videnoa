use std::io::ErrorKind;

use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::TryStreamExt;
use serde::Serialize;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio_util::io::{ReaderStream, StreamReader};

use super::{AppError, AppState};

mod path;

use path::{resolve_workspace_path, PathRequirement};

#[derive(Serialize)]
pub(super) struct UploadResponse {
    path: String,
    size: u64,
}

#[derive(Serialize)]
pub(super) struct FileStatResponse {
    path: String,
    size: u64,
    is_file: bool,
    is_dir: bool,
}

pub(super) async fn upload_file(
    State(state): State<AppState>,
    Path(relative_path): Path<String>,
    request: Request,
) -> Result<Json<UploadResponse>, AppError> {
    let workspace_root = &state.inner.workspace_root;
    let resolved = resolve_workspace_path(
        workspace_root,
        &relative_path,
        PathRequirement::AllowMissing,
    )
    .await?;
    let parent = resolved.absolute.parent().ok_or_else(|| {
        AppError::BadRequest("file path must have a parent directory".to_string())
    })?;

    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|error| io_error("create upload parent", parent, error))?;

    let resolved = resolve_workspace_path(
        workspace_root,
        &relative_path,
        PathRequirement::AllowMissing,
    )
    .await?;
    match tokio::fs::symlink_metadata(&resolved.absolute).await {
        Ok(metadata) if metadata.is_dir() => {
            return Err(AppError::BadRequest(format!(
                "upload path is a directory: {}",
                resolved.relative
            )));
        }
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(io_error("inspect upload target", &resolved.absolute, error));
        }
    }

    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let file = options
        .open(&resolved.absolute)
        .await
        .map_err(|error| io_error("open upload target", &resolved.absolute, error))?;

    let stream = request
        .into_body()
        .into_data_stream()
        .map_err(std::io::Error::other);
    let mut reader = StreamReader::new(stream);
    let mut writer = BufWriter::new(file);
    let size = tokio::io::copy(&mut reader, &mut writer)
        .await
        .map_err(|error| io_error("stream upload", &resolved.absolute, error))?;
    writer
        .flush()
        .await
        .map_err(|error| io_error("flush upload", &resolved.absolute, error))?;

    Ok(Json(UploadResponse {
        path: resolved.relative,
        size,
    }))
}

pub(super) async fn get_file_or_stat(
    State(state): State<AppState>,
    Path(relative_path): Path<String>,
) -> Result<Response, AppError> {
    let workspace_root = &state.inner.workspace_root;

    if let Some(stat_path) = relative_path.strip_suffix("/stat") {
        let resolved =
            resolve_workspace_path(workspace_root, stat_path, PathRequirement::Existing).await?;
        let metadata = tokio::fs::metadata(&resolved.absolute)
            .await
            .map_err(|error| io_error("read file metadata", &resolved.absolute, error))?;

        return Ok(Json(FileStatResponse {
            path: resolved.relative,
            size: metadata.len(),
            is_file: metadata.is_file(),
            is_dir: metadata.is_dir(),
        })
        .into_response());
    }

    let resolved =
        resolve_workspace_path(workspace_root, &relative_path, PathRequirement::Existing).await?;
    let metadata = tokio::fs::metadata(&resolved.absolute)
        .await
        .map_err(|error| io_error("read download metadata", &resolved.absolute, error))?;
    if !metadata.is_file() {
        return Err(AppError::BadRequest(format!(
            "download path is not a file: {}",
            resolved.relative
        )));
    }

    let file = tokio::fs::File::open(&resolved.absolute)
        .await
        .map_err(|error| io_error("open download", &resolved.absolute, error))?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, metadata.len().to_string())
        .body(Body::from_stream(ReaderStream::new(file)))
        .map_err(|error| AppError::Internal(format!("failed to build download response: {error}")))
}

pub(super) async fn delete_file(
    State(state): State<AppState>,
    Path(relative_path): Path<String>,
) -> Result<StatusCode, AppError> {
    let resolved = resolve_workspace_path(
        &state.inner.workspace_root,
        &relative_path,
        PathRequirement::Existing,
    )
    .await?;
    let metadata = tokio::fs::symlink_metadata(&resolved.absolute)
        .await
        .map_err(|error| io_error("inspect delete target", &resolved.absolute, error))?;

    if metadata.is_dir() {
        tokio::fs::remove_dir_all(&resolved.absolute)
            .await
            .map_err(|error| io_error("delete directory", &resolved.absolute, error))?;
    } else if metadata.is_file() {
        tokio::fs::remove_file(&resolved.absolute)
            .await
            .map_err(|error| io_error("delete file", &resolved.absolute, error))?;
    } else {
        return Err(AppError::BadRequest(format!(
            "unsupported file type: {}",
            resolved.relative
        )));
    }

    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn reject_workspace_root_delete(
    State(state): State<AppState>,
) -> Result<StatusCode, AppError> {
    let _ =
        resolve_workspace_path(&state.inner.workspace_root, "", PathRequirement::Existing).await?;
    Err(AppError::BadRequest(
        "workspace root cannot be deleted".to_string(),
    ))
}

fn io_error(action: &str, path: &std::path::Path, error: std::io::Error) -> AppError {
    if error.kind() == ErrorKind::NotFound {
        AppError::NotFound(format!("path not found: {}", path.display()))
    } else {
        AppError::Internal(format!("failed to {action} {}: {error}", path.display()))
    }
}
