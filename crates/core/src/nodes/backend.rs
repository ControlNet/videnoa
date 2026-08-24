//! Inference backend configuration: CUDA EP, TensorRT EP, and IoBinding support.
//!
//! Provides [`InferenceBackend`] enum and [`build_session`] helper to create
//! `ort::Session` with the appropriate execution providers and optional TRT engine caching.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::Once;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ort::{
    ep::{CUDAExecutionProvider, ExecutionProvider, TensorRTExecutionProvider},
    memory::{AllocationDevice, Allocator, AllocatorType, MemoryInfo, MemoryType},
    session::{builder::GraphOptimizationLevel, Session},
    value::{DynTensorValueType, DynValue},
};
use sha2::{Digest, Sha256};
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

fn output_memory_info(cuda_pinned: bool) -> ort::Result<MemoryInfo> {
    let (device, memory_type) = if cuda_pinned {
        (AllocationDevice::CUDA_PINNED, MemoryType::CPUOutput)
    } else {
        (AllocationDevice::CPU, MemoryType::Default)
    };
    MemoryInfo::new(device, 0, AllocatorType::Device, memory_type)
}

pub(crate) fn inference_output_memory_info(session: &Session) -> ort::Result<MemoryInfo> {
    let pinned = output_memory_info(true)?;
    match Allocator::new(session, pinned.clone()) {
        Ok(_) => Ok(pinned),
        Err(_) => output_memory_info(false),
    }
}

