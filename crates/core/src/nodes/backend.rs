//! Inference backend configuration: CUDA EP, TensorRT EP, and IoBinding support.
//!
//! Provides [`InferenceBackend`] enum and [`build_session`] helper to create
//! `ort::Session` with the appropriate execution providers and optional TRT engine caching.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, Once, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use ort::{
    environment::Environment,
    ep::{CUDAExecutionProvider, ExecutionProvider, TensorRTExecutionProvider},
    memory::{AllocationDevice, Allocator, AllocatorType, MemoryInfo, MemoryType},
    session::{
        builder::{GraphOptimizationLevel, SessionBuilder},
        Session,
    },
    value::{DynTensorValueType, DynValue},
};
use sha2::{Digest, Sha256};
use tracing::{debug, error, info, warn};

use crate::node::PortDefinition;
use crate::types::{PortData, PortType};

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
    /// CUDA device the session runs on, for both the TensorRT and CUDA EPs.
    pub device_id: i32,
}

/// The `device_id` input of an inference node: the CUDA device to run on,
/// 0 when unset.
pub fn read_device_id(inputs: &HashMap<String, PortData>) -> Result<i32> {
    match inputs.get("device_id") {
        None => Ok(0),
        Some(PortData::Int(value)) => i32::try_from(*value)
            .ok()
            .filter(|id| *id >= 0)
            .ok_or_else(|| anyhow!("device_id must be a non-negative Int, got {value}")),
        Some(_) => bail!("device_id must be an Int"),
    }
}

/// The `device_id` input port shared by the inference nodes.
pub fn device_id_port() -> PortDefinition {
    PortDefinition {
        name: "device_id".to_string(),
        port_type: PortType::Int,
        required: false,
        default_value: Some(serde_json::json!(0)),
    }
}

fn output_memory_contract(cuda_pinned: bool) -> (AllocationDevice, MemoryType) {
    if cuda_pinned {
        (AllocationDevice::CUDA_PINNED, MemoryType::CPUOutput)
    } else {
        (AllocationDevice::CPU, MemoryType::Default)
    }
}

fn output_memory_info(cuda_pinned: bool) -> ort::Result<MemoryInfo> {
    let (device, memory_type) = output_memory_contract(cuda_pinned);
    MemoryInfo::new(device, 0, AllocatorType::Device, memory_type)
}

pub(crate) fn inference_output_memory_info(session: &Session) -> ort::Result<MemoryInfo> {
    let pinned = output_memory_info(true)?;
    match Allocator::new(session, pinned.clone()) {
        Ok(_) => Ok(pinned),
        Err(_) => output_memory_info(false),
    }
}

