//! Inference backend configuration: CUDA EP, TensorRT EP, and IoBinding support.
//!
//! Provides [`InferenceBackend`] enum and [`build_session`] helper to create
//! `ort::Session` with the appropriate execution providers and optional TRT engine caching.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ort::{
    execution_providers::{CUDAExecutionProvider, ExecutionProvider, TensorRTExecutionProvider},
    session::{builder::GraphOptimizationLevel, Session},
};
use tracing::{debug, error, info, warn};

/// Inference backend selection.
///
/// Default is `Cuda`. `Tensorrt` requires TensorRT runtime libraries (`libnvinfer.so.10` or `nvinfer.dll`)
/// to be installed; if unavailable, the session falls back to CUDA EP automatically.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum InferenceBackend {
    #[default]
    Cuda,
    Tensorrt,
}

impl InferenceBackend {
    /// Parse from string (case-insensitive). Returns `Cuda` for unknown values.
    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "tensorrt" | "trt" => Self::Tensorrt,
            _ => Self::Cuda,
        }
    }
}

impl std::fmt::Display for InferenceBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cuda => write!(f, "cuda"),
            Self::Tensorrt => write!(f, "tensorrt"),
        }
    }
}

pub struct SessionConfig<'a> {
    pub model_path: &'a Path,
    pub backend: &'a InferenceBackend,
    pub trt_cache_dir: Option<&'a Path>,
}

#[derive(Clone, Copy, Debug, Default)]
struct CacheStats {
    file_count: u64,
    total_bytes: u64,
}

fn cache_stats(root: &Path) -> CacheStats {
    if !root.exists() {
        return CacheStats::default();
    }

    let mut stats = CacheStats::default();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }

            let meta = match entry.metadata() {
                Ok(meta) => meta,
                Err(_) => continue,
            };

            if meta.is_file() {
                stats.file_count += 1;
                stats.total_bytes += meta.len();
            }
        }
    }

    stats
}

/// Build an `ort::Session` with the requested backend and fallback chain.
///
/// For `InferenceBackend::Tensorrt`:
///   - Registers TRT EP with engine caching, then CUDA EP as fallback.
///   - If TRT runtime is unavailable, CUDA EP is used automatically.
///
/// For `InferenceBackend::Cuda`:
///   - Registers CUDA EP only.
///
/// In both cases, if CUDA EP is also unavailable, ORT falls back to CPU.
pub fn build_session(config: &SessionConfig<'_>) -> Result<Session> {
    let builder = Session::builder()?.with_optimization_level(GraphOptimizationLevel::Level3)?;

    let session = match config.backend {
        InferenceBackend::Tensorrt => {
            let cache_dir = config
                .trt_cache_dir
                .unwrap_or_else(|| Path::new("trt_cache"));

            if let Err(e) = std::fs::create_dir_all(cache_dir) {
                warn!(
                    dir = %cache_dir.display(),
                    error = %e,
                    "Failed to create TRT cache directory"
                );
            }

            let cache_path = cache_dir.to_string_lossy().to_string();
            let before = cache_stats(cache_dir);
            let started = Instant::now();

            debug!(
                backend = "tensorrt",
                cache_dir = %cache_dir.display(),
                "Building session with TensorRT EP (CUDA EP fallback)"
            );

            info!(
                cache_dir = %cache_dir.display(),
                cache_files = before.file_count,
                cache_bytes = before.total_bytes,
                "Initializing TensorRT session (first run may take several minutes)"
            );

            let (stop_tx, stop_rx) = channel::<()>();
            let cache_dir_for_log = cache_dir.display().to_string();
            let progress_thread = thread::spawn(move || {
                let tick = Duration::from_secs(15);
                let mut elapsed = 15_u64;
                loop {
                    match stop_rx.recv_timeout(tick) {
                        Ok(_) | Err(RecvTimeoutError::Disconnected) => break,
                        Err(RecvTimeoutError::Timeout) => {
                            info!(
                                elapsed_secs = elapsed,
                                cache_dir = %cache_dir_for_log,
                                "TensorRT session initialization still in progress"
                            );
                            elapsed += 15;
                        }
                    }
                }
            });

            // TRT EP may fail at runtime if libnvinfer.so.10 (or nvinfer.dll) is not installed.
            // The fallback CUDA EP ensures inference still works.
            let session_result = builder
                .with_execution_providers([
                    TensorRTExecutionProvider::default()
                        .with_engine_cache(true)
                        .with_engine_cache_path(&cache_path)
                        .with_fp16(true)
                        .with_device_id(0)
                        .build(),
                    CUDAExecutionProvider::default().build(),
                ])?
                .commit_from_file(config.model_path)
                .with_context(|| {
                    format!("Failed to load ONNX model: {}", config.model_path.display())
                });

            let _ = stop_tx.send(());
            let _ = progress_thread.join();

            let elapsed = started.elapsed().as_secs_f64();
            match session_result {
                Ok(session) => {
                    let after = cache_stats(cache_dir);
                    let cache_updated = after.file_count > before.file_count
                        || after.total_bytes > before.total_bytes;

                    if cache_updated {
                        info!(
                            elapsed_secs = elapsed,
                            cache_dir = %cache_dir.display(),
                            cache_files_before = before.file_count,
                            cache_files_after = after.file_count,
                            cache_bytes_before = before.total_bytes,
                            cache_bytes_after = after.total_bytes,
                            "TensorRT session ready; engine cache updated"
                        );
                    } else {
                        info!(
                            elapsed_secs = elapsed,
                            cache_dir = %cache_dir.display(),
                            cache_files = after.file_count,
                            cache_bytes = after.total_bytes,
                            "TensorRT session ready; using existing cache"
                        );
                    }

                    session
                }
                Err(error_value) => {
                    let after = cache_stats(cache_dir);
                    error!(
                        elapsed_secs = elapsed,
                        cache_dir = %cache_dir.display(),
                        cache_files_before = before.file_count,
                        cache_files_after = after.file_count,
                        cache_bytes_before = before.total_bytes,
                        cache_bytes_after = after.total_bytes,
                        error = %error_value,
                        "TensorRT session initialization failed"
                    );
                    return Err(error_value);
                }
            }
        }
        InferenceBackend::Cuda => {
            let cuda = CUDAExecutionProvider::default();
            if !cuda.is_available().unwrap_or(false) {
                warn!("CUDA EP is not available — inference will fall back to CPU");
            }

            debug!(backend = "cuda", "Building session with CUDA EP");

            builder
                .with_execution_providers([CUDAExecutionProvider::default()
                    .build()
                    .error_on_failure()])?
                .commit_from_file(config.model_path)
                .with_context(|| {
                    format!("Failed to load ONNX model: {}", config.model_path.display())
                })?
        }
    };

    Ok(session)
}