pub(crate) fn ensure_inference_output_memory(
    value: &DynValue,
    expected: &MemoryInfo,
) -> Result<()> {
    let tensor = value.downcast_ref::<DynTensorValueType>()?;
    let memory = tensor.memory_info();
    anyhow::ensure!(
        memory.allocation_device() == expected.allocation_device()
            && memory.memory_type() == expected.memory_type(),
        "expected inference output memory device={} memory_type={:?}, got device={} memory_type={:?}",
        expected.allocation_device().as_str(),
        expected.memory_type(),
        memory.allocation_device().as_str(),
        memory.memory_type()
    );

    static LOG_ONCE: Once = Once::new();
    LOG_ONCE.call_once(|| {
        debug!(
            allocation_device = memory.allocation_device().as_str(),
            memory_type = ?memory.memory_type(),
            "Verified inference output memory"
        );
    });

    Ok(())
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
    let builder = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|error| -> ort::Error { error.into() })?;

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
                ])
                .map_err(|error| -> ort::Error { error.into() })?
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
                    .error_on_failure()])
                .map_err(|error| -> ort::Error { error.into() })?
                .commit_from_file(config.model_path)
                .with_context(|| {
                    format!("Failed to load ONNX model: {}", config.model_path.display())
                })?
        }
    };

    Ok(session)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrtTileIdentity {
    pub tile_size: usize,
    pub original_h: usize,
    pub original_w: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrtCacheIdentity {
    pub device_id: usize,
    pub input_h: usize,
    pub input_w: usize,
    pub tile: Option<TrtTileIdentity>,
}

/// Format: `device-{device_id}_{model_hash}_{input_h}x{input_w}[_tile-{tile_size}_orig-{original_h}x{original_w}]`
pub fn trt_cache_key(identity: TrtCacheIdentity, model_hash: &str) -> String {
    let base = format!(
        "device-{}_{}_{}x{}",
        identity.device_id, model_hash, identity.input_h, identity.input_w
    );
    match identity.tile {
        Some(tile) => format!(
            "{base}_tile-{}_orig-{}x{}",
            tile.tile_size, tile.original_h, tile.original_w
        ),
        None => base,
    }
}

pub fn resolve_trt_cache_dir(base_dir: &Path, cache_key: Option<&str>) -> PathBuf {
    match cache_key {
        Some(key) => base_dir.join(key),
        None => base_dir.to_path_buf(),
    }
}

pub fn model_trt_cache_dir(
    base_dir: &Path,
    model_path: &Path,
    identity: TrtCacheIdentity,
) -> Result<PathBuf> {
    let model = std::fs::read(model_path)
        .with_context(|| format!("failed to read ONNX model: {}", model_path.display()))?;
    let model_hash = format!("{:x}", Sha256::digest(model));
    let key = trt_cache_key(identity, &model_hash);
    Ok(resolve_trt_cache_dir(base_dir, Some(&key)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ort::memory::AllocationDevice;

    #[test]
    fn output_memory_info_when_cuda_pinned_uses_cpu_output_memory() {
        // Given: output tensors produced by a CUDA execution provider.

        // When: the pinned output memory contract is created.
        let memory = output_memory_info(true).expect("create pinned output memory info");

        // Then: ONNX Runtime receives CUDA-pinned host memory.
        assert_eq!(memory.allocation_device(), AllocationDevice::CUDA_PINNED);
        assert_eq!(memory.memory_type(), MemoryType::CPUOutput);
    }

    #[test]
    fn output_memory_info_when_cpu_fallback_uses_default_cpu_memory() {
        // Given: a session that fell back to the CPU execution provider.

        // When: the fallback output memory contract is created.
        let memory = output_memory_info(false).expect("create CPU output memory info");

        // Then: output binding remains CPU-compatible.
        assert_eq!(memory.allocation_device(), AllocationDevice::CPU);
        assert_eq!(memory.memory_type(), MemoryType::Default);
    }

    #[test]
    #[ignore]
    fn inference_output_memory_info_when_session_is_cpu_only_uses_cpu() {
        // Given: a real ONNX Runtime session with environment execution providers disabled.
        let mut builder = Session::builder().expect("create session builder");
        builder = builder
            .with_no_environment_execution_providers()
            .expect("disable environment execution providers");
        let model_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../models/RealESRGAN_x4plus_anime_6B.onnx");
        let session = builder
            .commit_from_file(model_path)
            .expect("create CPU-only session");

        // When: the session-specific output memory contract is selected.
        let memory = inference_output_memory_info(&session).expect("select output memory");

        // Then: the CPU session does not request a CUDA-pinned allocator.
        assert_eq!(memory.allocation_device(), AllocationDevice::CPU);
        assert_eq!(memory.memory_type(), MemoryType::Default);
    }

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
        let key = trt_cache_key(
            TrtCacheIdentity {
                device_id: 0,
                input_h: 1080,
                input_w: 1920,
                tile: None,
            },
            "abc123",
        );
        assert_eq!(key, "device-0_abc123_1080x1920");
    }

    #[test]
    fn test_trt_cache_key_small_input() {
        let key = trt_cache_key(
            TrtCacheIdentity {
                device_id: 1,
                input_h: 160,
                input_w: 240,
                tile: Some(TrtTileIdentity {
                    tile_size: 64,
                    original_h: 157,
                    original_w: 239,
                }),
            },
            "def456",
        );
        assert_eq!(key, "device-1_def456_160x240_tile-64_orig-157x239");
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
    fn trt_cache_namespace_when_model_device_or_shape_changes_is_isolated() {
        // Given: two model files and one TensorRT cache root.
        let temp = tempfile::tempdir().expect("create temporary cache fixture");
        let model_a = temp.path().join("model-a.onnx");
        let model_b = temp.path().join("model-b.onnx");
        std::fs::write(&model_a, b"model-a").expect("write first model fixture");
        std::fs::write(&model_b, b"model-b").expect("write second model fixture");

        // When: cache namespaces are resolved for different models, devices, and shapes.
        let base = temp.path().join("trt-cache");
        let baseline_identity = TrtCacheIdentity {
            device_id: 0,
            input_h: 1088,
            input_w: 1920,
            tile: None,
        };
        let baseline = model_trt_cache_dir(&base, &model_a, baseline_identity)
            .expect("resolve baseline namespace");
        let same = model_trt_cache_dir(&base, &model_a, baseline_identity)
            .expect("resolve matching namespace");
        let other_model = model_trt_cache_dir(&base, &model_b, baseline_identity)
            .expect("resolve model namespace");
        let other_device = model_trt_cache_dir(
            &base,
            &model_a,
            TrtCacheIdentity {
                device_id: 1,
                ..baseline_identity
            },
        )
        .expect("resolve device namespace");
        let other_shape = model_trt_cache_dir(
            &base,
            &model_a,
            TrtCacheIdentity {
                input_h: 2176,
                input_w: 3840,
                ..baseline_identity
            },
        )
        .expect("resolve shape namespace");

        // Then: identical sessions reuse one directory and incompatible sessions do not.
        assert_eq!(baseline, same);
        assert_ne!(baseline, other_model);
        assert_ne!(baseline, other_device);
        assert_ne!(baseline, other_shape);
        assert_eq!(baseline.parent(), Some(base.as_path()));
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