/// A CUDA-pinned host allocator for model inputs, when the session runs on
/// CUDA. Pinned inputs let ONNX Runtime copy them to the GPU with DMA instead
/// of staging pageable memory.
pub(crate) fn pinned_input_allocator(session: &Session) -> Option<Allocator> {
    let memory = MemoryInfo::new(
        AllocationDevice::CUDA_PINNED,
        0,
        AllocatorType::Device,
        MemoryType::CPUInput,
    )
    .ok()?;
    Allocator::new(session, memory).ok()
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
/// Keep the ONNX Runtime environment alive until the process exits.
///
/// `ort` releases its global environment from the executable's `.fini_array`.
/// glibc runs that only after the C++ static destructors that `libonnxruntime`
/// and the CUDA provider registered with `__cxa_atexit`, so `ReleaseEnv` then
/// works on destroyed state and corrupts the heap. The process intermittently
/// aborts with "corrupted double-linked list" in libcuda's destructor after a
/// successful CUDA job. A reference held by a static is never dropped, so the
/// environment is never released and the OS reclaims it on exit.
fn retain_ort_environment() -> Result<()> {
    static RETAINED: OnceLock<Arc<Environment>> = OnceLock::new();
    if RETAINED.get().is_none() {
        let _ = RETAINED.set(ort::environment::current()?);
    }
    Ok(())
}

pub fn build_session(config: &SessionConfig<'_>) -> Result<Session> {
    retain_ort_environment()?;
    let builder = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|error| -> ort::Error { error.into() })?;

    let session = match config.backend {
        InferenceBackend::Tensorrt => match config.trt_cache_dir {
            Some(entry) => open_trt_cache_entry(entry, |cache_dir| {
                commit_tensorrt_session(builder.clone(), config, cache_dir)
            })?,
            None => commit_tensorrt_session(builder, config, Path::new("trt_cache"))?,
        },
        InferenceBackend::Cuda => {
            let cuda = CUDAExecutionProvider::default();
            if !cuda.is_available().unwrap_or(false) {
                warn!("CUDA EP is not available — inference will fall back to CPU");
            }

            debug!(
                backend = "cuda",
                device_id = config.device_id,
                "Building session with CUDA EP"
            );

            builder
                .with_execution_providers([CUDAExecutionProvider::default()
                    .with_device_id(config.device_id)
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

/// Create a session with the TensorRT EP (CUDA EP fallback) whose engine
/// cache is `cache_dir`. A missing engine is built and written there while the
/// session is created, which can take several minutes.
fn commit_tensorrt_session(
    builder: SessionBuilder,
    config: &SessionConfig<'_>,
    cache_dir: &Path,
) -> Result<Session> {
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
        device_id = config.device_id,
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
                .with_device_id(config.device_id)
                .build(),
            CUDAExecutionProvider::default()
                .with_device_id(config.device_id)
                .build(),
        ])
        .map_err(|error| -> ort::Error { error.into() })?
        .commit_from_file(config.model_path)
        .with_context(|| format!("Failed to load ONNX model: {}", config.model_path.display()));

    let _ = stop_tx.send(());
    let _ = progress_thread.join();

    let elapsed = started.elapsed().as_secs_f64();
    match session_result {
        Ok(session) => {
            let after = cache_stats(cache_dir);
            let cache_updated =
                after.file_count > before.file_count || after.total_bytes > before.total_bytes;

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

            Ok(session)
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
            Err(error_value)
        }
    }
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

/// The inference runtime a TensorRT engine was built with.
///
/// A serialized engine only loads in the TensorRT build that wrote it, and the
/// TensorRT EP of ONNX Runtime leaves stale engines to the user, so both are
/// part of the cache key: upgrading either builds new engines instead of
/// loading old ones.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrtRuntimeIdentity {
    /// `getInferLibVersion()` of the TensorRT library, e.g. `100900` for
    /// 10.9.0, or `none` when it cannot be loaded (the session then falls
    /// back to the CUDA EP and writes no engine).
    pub tensorrt: String,
    /// The git commit of the ONNX Runtime build, from its build info. The
    /// build info of the bundled library reports no version number.
    pub onnxruntime: String,
}

impl TrtRuntimeIdentity {
    /// The runtime of this process. The first call loads ONNX Runtime and the
    /// TensorRT library.
    pub fn current() -> &'static Self {
        static CURRENT: OnceLock<TrtRuntimeIdentity> = OnceLock::new();
        CURRENT.get_or_init(|| Self {
            tensorrt: tensorrt_library_version()
                .map_or_else(|| "none".to_string(), |version| version.to_string()),
            onnxruntime: onnxruntime_build_identity(ort::info()),
        })
    }
}

#[cfg(unix)]
const TENSORRT_LIBRARIES: &[&str] = &["libnvinfer.so.10"];
#[cfg(windows)]
const TENSORRT_LIBRARIES: &[&str] = &["nvinfer_10.dll", "nvinfer.dll"];

/// `getInferLibVersion()` of the TensorRT library the TensorRT EP loads.
fn tensorrt_library_version() -> Option<i32> {
    TENSORRT_LIBRARIES.iter().find_map(|name| {
        // SAFETY: this loads the TensorRT library by the name the TensorRT EP
        // resolves (it is normally resident already) and calls
        // `getInferLibVersion`, an argument-less C function returning the
        // library version. The handle is leaked so the library stays mapped,
        // as it does for the EP.
        unsafe {
            let library = libloading::Library::new(*name).ok()?;
            let version = library
                .get::<unsafe extern "C" fn() -> i32>(b"getInferLibVersion")
                .ok()
                .map(|get_version| get_version());
            std::mem::forget(library);
            version
        }
    })
}

/// The `git-commit-id` field of `ort::info()`, or the first 12 hex digits of
/// the build info's SHA-256 when it has none.
fn onnxruntime_build_identity(build_info: &str) -> String {
    build_info
        .split(',')
        .find_map(|field| field.trim().strip_prefix("git-commit-id="))
        .map(str::trim)
        .filter(|commit| !commit.is_empty() && commit.chars().all(|c| c.is_ascii_alphanumeric()))
        .map_or_else(
            || format!("{:x}", Sha256::digest(build_info))[..12].to_string(),
            str::to_string,
        )
}

/// Format: `trt-{tensorrt}_ort-{onnxruntime}_device-{device_id}_{model_hash}_{input_h}x{input_w}[_tile-{tile_size}_orig-{original_h}x{original_w}]`
pub fn trt_cache_key(
    identity: TrtCacheIdentity,
    model_hash: &str,
    runtime: &TrtRuntimeIdentity,
) -> String {
    let base = format!(
        "trt-{}_ort-{}_device-{}_{}_{}x{}",
        runtime.tensorrt,
        runtime.onnxruntime,
        identity.device_id,
        model_hash,
        identity.input_h,
        identity.input_w
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
    runtime: &TrtRuntimeIdentity,
) -> Result<PathBuf> {
    let model = std::fs::read(model_path)
        .with_context(|| format!("failed to read ONNX model: {}", model_path.display()))?;
    let model_hash = format!("{:x}", Sha256::digest(model));
    let key = trt_cache_key(identity, &model_hash, runtime);
    Ok(resolve_trt_cache_dir(base_dir, Some(&key)))
}

/// Open a session whose TensorRT engine cache is the keyed directory `entry`.
/// `open` creates the session with the engine cache at the path it is given.
///
/// - An existing entry is loaded. If its engine cannot be deserialized, the
///   entry is moved aside to `<entry>.broken-<unix-ms>` and built again, once.
/// - A missing entry is built in a sibling `<entry>.tmp-<pid>-<nanos>` and
///   renamed into place, so a crash or a failed build never leaves a partial
///   entry under the final name.
/// - Building, and moving an entry aside, hold an exclusive lock on
///   `<entry>.lock`, so processes that share the cache directory never build
///   the same entry at the same time; the second one waits and loads it.
///
/// This relies on the TensorRT EP writing the engine while the session is
/// created (it builds at session creation, which is what takes minutes).
pub(crate) fn open_trt_cache_entry<T>(
    entry: &Path,
    mut open: impl FnMut(&Path) -> Result<T>,
) -> Result<T> {
    let mut found_broken = false;
    if trt_cache_entry_exists(entry) {
        match open(entry) {
            Ok(session) => return Ok(session),
            Err(error) if is_engine_load_error(&error) => {
                warn!(
                    entry = %entry.display(),
                    error = %format!("{error:#}"),
                    "Cached TensorRT engine cannot be loaded; building it again"
                );
                found_broken = true;
            }
            Err(error) => return Err(error),
        }
    }

    let _lock = lock_trt_cache_entry(entry)?;
    if trt_cache_entry_exists(entry) {
        if !found_broken {
            // Another process built it while this one waited for the lock.
            return open(entry);
        }
        // Another process may have replaced the broken entry meanwhile.
        match open(entry) {
            Ok(session) => return Ok(session),
            Err(error) if is_engine_load_error(&error) => set_aside_trt_cache_entry(entry)?,
            Err(error) => return Err(error),
        }
    }
    build_trt_cache_entry(entry, open)
}

/// Entries are only ever renamed into place complete, so any content means
/// the entry was built.
fn trt_cache_entry_exists(entry: &Path) -> bool {
    std::fs::read_dir(entry).is_ok_and(|mut contents| contents.next().is_some())
}

/// `<entry>.<suffix>`, next to `entry`.
fn trt_cache_sibling(entry: &Path, suffix: &str) -> PathBuf {
    let mut name = entry.file_name().map(OsString::from).unwrap_or_default();
    name.push(format!(".{suffix}"));
    entry.with_file_name(name)
}

/// The TensorRT EP fails session creation with "TensorRT EP could not
/// deserialize engine from cache: <path>" when a cached engine is unusable.
fn is_engine_load_error(error: &anyhow::Error) -> bool {
    format!("{error:#}").contains("deserialize engine")
}

fn lock_trt_cache_entry(entry: &Path) -> Result<std::fs::File> {
    let lock_path = trt_cache_sibling(entry, "lock");
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("failed to create TRT cache directory: {}", parent.display())
        })?;
    }
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("failed to open TRT cache lock: {}", lock_path.display()))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            info!(
                entry = %entry.display(),
                "Waiting for another process to finish building this TensorRT engine"
            );
            lock.lock().with_context(|| {
                format!("failed to lock TRT cache entry: {}", lock_path.display())
            })?;
        }
        Err(std::fs::TryLockError::Error(error)) => {
            return Err(error).with_context(|| {
                format!("failed to lock TRT cache entry: {}", lock_path.display())
            });
        }
    }
    Ok(lock)
}