/// Format: `{compute_capability}_{model_hash}_{input_h}x{input_w}`
pub fn trt_cache_key(
    compute_capability: &str,
    model_hash: &str,
    input_h: usize,
    input_w: usize,
) -> String {
    format!(
        "{}_{}_{}x{}",
        compute_capability, model_hash, input_h, input_w
    )
}

pub fn resolve_trt_cache_dir(base_dir: &Path, cache_key: Option<&str>) -> PathBuf {
    match cache_key {
        Some(key) => base_dir.join(key),
        None => base_dir.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_from_str_lossy() {
        assert_eq!(
            InferenceBackend::from_str_lossy("cuda"),
            InferenceBackend::Cuda
        );
        assert_eq!(
            InferenceBackend::from_str_lossy("CUDA"),
            InferenceBackend::Cuda
        );
        assert_eq!(
            InferenceBackend::from_str_lossy("tensorrt"),
            InferenceBackend::Tensorrt
        );
        assert_eq!(
            InferenceBackend::from_str_lossy("TensorRT"),
            InferenceBackend::Tensorrt
        );
        assert_eq!(
            InferenceBackend::from_str_lossy("trt"),
            InferenceBackend::Tensorrt
        );
        assert_eq!(
            InferenceBackend::from_str_lossy("TRT"),
            InferenceBackend::Tensorrt
        );
        assert_eq!(
            InferenceBackend::from_str_lossy("unknown"),
            InferenceBackend::Cuda
        );
        assert_eq!(InferenceBackend::from_str_lossy(""), InferenceBackend::Cuda);
    }

    #[test]
    fn test_backend_default() {
        assert_eq!(InferenceBackend::default(), InferenceBackend::Cuda);
    }

    #[test]
    fn test_backend_display() {
        assert_eq!(InferenceBackend::Cuda.to_string(), "cuda");
        assert_eq!(InferenceBackend::Tensorrt.to_string(), "tensorrt");
    }

    #[test]
    fn test_trt_cache_key() {
        let key = trt_cache_key("8.0", "abc123", 1080, 1920);
        assert_eq!(key, "8.0_abc123_1080x1920");
    }

    #[test]
    fn test_trt_cache_key_small_input() {
        let key = trt_cache_key("8.6", "def456", 160, 240);
        assert_eq!(key, "8.6_def456_160x240");
    }

    #[test]
    fn test_resolve_trt_cache_dir_with_key() {
        let base = PathBuf::from("trt_cache");
        let resolved = resolve_trt_cache_dir(&base, Some("8.0_abc_1080x1920"));
        assert_eq!(resolved, PathBuf::from("trt_cache/8.0_abc_1080x1920"));
    }

    #[test]
    fn test_resolve_trt_cache_dir_without_key() {
        let base = PathBuf::from("trt_cache");
        let resolved = resolve_trt_cache_dir(&base, None);
        assert_eq!(resolved, PathBuf::from("trt_cache"));
    }

    #[test]
    fn test_session_config_tensorrt() {
        let trt_cache_dir = std::env::temp_dir().join("trt_cache");
        let config = SessionConfig {
            model_path: Path::new("model.onnx"),
            backend: &InferenceBackend::Tensorrt,
            trt_cache_dir: Some(trt_cache_dir.as_path()),
        };
        assert_eq!(config.backend, &InferenceBackend::Tensorrt);
        assert_eq!(config.trt_cache_dir.unwrap(), trt_cache_dir.as_path());
    }
}