fn set_aside_trt_cache_entry(entry: &Path) -> Result<()> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let broken = trt_cache_sibling(entry, &format!("broken-{millis}"));
    std::fs::rename(entry, &broken).with_context(|| {
        format!(
            "failed to move unusable TRT cache entry {} aside",
            entry.display()
        )
    })?;
    warn!(
        entry = %entry.display(),
        moved_to = %broken.display(),
        "Moved unusable TensorRT engine cache aside"
    );
    Ok(())
}

/// Build `entry` in a temporary sibling and rename it into place. The caller
/// holds the entry lock.
fn build_trt_cache_entry<T>(entry: &Path, mut open: impl FnMut(&Path) -> Result<T>) -> Result<T> {
    remove_abandoned_trt_builds(entry);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let build_dir = trt_cache_sibling(entry, &format!("tmp-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&build_dir).with_context(|| {
        format!(
            "failed to create TRT cache build directory: {}",
            build_dir.display()
        )
    })?;

    let session = open(&build_dir);
    if session.is_ok() && trt_cache_entry_exists(&build_dir) {
        // An empty directory left at the final name would fail the rename on
        // Windows; remove_dir leaves a non-empty one alone.
        let _ = std::fs::remove_dir(entry);
        if let Err(error) = std::fs::rename(&build_dir, entry) {
            // The session is already open; the next run builds the engine again.
            warn!(
                entry = %entry.display(),
                error = %error,
                "Failed to place the built TensorRT engine in the cache"
            );
        }
    }
    if let Err(error) = std::fs::remove_dir_all(&build_dir) {
        if error.kind() != std::io::ErrorKind::NotFound {
            warn!(
                dir = %build_dir.display(),
                error = %error,
                "Failed to remove TRT cache build directory"
            );
        }
    }
    session
}

/// Remove build directories of `entry` left by a process that died while
/// building it. The caller holds the entry lock, so none of them is in use.
fn remove_abandoned_trt_builds(entry: &Path) {
    let (Some(parent), Some(name)) = (entry.parent(), entry.file_name()) else {
        return;
    };
    let mut prefix = name.to_os_string();
    prefix.push(".tmp-");
    let prefix = prefix.to_string_lossy().into_owned();
    let Ok(siblings) = std::fs::read_dir(parent) else {
        return;
    };
    for sibling in siblings.flatten() {
        if sibling.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = std::fs::remove_dir_all(sibling.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ort::memory::AllocationDevice;

    #[test]
    fn output_memory_info_when_cuda_pinned_uses_cpu_output_memory() {
        // Given: output tensors produced by a CUDA execution provider.

        // When: the pinned output memory contract is selected.
        let (device, memory_type) = output_memory_contract(true);

        // Then: ONNX Runtime receives CUDA-pinned host memory.
        assert_eq!(device, AllocationDevice::CUDA_PINNED);
        assert_eq!(memory_type, MemoryType::CPUOutput);
    }

    #[test]
    fn output_memory_info_when_cpu_fallback_uses_default_cpu_memory() {
        // Given: a session that fell back to the CPU execution provider.

        // When: the fallback output memory contract is selected.
        let (device, memory_type) = output_memory_contract(false);

        // Then: output binding remains CPU-compatible.
        assert_eq!(device, AllocationDevice::CPU);
        assert_eq!(memory_type, MemoryType::Default);
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

    fn runtime(tensorrt: &str, onnxruntime: &str) -> TrtRuntimeIdentity {
        TrtRuntimeIdentity {
            tensorrt: tensorrt.to_string(),
            onnxruntime: onnxruntime.to_string(),
        }
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
            &runtime("100900", "a83fc4d58c"),
        );
        assert_eq!(key, "trt-100900_ort-a83fc4d58c_device-0_abc123_1080x1920");
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
            &runtime("100700", "a83fc4d58c"),
        );
        assert_eq!(
            key,
            "trt-100700_ort-a83fc4d58c_device-1_def456_160x240_tile-64_orig-157x239"
        );
    }

    #[test]
    fn trt_cache_key_changes_with_the_tensorrt_or_onnxruntime_build() {
        let identity = TrtCacheIdentity {
            device_id: 0,
            input_h: 1088,
            input_w: 1920,
            tile: None,
        };
        let baseline = trt_cache_key(identity, "model", &runtime("100700", "a83fc4d58c"));

        assert_ne!(
            baseline,
            trt_cache_key(identity, "model", &runtime("100900", "a83fc4d58c"))
        );
        assert_ne!(
            baseline,
            trt_cache_key(identity, "model", &runtime("100700", "5c1b7ccbff"))
        );
    }

    #[test]
    fn onnxruntime_build_identity_uses_the_commit_or_a_hash_of_the_build_info() {
        assert_eq!(
            onnxruntime_build_identity(
                "ORT Build Info: git-branch=HEAD, git-commit-id=a83fc4d58c, build type=Release"
            ),
            "a83fc4d58c"
        );
        let hashed = onnxruntime_build_identity("ORT Build Info: build type=Release");
        assert_eq!(hashed.len(), 12);
        assert!(hashed.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(
            hashed,
            onnxruntime_build_identity("ORT Build Info: build type=Debug")
        );
    }

    #[test]
    #[ignore = "loads the ONNX Runtime and TensorRT libraries"]
    fn trt_runtime_identity_reads_the_loaded_libraries() {
        let runtime = TrtRuntimeIdentity::current();

        assert!(runtime.tensorrt.parse::<i32>().is_ok(), "{runtime:?}");
        assert!(!runtime.onnxruntime.is_empty(), "{runtime:?}");
    }

    fn write_engine(dir: &Path, contents: &str) -> Result<()> {
        std::fs::write(dir.join("model.engine"), contents)?;
        Ok(())
    }

    fn siblings(entry: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(entry.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn trt_cache_entry_is_built_aside_and_renamed_into_place() {
        let temp = tempfile::tempdir().unwrap();
        let entry = temp.path().join("key");
        let mut opened = Vec::new();

        open_trt_cache_entry(&entry, |dir| {
            opened.push(dir.to_path_buf());
            assert!(!entry.exists(), "the final entry must not exist mid-build");
            write_engine(dir, "engine")
        })
        .expect("build should succeed");

        assert_eq!(opened.len(), 1);
        assert_ne!(opened[0], entry, "the engine must be built in a sibling");
        assert_eq!(opened[0].parent(), entry.parent());
        assert_eq!(
            std::fs::read_to_string(entry.join("model.engine")).unwrap(),
            "engine"
        );
        assert_eq!(siblings(&entry), vec!["key", "key.lock"]);
    }

    #[test]
    fn existing_trt_cache_entry_is_loaded_in_place() {
        let temp = tempfile::tempdir().unwrap();
        let entry = temp.path().join("key");
        std::fs::create_dir_all(&entry).unwrap();
        write_engine(&entry, "cached").unwrap();
        let mut opened = Vec::new();

        open_trt_cache_entry(&entry, |dir| {
            opened.push(dir.to_path_buf());
            Ok(())
        })
        .unwrap();

        assert_eq!(opened, vec![entry.clone()]);
    }

    #[test]
    fn failed_trt_build_leaves_no_entry() {
        let temp = tempfile::tempdir().unwrap();
        let entry = temp.path().join("key");

        let error = open_trt_cache_entry(&entry, |dir| -> Result<()> {
            write_engine(dir, "partial")?;
            anyhow::bail!("out of memory")
        })
        .expect_err("the build error must be returned");

        assert_eq!(error.to_string(), "out of memory");
        assert_eq!(siblings(&entry), vec!["key.lock"]);
    }

    #[test]
    fn undeserializable_trt_cache_entry_is_moved_aside_and_rebuilt_once() {
        let temp = tempfile::tempdir().unwrap();
        let entry = temp.path().join("key");
        std::fs::create_dir_all(&entry).unwrap();
        write_engine(&entry, "stale").unwrap();
        let mut opened = Vec::new();

        open_trt_cache_entry(&entry, |dir| {
            opened.push(dir.to_path_buf());
            if std::fs::read_to_string(dir.join("model.engine")).is_ok_and(|e| e == "stale") {
                anyhow::bail!(
                    "TensorRT EP could not deserialize engine from cache: {}",
                    dir.display()
                );
            }
            write_engine(dir, "rebuilt")
        })
        .expect("the entry should be rebuilt");

        // Loaded, reloaded under the lock, then built once.
        assert_eq!(opened.len(), 3);
        assert_eq!(&opened[..2], &[entry.clone(), entry.clone()]);
        assert_ne!(opened[2], entry);
        assert_eq!(
            std::fs::read_to_string(entry.join("model.engine")).unwrap(),
            "rebuilt"
        );
        let names = siblings(&entry);
        assert_eq!(names.len(), 3, "{names:?}");
        assert!(names[1].starts_with("key.broken-"), "{names:?}");
        assert_eq!(names[2], "key.lock");
    }

    #[test]
    fn other_errors_on_an_existing_trt_cache_entry_keep_it() {
        let temp = tempfile::tempdir().unwrap();
        let entry = temp.path().join("key");
        std::fs::create_dir_all(&entry).unwrap();
        write_engine(&entry, "cached").unwrap();
        let mut calls = 0;

        open_trt_cache_entry(&entry, |_| -> Result<()> {
            calls += 1;
            anyhow::bail!("CUDA out of memory")
        })
        .expect_err("the error must be returned");

        assert_eq!(calls, 1);
        assert_eq!(siblings(&entry), vec!["key"]);
    }

    #[test]
    fn abandoned_trt_builds_are_removed_before_building() {
        let temp = tempfile::tempdir().unwrap();
        let entry = temp.path().join("key");
        let abandoned = temp.path().join("key.tmp-1-1");
        std::fs::create_dir_all(&abandoned).unwrap();
        write_engine(&abandoned, "partial").unwrap();
        std::fs::create_dir_all(temp.path().join("other.tmp-1-1")).unwrap();

        open_trt_cache_entry(&entry, |dir| write_engine(dir, "engine")).unwrap();

        assert_eq!(siblings(&entry), vec!["key", "key.lock", "other.tmp-1-1"]);
    }

    #[test]
    fn concurrent_opens_build_a_trt_cache_entry_once() {
        let temp = tempfile::tempdir().unwrap();
        let entry = temp.path().join("key");
        let builds = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let building = Arc::new(std::sync::Barrier::new(2));

        let first = {
            let entry = entry.clone();
            let builds = Arc::clone(&builds);
            let building = Arc::clone(&building);
            thread::spawn(move || {
                open_trt_cache_entry(&entry, |dir| {
                    builds.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    building.wait();
                    // Hold the lock while the second opener reaches it.
                    thread::sleep(Duration::from_millis(200));
                    write_engine(dir, "engine")
                })
            })
        };
        building.wait();
        let mut loaded_from = None;
        open_trt_cache_entry(&entry, |dir| {
            if dir != entry {
                builds.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                write_engine(dir, "engine")?;
            }
            loaded_from = Some(dir.to_path_buf());
            Ok(())
        })
        .unwrap();
        first.join().unwrap().unwrap();

        assert_eq!(builds.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(loaded_from, Some(entry.clone()));
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
        let runtime = runtime("100900", "a83fc4d58c");
        let baseline = model_trt_cache_dir(&base, &model_a, baseline_identity, &runtime)
            .expect("resolve baseline namespace");
        let same = model_trt_cache_dir(&base, &model_a, baseline_identity, &runtime)
            .expect("resolve matching namespace");
        let other_model = model_trt_cache_dir(&base, &model_b, baseline_identity, &runtime)
            .expect("resolve model namespace");
        let other_device = model_trt_cache_dir(
            &base,
            &model_a,
            TrtCacheIdentity {
                device_id: 1,
                ..baseline_identity
            },
            &runtime,
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
            &runtime,
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
    fn read_device_id_defaults_to_zero_and_rejects_invalid_ids() {
        let device = |value: PortData| HashMap::from([("device_id".to_string(), value)]);

        assert_eq!(read_device_id(&HashMap::new()).unwrap(), 0);
        assert_eq!(read_device_id(&device(PortData::Int(1))).unwrap(), 1);
        for invalid in [
            PortData::Int(-1),
            PortData::Int(i64::from(i32::MAX) + 1),
            PortData::Str("1".to_string()),
        ] {
            read_device_id(&device(invalid)).expect_err("invalid device_id must be rejected");
        }
    }

    #[test]
    fn test_session_config_tensorrt() {
        let trt_cache_dir = std::env::temp_dir().join("trt_cache");
        let config = SessionConfig {
            model_path: Path::new("model.onnx"),
            backend: &InferenceBackend::Tensorrt,
            trt_cache_dir: Some(trt_cache_dir.as_path()),
            device_id: 0,
        };
        assert_eq!(config.backend, &InferenceBackend::Tensorrt);
        assert_eq!(config.trt_cache_dir.unwrap(), trt_cache_dir.as_path());
    }
}
