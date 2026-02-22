use std::collections::{HashMap, VecDeque};
use std::path::{Path as StdPath, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, RwLock, Semaphore};
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;
#[cfg(debug_assertions)]
use tower_http::services::{ServeDir, ServeFile};
use tracing::{error, info, warn};
use uuid::Uuid;

mod persistence;

use crate::config::AppConfig;
use crate::debug_event::NodeDebugValueEvent;
use crate::descriptor::{all_node_descriptors, NodeDescriptor};
use crate::executor::SequentialExecutor;
use crate::graph::PipelineGraph;
use crate::jellyfin::{ItemQuery, JellyfinClient};
use crate::model_inspect::{self, ModelInspection};
use crate::model_registry::{ModelEntry, ModelRegistry};
use crate::nodes::compile_context::VideoCompileContext;
use crate::registry::{register_all_nodes, NodeRegistry};
use persistence::JobsPersistence;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub description: String,
    pub workflow: serde_json::Value,
}

#[derive(Serialize)]
pub struct PresetResponse {
    pub id: String,
    pub name: String,
    pub description: String,
    pub workflow: serde_json::Value,
}

#[derive(Deserialize)]
pub struct CreatePresetRequest {
    pub name: String,
    pub description: String,
    pub workflow: serde_json::Value,
}

#[derive(Clone)]
pub struct AppState {
    inner: Arc<AppStateInner>,
}

struct AppStateInner {
    jobs: DashMap<String, Job>,
    jobs_persistence: Option<JobsPersistence>,
    gpu_semaphore: Arc<Semaphore>,
    node_registry: NodeRegistry,
    model_registry: ModelRegistry,
    progress_senders: DashMap<String, broadcast::Sender<JobWsEvent>>,
    presets: DashMap<String, Preset>,
    config: RwLock<AppConfig>,
    config_path: PathBuf,
    data_dir: PathBuf,
    preview_sessions: DashMap<String, PathBuf>,
    performance_series: Mutex<VecDeque<RuntimePerformanceSeriesSample>>,
}

const PRINT_PREVIEW_THROTTLE_MS: u64 = 150;
const WORKFLOW_SOURCE_API_JOBS: &str = "api_jobs";
const WORKFLOW_SOURCE_API_BATCH: &str = "api_batch";
const WORKFLOW_SOURCE_API_RUN_WORKFLOWS: &str = "api_run_workflows";
const WORKFLOW_SOURCE_API_RUN_PRESETS: &str = "api_run_presets";
const DEFAULT_WORKFLOW_NAME_API_JOBS: &str = "ad-hoc workflow";
const DEFAULT_WORKFLOW_NAME_API_BATCH: &str = "batch workflow";
const RERUN_COMPLETED_REJECTION: &str = "cannot rerun completed job";

impl AppState {
    pub fn new(
        node_registry: NodeRegistry,
        model_registry: ModelRegistry,
        presets: DashMap<String, Preset>,
        config: AppConfig,
        config_path: PathBuf,
        data_dir: PathBuf,
    ) -> Self {
        let jobs = DashMap::new();

        let jobs_persistence = match JobsPersistence::new(&data_dir) {
            Ok(persistence) => Some(persistence),
            Err(err) => {
                warn!(
                    error = %err,
                    data_dir = %data_dir.display(),
                    "Failed to initialize jobs persistence; running with in-memory job state only"
                );
                None
            }
        };

        if let Some(persistence) = &jobs_persistence {
            match persistence.load_jobs_for_startup() {
                Ok(restored_jobs) => {
                    let restored_count = restored_jobs.len();
                    for job in restored_jobs {
                        jobs.insert(job.id.clone(), job);
                    }

                    info!(
                        restored_count,
                        db_path = %persistence.db_path().display(),
                        "Restored persisted jobs into runtime state"
                    );
                }
                Err(err) => {
                    warn!(
                        error = %err,
                        db_path = %persistence.db_path().display(),
                        "Failed to restore persisted jobs; continuing with empty runtime jobs"
                    );
                }
            }
        }

        Self {
            inner: Arc::new(AppStateInner {
                jobs,
                jobs_persistence,
                gpu_semaphore: Arc::new(Semaphore::new(1)),
                node_registry,
                model_registry,
                progress_senders: DashMap::new(),
                presets,
                config: RwLock::new(config),
                config_path,
                data_dir,
                preview_sessions: DashMap::new(),
                performance_series: Mutex::new(VecDeque::new()),
            }),
        }
    }

    fn persist_job_snapshot(&self, job: &Job) -> Result<()> {
        if let Some(persistence) = &self.inner.jobs_persistence {
            persistence.upsert_job(job)?;
        }
        Ok(())
    }

    /// Resolve workflows_dir relative to process current working directory.
    pub async fn resolve_workflows_dir(&self) -> PathBuf {
        let config = self.inner.config.read().await;

        let base_dir = match std::env::current_dir() {
            Ok(cwd) => cwd,
            Err(cwd_err) => {
                warn!(
                    error = %cwd_err,
                    data_dir = %self.inner.data_dir.display(),
                    "Failed to resolve current directory; falling back to executable directory for workflows_dir"
                );

                match std::env::current_exe() {
                    Ok(exe_path) => match exe_path.parent() {
                        Some(exe_dir) => exe_dir.to_path_buf(),
                        None => {
                            warn!(
                                executable = %exe_path.display(),
                                "Executable path has no parent directory; using workflows_dir value as provided"
                            );
                            return config.paths.workflows_dir.clone();
                        }
                    },
                    Err(exe_err) => {
                        warn!(
                            error = %exe_err,
                            "Failed to resolve executable path; using workflows_dir value as provided"
                        );
                        return config.paths.workflows_dir.clone();
                    }
                }
            }
        };

        crate::config::resolve_relative_to(&base_dir, &config.paths.workflows_dir)
    }
}

pub fn load_builtin_presets(dir: &StdPath) -> DashMap<String, Preset> {
    let presets = DashMap::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            warn!("Failed to read presets directory {}: {e}", dir.display());
            return presets;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let slug = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();

        match std::fs::read_to_string(&path) {
            Ok(contents) => match serde_json::from_str::<Preset>(&contents) {
                Ok(preset) => {
                    info!(id = %slug, name = %preset.name, "Loaded preset");
                    presets.insert(slug, preset);
                }
                Err(e) => warn!("Failed to parse preset {}: {e}", path.display()),
            },
            Err(e) => warn!("Failed to read preset {}: {e}", path.display()),
        }
    }

    presets
}

#[derive(Clone)]
pub struct Job {
    pub id: String,
    pub status: JobStatus,
    pub workflow: PipelineGraph,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub progress: Option<ProgressUpdate>,
    pub error: Option<String>,
    pub cancel_token: CancellationToken,
    pub params: Option<HashMap<String, serde_json::Value>>,
    pub workflow_name: String,
    pub workflow_source: String,
    pub rerun_of_job_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressUpdate {
    pub current_frame: u64,
    pub total_frames: Option<u64>,
    pub fps: f32,
    pub eta_seconds: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobWsEvent {
    Progress {
        current_frame: u64,
        total_frames: Option<u64>,
        fps: f32,
        eta_seconds: Option<f64>,
    },
    NodeDebugValue {
        node_id: String,
        node_type: String,
        value_preview: String,
        truncated: bool,
        preview_max_chars: usize,
    },
}

impl From<ProgressUpdate> for JobWsEvent {
    fn from(value: ProgressUpdate) -> Self {
        Self::Progress {
            current_frame: value.current_frame,
            total_frames: value.total_frames,
            fps: value.fps,
            eta_seconds: value.eta_seconds,
        }
    }
}

impl From<NodeDebugValueEvent> for JobWsEvent {
    fn from(value: NodeDebugValueEvent) -> Self {
        Self::NodeDebugValue {
            node_id: value.node_id,
            node_type: value.node_type,
            value_preview: value.value_preview,
            truncated: value.truncated,
            preview_max_chars: value.preview_max_chars,
        }
    }
}

#[derive(Debug)]
struct NodeDebugEventThrottle {
    window: Duration,
    last_emit_by_node_id: HashMap<String, Instant>,
}

#[derive(Clone, Copy)]
struct ProgressFpsBaseline {
    first_frame: u64,
    first_frame_instant: Instant,
}

fn estimate_input_fps_from_second_frame(
    baseline: Option<ProgressFpsBaseline>,
    current_frame: u64,
    now: Instant,
) -> (Option<ProgressFpsBaseline>, f32) {
    if current_frame == 0 {
        return (baseline, 0.0);
    }

    match baseline {
        None => (
            Some(ProgressFpsBaseline {
                first_frame: current_frame,
                first_frame_instant: now,
            }),
            0.0,
        ),
        Some(existing) => {
            if current_frame < existing.first_frame {
                return (
                    Some(ProgressFpsBaseline {
                        first_frame: current_frame,
                        first_frame_instant: now,
                    }),
                    0.0,
                );
            }

            if current_frame == existing.first_frame {
                return (Some(existing), 0.0);
            }

            let elapsed = now
                .saturating_duration_since(existing.first_frame_instant)
                .as_secs_f64();
            if elapsed <= 0.0 {
                return (Some(existing), 0.0);
            }

            let frames_since_first = (current_frame - existing.first_frame) as f64;
            (Some(existing), (frames_since_first / elapsed) as f32)
        }
    }
}

impl NodeDebugEventThrottle {
    fn new(window: Duration) -> Self {
        Self {
            window,
            last_emit_by_node_id: HashMap::new(),
        }
    }

    fn should_emit(&mut self, node_id: &str, now: Instant) -> bool {
        if let Some(last_emit_at) = self.last_emit_by_node_id.get(node_id) {
            if now
                .checked_duration_since(*last_emit_at)
                .is_some_and(|elapsed| elapsed < self.window)
            {
                return false;
            }
        }

        self.last_emit_by_node_id.insert(node_id.to_string(), now);
        true
    }
}

#[derive(Deserialize)]
pub struct CreateJobRequest {
    pub workflow: serde_json::Value,
    #[serde(default)]
    pub workflow_name: Option<String>,
    #[serde(default)]
    pub params: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunWorkflowRequest {
    #[serde(default)]
    pub workflow_name: Option<String>,
    #[serde(default)]
    pub params: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Serialize)]
pub struct CreateJobResponse {
    pub id: String,
    pub status: JobStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct JobResponse {
    pub id: String,
    pub status: JobStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub progress: Option<ProgressUpdate>,
    pub error: Option<String>,
    pub workflow_name: String,
    pub workflow_source: String,
    pub params: Option<HashMap<String, serde_json::Value>>,
    pub rerun_of_job_id: Option<String>,
    pub duration_ms: Option<i64>,
}

#[derive(Deserialize)]
pub struct BatchRequest {
    pub file_paths: Vec<String>,
    pub workflow: serde_json::Value,
}

#[derive(Serialize)]
pub struct BatchResponse {
    pub job_ids: Vec<String>,
    pub total: usize,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Deserialize)]
pub struct FsListQuery {
    pub base: Option<String>,
    pub prefix: Option<String>,
}

#[derive(Deserialize)]
pub struct FsBrowseQuery {
    pub path: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct FsEntry {
    pub name: String,
    pub is_dir: bool,
    pub path: String,
}

#[derive(Deserialize)]
pub struct ExtractFramesRequest {
    pub video_path: String,
    pub count: u32,
}

#[derive(Serialize)]
pub struct FrameInfo {
    pub index: u32,
    pub url: String,
}

#[derive(Serialize)]
pub struct ExtractFramesResponse {
    pub preview_id: String,
    pub frames: Vec<FrameInfo>,
}

#[derive(Deserialize)]
pub struct ProcessFrameRequest {
    pub preview_id: String,
    pub frame_index: u32,
    #[allow(dead_code)]
    pub workflow: serde_json::Value,
}

#[derive(Serialize)]
pub struct ProcessFrameResponse {
    pub processed_url: String,
}

#[derive(Deserialize)]
pub struct SaveWorkflowRequest {
    pub name: String,
    pub description: String,
    pub workflow: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
pub struct WorkflowEntry {
    pub filename: String,
    pub name: String,
    pub description: String,
    pub workflow: serde_json::Value,
    pub has_interface: bool,
}

// ─── Embedded frontend assets (release builds only) ──────────────────────────

#[cfg(not(debug_assertions))]
#[derive(rust_embed::RustEmbed)]
#[folder = "../../web/dist"]
struct FrontendAssets;

#[cfg(not(debug_assertions))]
async fn embedded_static_handler(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match FrontendAssets::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                [(axum::http::header::CONTENT_TYPE, mime.as_ref())],
                file.data,
            )
                .into_response()
        }
        None => match FrontendAssets::get("index.html") {
            Some(index) => (
                [(axum::http::header::CONTENT_TYPE, "text/html")],
                index.data,
            )
                .into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        },
    }
}

// ─── Router ──────────────────────────────────────────────────────────────────

pub fn app_router(state: AppState) -> Router {
    app_router_with_static(state, None)
}

pub fn app_router_with_static(state: AppState, static_dir: Option<&StdPath>) -> Router {
    let api = Router::new()
        .route("/api/health", get(health))
        .route("/api/config", get(get_config).put(update_config))
        .route("/api/performance/current", get(get_performance_current))
        .route("/api/performance/overview", get(get_performance_overview))
        .route("/api/performance/export", get(get_performance_export))
        .route(
            "/api/performance/capabilities",
            get(get_performance_capabilities),
        )
        .route("/api/jobs", post(create_job).get(list_jobs))
        .route("/api/run", post(run_workflow_by_name))
        .route("/api/jobs/{id}", get(get_job).delete(delete_job_history))
        .route("/api/jobs/{id}/rerun", post(rerun_job))
        .route("/api/jobs/{id}/ws", any(job_ws))
        .route("/api/nodes", get(list_nodes))
        .route("/api/models", get(list_models))
        .route("/api/models/{filename}/inspect", get(inspect_model))
        .route("/api/batch", post(create_batch))
        .route("/api/presets", get(list_presets).post(create_preset))
        .route("/api/workflows", get(list_workflows).post(save_workflow))
        .route(
            "/api/workflows/{filename}/interface",
            get(get_workflow_interface),
        )
        .route("/api/workflows/{filename}", delete(delete_workflow))
        .route("/api/jellyfin/libraries", get(jellyfin_libraries))
        .route("/api/jellyfin/items", get(jellyfin_items))
        .route("/api/fs/list", get(list_fs))
        .route("/api/fs/browse", get(browse_fs))
        .route("/api/preview/extract", post(extract_frames))
        .route("/api/preview/process", post(process_frame))
        .route(
            "/api/preview/frames/{preview_id}/{filename}",
            get(serve_preview_frame),
        )
        .route("/api/{*path}", any(api_route_not_found))
        .layer(CorsLayer::permissive())
        .with_state(state);

    #[cfg(not(debug_assertions))]
    {
        let _ = static_dir;
        api.fallback(embedded_static_handler)
    }
    #[cfg(debug_assertions)]
    {
        if let Some(dir) = static_dir {
            let index = dir.join("index.html");
            let spa = ServeDir::new(dir).fallback(ServeFile::new(index));
            api.fallback_service(spa)
        } else {
            api
        }
    }
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
    })
}

#[derive(Clone)]
struct RuntimePerformanceSample {
    metrics: serde_json::Map<String, serde_json::Value>,
    has_cpu_metrics: bool,
    has_memory_metrics: bool,
    has_gpu_metrics: bool,
    has_vram_metrics: bool,
}

#[derive(Clone)]
struct RuntimePerformanceSeriesSample {
    timestamp_ms: i64,
    metrics: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Copy)]
struct CpuTimes {
    total_ticks: u64,
    idle_ticks: u64,
}

#[derive(Clone, Copy)]
struct NvidiaSmiGpuSnapshot {
    gpu_util_percent: f64,
    vram_used_bytes: u64,
    vram_total_bytes: u64,
}

const BYTES_PER_MIB: u64 = 1024 * 1024;
const PERFORMANCE_EXPORT_RETENTION_SAMPLES: usize = 180;
static PREVIOUS_CPU_TIMES: OnceLock<Mutex<Option<CpuTimes>>> = OnceLock::new();

fn read_proc_meminfo_kib(key: &str) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let contents = std::fs::read_to_string("/proc/meminfo").ok()?;
        for line in contents.lines() {
            if let Some(rest) = line.strip_prefix(key) {
                let raw_value = rest.trim().strip_suffix("kB")?.trim();
                return raw_value.parse::<u64>().ok();
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = key;
        None
    }
}

fn read_process_rss_kib() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let contents = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in contents.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                let raw_value = rest.trim().strip_suffix("kB")?.trim();
                return raw_value.parse::<u64>().ok();
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

fn parse_proc_stat_cpu_line(line: &str) -> Option<CpuTimes> {
    let mut fields = line.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }

    let ticks: Vec<u64> = fields
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .map(str::parse::<u64>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .ok()?;

    if ticks.len() < 4 {
        return None;
    }

    let idle_ticks = ticks[3].saturating_add(*ticks.get(4).unwrap_or(&0));
    let total_ticks = ticks.into_iter().sum();
    if total_ticks == 0 {
        return None;
    }

    Some(CpuTimes {
        total_ticks,
        idle_ticks,
    })
}

fn cpu_util_percent_since_boot(times: CpuTimes) -> Option<f64> {
    if times.total_ticks == 0 {
        return None;
    }

    let busy_ticks = times.total_ticks.saturating_sub(times.idle_ticks);
    let percent = (busy_ticks as f64 / times.total_ticks as f64) * 100.0;
    Some(percent.clamp(0.0, 100.0))
}

fn compute_cpu_util_percent(previous: CpuTimes, current: CpuTimes) -> Option<f64> {
    let total_delta = current.total_ticks.saturating_sub(previous.total_ticks);
    if total_delta == 0 {
        return None;
    }

    let idle_delta = current.idle_ticks.saturating_sub(previous.idle_ticks);
    let busy_delta = total_delta.saturating_sub(idle_delta);
    let percent = (busy_delta as f64 / total_delta as f64) * 100.0;
    Some(percent.clamp(0.0, 100.0))
}

fn read_proc_stat_cpu_times() -> Option<CpuTimes> {
    #[cfg(target_os = "linux")]
    {
        let contents = std::fs::read_to_string("/proc/stat").ok()?;
        let line = contents.lines().find(|raw| raw.starts_with("cpu "))?;
        parse_proc_stat_cpu_line(line)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

fn read_cpu_util_percent() -> Option<f64> {
    let current = read_proc_stat_cpu_times()?;
    let previous_cell = PREVIOUS_CPU_TIMES.get_or_init(|| Mutex::new(None));
    let mut previous_guard = previous_cell.lock().ok()?;

    let util_percent = match *previous_guard {
        Some(previous) => compute_cpu_util_percent(previous, current),
        None => cpu_util_percent_since_boot(current),
    };

    *previous_guard = Some(current);
    util_percent
}

fn parse_nvidia_smi_gpu_snapshot(stdout: &str) -> Option<NvidiaSmiGpuSnapshot> {
    let line = stdout.lines().find(|raw| !raw.trim().is_empty())?;
    let mut columns = line.split(',').map(|raw| raw.trim());

    let gpu_util_raw = columns.next()?;
    let vram_used_mib_raw = columns.next()?;
    let vram_total_mib_raw = columns.next()?;

    if gpu_util_raw.eq_ignore_ascii_case("N/A")
        || vram_used_mib_raw.eq_ignore_ascii_case("N/A")
        || vram_total_mib_raw.eq_ignore_ascii_case("N/A")
    {
        return None;
    }

    let gpu_util_percent = gpu_util_raw.parse::<f64>().ok()?.clamp(0.0, 100.0);
    let vram_used_bytes = vram_used_mib_raw
        .parse::<u64>()
        .ok()?
        .saturating_mul(BYTES_PER_MIB);
    let vram_total_bytes = vram_total_mib_raw
        .parse::<u64>()
        .ok()?
        .saturating_mul(BYTES_PER_MIB);

    Some(NvidiaSmiGpuSnapshot {
        gpu_util_percent,
        vram_used_bytes,
        vram_total_bytes,
    })
}

fn parse_nvidia_smi_compute_apps_vram(stdout: &str, pid: u32) -> Option<u64> {
    let mut total_vram_bytes = 0_u64;
    let mut matched = false;

    for line in stdout.lines().map(str::trim).filter(|raw| !raw.is_empty()) {
        let mut columns = line.split(',').map(|raw| raw.trim());
        let process_pid = columns.next().and_then(|raw| raw.parse::<u32>().ok());
        let used_mib_raw = columns.next();

        if process_pid != Some(pid) {
            continue;
        }

        let Some(raw) = used_mib_raw else {
            continue;
        };
        if raw.eq_ignore_ascii_case("N/A") {
            continue;
        }

        let Some(used_mib) = raw.parse::<u64>().ok() else {
            continue;
        };

        matched = true;
        total_vram_bytes = total_vram_bytes.saturating_add(used_mib.saturating_mul(BYTES_PER_MIB));
    }

    matched.then_some(total_vram_bytes)
}

fn query_nvidia_smi_gpu_snapshot() -> Option<NvidiaSmiGpuSnapshot> {
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("nvidia-smi")
            .args([
                "--query-gpu=utilization.gpu,memory.used,memory.total",
                "--format=csv,noheader,nounits",
            ])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_nvidia_smi_gpu_snapshot(stdout.as_ref())
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

fn query_nvidia_smi_process_vram_bytes(pid: u32) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("nvidia-smi")
            .args([
                "--query-compute-apps=pid,used_gpu_memory",
                "--format=csv,noheader,nounits",
            ])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_nvidia_smi_compute_apps_vram(stdout.as_ref(), pid)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        None
    }
}

fn collect_runtime_performance_sample() -> RuntimePerformanceSample {
    let cpu_util_percent = read_cpu_util_percent();

    let mem_total_bytes =
        read_proc_meminfo_kib("MemTotal:").map(|value| value.saturating_mul(1024));
    let mem_available_bytes =
        read_proc_meminfo_kib("MemAvailable:").map(|value| value.saturating_mul(1024));
    let mem_used_bytes = mem_total_bytes
        .zip(mem_available_bytes)
        .map(|(total, available)| total.saturating_sub(available));
    let process_rss_bytes = read_process_rss_kib().map(|value| value.saturating_mul(1024));

    let gpu_snapshot = query_nvidia_smi_gpu_snapshot();
    let process_vram_used_bytes = if gpu_snapshot.is_some() {
        query_nvidia_smi_process_vram_bytes(std::process::id())
    } else {
        None
    };

    let has_cpu_metrics = cpu_util_percent.is_some();
    let has_memory_metrics = mem_used_bytes.is_some() && mem_total_bytes.is_some();
    let has_gpu_metrics = gpu_snapshot.is_some();
    let has_vram_metrics = gpu_snapshot.is_some();

    let mut metrics = serde_json::Map::new();
    metrics.insert("cpu_util_percent".to_string(), serde_json::Value::Null);
    metrics.insert("gpu_util_percent".to_string(), serde_json::Value::Null);
    metrics.insert("vram_used_bytes".to_string(), serde_json::Value::Null);
    metrics.insert("vram_total_bytes".to_string(), serde_json::Value::Null);
    metrics.insert(
        "process_vram_used_bytes".to_string(),
        serde_json::Value::Null,
    );

    if let Some(value) = cpu_util_percent {
        metrics.insert("cpu_util_percent".to_string(), serde_json::json!(value));
    }

    if let Some(value) = mem_used_bytes {
        metrics.insert("ram_used_bytes".to_string(), serde_json::json!(value));
    }
    if let Some(value) = mem_total_bytes {
        metrics.insert("ram_total_bytes".to_string(), serde_json::json!(value));
    }
    if let Some(value) = process_rss_bytes {
        metrics.insert(
            "process_ram_used_bytes".to_string(),
            serde_json::json!(value),
        );
    }

    if let Some(snapshot) = gpu_snapshot {
        metrics.insert(
            "gpu_util_percent".to_string(),
            serde_json::json!(snapshot.gpu_util_percent),
        );
        metrics.insert(
            "vram_used_bytes".to_string(),
            serde_json::json!(snapshot.vram_used_bytes),
        );
        metrics.insert(
            "vram_total_bytes".to_string(),
            serde_json::json!(snapshot.vram_total_bytes),
        );
    }

    if let Some(value) = process_vram_used_bytes {
        metrics.insert(
            "process_vram_used_bytes".to_string(),
            serde_json::json!(value),
        );
    }

    RuntimePerformanceSample {
        has_cpu_metrics,
        has_memory_metrics,
        has_gpu_metrics,
        has_vram_metrics,
        metrics,
    }
}

fn record_runtime_performance_series_sample(
    state: &AppState,
    timestamp_ms: i64,
    sample: &RuntimePerformanceSample,
) {
    let mut performance_series = match state.inner.performance_series.lock() {
        Ok(guard) => guard,
        Err(err) => {
            warn!(error = %err, "failed to acquire performance series lock");
            return;
        }
    };

    performance_series.push_back(RuntimePerformanceSeriesSample {
        timestamp_ms,
        metrics: sample.metrics.clone(),
    });

    while performance_series.len() > PERFORMANCE_EXPORT_RETENTION_SAMPLES {
        performance_series.pop_front();
    }
}

fn export_runtime_performance_series_rows(state: &AppState) -> Vec<serde_json::Value> {
    let performance_series = match state.inner.performance_series.lock() {
        Ok(guard) => guard,
        Err(err) => {
            warn!(error = %err, "failed to acquire performance series lock");
            return Vec::new();
        }
    };

    if performance_series.is_empty() {
        return Vec::new();
    }

    let mut samples: Vec<RuntimePerformanceSeriesSample> =
        performance_series.iter().cloned().collect();

    if samples.len() == 1 {
        let only = samples[0].clone();
        samples.insert(
            0,
            RuntimePerformanceSeriesSample {
                timestamp_ms: only.timestamp_ms.saturating_sub(1000),
                metrics: only.metrics.clone(),
            },
        );
    }

    samples
        .into_iter()
        .map(|sample| {
            serde_json::json!({
                "timestamp_ms": sample.timestamp_ms,
                "metrics": sample.metrics,
            })
        })
        .collect()
}

fn disabled_performance_envelope() -> serde_json::Value {
    serde_json::json!({
        "status": "disabled",
        "enabled": false,
        "reason": "disabled_by_config",
        "message": "telemetry disabled",
    })
}

fn enabled_performance_envelope(sample: &RuntimePerformanceSample) -> serde_json::Value {
    if sample.has_cpu_metrics
        && sample.has_memory_metrics
        && sample.has_gpu_metrics
        && sample.has_vram_metrics
    {
        serde_json::json!({
            "status": "enabled",
            "enabled": true,
            "reason": "collector_ok",
            "message": "telemetry available",
        })
    } else if sample.has_cpu_metrics
        || sample.has_memory_metrics
        || sample.has_gpu_metrics
        || sample.has_vram_metrics
    {
        serde_json::json!({
            "status": "partial",
            "enabled": true,
            "reason": "limited_metrics",
            "message": "partial telemetry",
        })
    } else {
        serde_json::json!({
            "status": "degraded",
            "enabled": true,
            "reason": "metrics_unavailable",
            "message": "telemetry enabled with limited metrics",
        })
    }
}

async fn get_performance_current(State(state): State<AppState>) -> Json<serde_json::Value> {
    let profiling_enabled = {
        let config = state.inner.config.read().await;
        config.performance.profiling_enabled
    };

    if !profiling_enabled {
        let mut payload = disabled_performance_envelope();
        if let serde_json::Value::Object(ref mut object) = payload {
            object.insert("metrics".to_string(), serde_json::Value::Null);
        }
        return Json(payload);
    }

    let sample = collect_runtime_performance_sample();
    let mut payload = enabled_performance_envelope(&sample);
    if let serde_json::Value::Object(ref mut object) = payload {
        object.insert(
            "metrics".to_string(),
            serde_json::Value::Object(sample.metrics),
        );
    }

    Json(payload)
}

async fn get_performance_overview(State(state): State<AppState>) -> Json<serde_json::Value> {
    let profiling_enabled = {
        let config = state.inner.config.read().await;
        config.performance.profiling_enabled
    };

    if !profiling_enabled {
        let mut payload = disabled_performance_envelope();
        if let serde_json::Value::Object(ref mut object) = payload {
            object.insert("metrics".to_string(), serde_json::Value::Null);
        }
        return Json(payload);
    }

    let sample = collect_runtime_performance_sample();
    let mut payload = enabled_performance_envelope(&sample);
    if let serde_json::Value::Object(ref mut object) = payload {
        object.insert(
            "metrics".to_string(),
            serde_json::Value::Object(sample.metrics),
        );
    }

    Json(payload)
}

async fn get_performance_export(State(state): State<AppState>) -> Json<serde_json::Value> {
    let profiling_enabled = {
        let config = state.inner.config.read().await;
        config.performance.profiling_enabled
    };

    if !profiling_enabled {
        let mut payload = disabled_performance_envelope();
        if let serde_json::Value::Object(ref mut object) = payload {
            object.insert("series".to_string(), serde_json::json!([]));
        }
        return Json(payload);
    }

    let sample = collect_runtime_performance_sample();
    let now_ms = Utc::now().timestamp_millis();
    record_runtime_performance_series_sample(&state, now_ms, &sample);

    let mut payload = enabled_performance_envelope(&sample);
    if let serde_json::Value::Object(ref mut object) = payload {
        let series_rows = export_runtime_performance_series_rows(&state);
        object.insert("series".to_string(), serde_json::Value::Array(series_rows));
    }

    Json(payload)
}

async fn get_performance_capabilities(State(state): State<AppState>) -> Json<serde_json::Value> {
    let profiling_enabled = {
        let config = state.inner.config.read().await;
        config.performance.profiling_enabled
    };

    let mut payload = if profiling_enabled {
        serde_json::json!({
            "status": "enabled",
            "enabled": true,
            "reason": "configured",
            "message": "telemetry enabled",
        })
    } else {
        disabled_performance_envelope()
    };

    if let serde_json::Value::Object(ref mut object) = payload {
        object.insert(
            "supported_statuses".to_string(),
            serde_json::json!(["disabled", "enabled", "degraded", "partial"]),
        );
    }

    Json(payload)
}

async fn api_route_not_found(Path(path): Path<String>) -> AppError {
    AppError::NotFound(format!("api endpoint not found: /api/{path}"))
}

async fn get_config(State(state): State<AppState>) -> Json<AppConfig> {
    let config = state.inner.config.read().await.clone();
    Json(config)
}

async fn update_config(
    State(state): State<AppState>,
    Json(payload): Json<AppConfig>,
) -> Result<Json<AppConfig>, AppError> {
    payload.save_to_path(&state.inner.config_path)?;

    {
        let mut config = state.inner.config.write().await;
        *config = payload.clone();
    }

    Ok(Json(payload))
}

async fn create_job(
    State(state): State<AppState>,
    Json(payload): Json<CreateJobRequest>,
) -> Result<(StatusCode, Json<CreateJobResponse>), AppError> {
    let workflow_name = payload
        .workflow_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            workflow_name_from_request(&payload.workflow, DEFAULT_WORKFLOW_NAME_API_JOBS)
        });

    let inferred_params = extract_workflow_input_params(&payload.workflow);
    let params = payload.params.or(inferred_params);

    let workflow = parse_and_validate_workflow(&state, payload.workflow)?;
    let created = create_and_spawn_job(
        &state,
        workflow,
        params,
        workflow_name,
        WORKFLOW_SOURCE_API_JOBS.to_string(),
        None,
    )?;

    Ok((StatusCode::CREATED, Json(created)))
}

async fn run_workflow_by_name(
    State(state): State<AppState>,
    Json(payload): Json<RunWorkflowRequest>,
) -> Result<(StatusCode, Json<CreateJobResponse>), AppError> {
    let workflow_name = validate_run_workflow_name(payload.workflow_name.as_deref())?;
    let resolved = resolve_run_workflow_file(&state, &workflow_name).await?;

    let workflow_document = std::fs::read_to_string(&resolved.path)
        .map_err(|e| AppError::Internal(format!("failed to read workflow: {e}")))?;
    let parsed_document: serde_json::Value = serde_json::from_str(&workflow_document)
        .map_err(|e| AppError::BadRequest(format!("invalid JSON: {e}")))?;
    let workflow_value = parsed_document
        .get("workflow")
        .cloned()
        .unwrap_or(parsed_document);

    let workflow = parse_and_validate_workflow(&state, workflow_value)?;
    let created = create_and_spawn_job(
        &state,
        workflow,
        payload.params,
        workflow_name,
        resolved.workflow_source.to_string(),
        None,
    )?;

    Ok((StatusCode::CREATED, Json(created)))
}

fn parse_and_validate_workflow(
    state: &AppState,
    workflow_json: serde_json::Value,
) -> Result<PipelineGraph, AppError> {
    let workflow: PipelineGraph =
        serde_json::from_value(workflow_json).map_err(|e| AppError::BadRequest(e.to_string()))?;

    workflow
        .validate(&state.inner.node_registry)
        .map_err(|e| AppError::BadRequest(format!("workflow validation failed: {e:#}")))?;

    Ok(workflow)
}

fn create_and_spawn_job(
    state: &AppState,
    workflow: PipelineGraph,
    params: Option<HashMap<String, serde_json::Value>>,
    workflow_name: String,
    workflow_source: String,
    rerun_of_job_id: Option<String>,
) -> Result<CreateJobResponse, AppError> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let cancel_token = CancellationToken::new();

    let (tx, _rx) = broadcast::channel::<JobWsEvent>(64);
    state.inner.progress_senders.insert(id.clone(), tx);

    let job = Job {
        id: id.clone(),
        status: JobStatus::Queued,
        workflow,
        created_at: now,
        started_at: None,
        completed_at: None,
        progress: None,
        error: None,
        cancel_token: cancel_token.clone(),
        params,
        workflow_name,
        workflow_source: workflow_source.clone(),
        rerun_of_job_id,
    };

    state
        .persist_job_snapshot(&job)
        .map_err(|e| AppError::Internal(format!("failed to persist new job: {e:#}")))?;

    state.inner.jobs.insert(id.clone(), job);

    let state_clone = state.clone();
    let job_id = id.clone();
    tokio::spawn(async move {
        run_job(state_clone, job_id).await;
    });

    info!(job_id = %id, workflow_source, "Job created");

    Ok(CreateJobResponse {
        id,
        status: JobStatus::Queued,
        created_at: now,
    })
}

struct ResolvedWorkflowFile {
    path: PathBuf,
    workflow_source: &'static str,
}

async fn resolve_run_workflow_file(
    state: &AppState,
    workflow_name: &str,
) -> Result<ResolvedWorkflowFile, AppError> {
    let filename = format!("{workflow_name}.json");
    sanitize_workflow_filename(&filename)?;

    let workflows_dir = state.resolve_workflows_dir().await;
    let workflows_path = workflows_dir.join(&filename);
    if workflows_path.exists() {
        return Ok(ResolvedWorkflowFile {
            path: workflows_path,
            workflow_source: WORKFLOW_SOURCE_API_RUN_WORKFLOWS,
        });
    }

    let config = state.inner.config.read().await;
    let presets_path = config.paths.presets_dir.join(&filename);
    if presets_path.exists() {
        return Ok(ResolvedWorkflowFile {
            path: presets_path,
            workflow_source: WORKFLOW_SOURCE_API_RUN_PRESETS,
        });
    }

    Err(AppError::NotFound(format!(
        "workflow not found: {workflow_name}"
    )))
}

fn validate_run_workflow_name(raw_name: Option<&str>) -> Result<String, AppError> {
    let workflow_name = raw_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| AppError::BadRequest("workflow_name is required".to_string()))?;

    if workflow_name.ends_with(".json") {
        return Err(AppError::BadRequest(
            "workflow_name must not include .json suffix".to_string(),
        ));
    }

    if workflow_name.contains('/') || workflow_name.contains('\\') {
        return Err(AppError::BadRequest(
            "workflow_name must not contain path separators".to_string(),
        ));
    }

    if workflow_name.contains("..") {
        return Err(AppError::BadRequest(
            "workflow_name must not contain '..'".to_string(),
        ));
    }

    Ok(workflow_name.to_string())
}

async fn create_batch(
    State(state): State<AppState>,
    Json(payload): Json<BatchRequest>,
) -> Result<(StatusCode, Json<BatchResponse>), AppError> {
    if payload.file_paths.is_empty() {
        return Err(AppError::BadRequest(
            "file_paths must not be empty".to_string(),
        ));
    }

    let base_workflow: serde_json::Value = payload.workflow;
    let workflow_name = workflow_name_from_request(&base_workflow, DEFAULT_WORKFLOW_NAME_API_BATCH);

    let mut job_ids = Vec::with_capacity(payload.file_paths.len());

    for file_path in &payload.file_paths {
        let mut wf = base_workflow.clone();
        if let Some(nodes) = wf.get_mut("nodes").and_then(|n| n.as_array_mut()) {
            for node in nodes.iter_mut() {
                let node_type = node.get("node_type").and_then(|t| t.as_str());
                match node_type {
                    Some("VideoInput") => {
                        if let Some(params) = node.get_mut("params").and_then(|p| p.as_object_mut())
                        {
                            params.insert(
                                "path".to_string(),
                                serde_json::Value::String(file_path.clone()),
                            );
                        }
                    }
                    Some("WorkflowInput") => {
                        if let Some(params) = node.get_mut("params").and_then(|p| p.as_object_mut())
                        {
                            if let Some(ports_arr) =
                                params.get("ports").and_then(|v| v.as_array()).cloned()
                            {
                                for port in ports_arr {
                                    let port_name = port
                                        .get("name")
                                        .and_then(|n| n.as_str())
                                        .unwrap_or_default();
                                    let port_type = port
                                        .get("port_type")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or_default();

                                    if port_type != "Path" || port_name.is_empty() {
                                        continue;
                                    }

                                    let name_lower = port_name.to_lowercase();
                                    if name_lower.contains("input")
                                        || name_lower == "input"
                                        || name_lower == "path"
                                    {
                                        params.insert(
                                            port_name.to_string(),
                                            serde_json::Value::String(file_path.clone()),
                                        );
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        let workflow: PipelineGraph = parse_and_validate_workflow(&state, wf)?;

        let created = create_and_spawn_job(
            &state,
            workflow,
            None,
            workflow_name.clone(),
            WORKFLOW_SOURCE_API_BATCH.to_string(),
            None,
        )?;
        let id = created.id;

        info!(job_id = %id, file_path = %file_path, "Batch job created");
        job_ids.push(id);
    }

    let total = job_ids.len();
    Ok((StatusCode::CREATED, Json(BatchResponse { job_ids, total })))
}

async fn list_jobs(State(state): State<AppState>) -> Json<Vec<JobResponse>> {
    let jobs: Vec<JobResponse> = state
        .inner
        .jobs
        .iter()
        .map(|entry| job_to_response(entry.value()))
        .collect();
    Json(jobs)
}

async fn get_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<JobResponse>, AppError> {
    let job = state
        .inner
        .jobs
        .get(&id)
        .ok_or_else(|| AppError::NotFound(format!("job not found: {id}")))?;

    Ok(Json(job_to_response(job.value())))
}

async fn rerun_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<CreateJobResponse>), AppError> {
    let (workflow, params, workflow_name, workflow_source) = {
        let source_job = state
            .inner
            .jobs
            .get(&id)
            .ok_or_else(|| AppError::NotFound(format!("job not found: {id}")))?;

        if source_job.status == JobStatus::Completed {
            return Err(AppError::BadRequest(format!(
                "{RERUN_COMPLETED_REJECTION}: {id}"
            )));
        }

        (
            source_job.workflow.clone(),
            source_job.params.clone(),
            source_job.workflow_name.clone(),
            source_job.workflow_source.clone(),
        )
    };

    let created = create_and_spawn_job(
        &state,
        workflow,
        params,
        workflow_name,
        workflow_source,
        Some(id),
    )?;

    Ok((StatusCode::CREATED, Json(created)))
}

async fn delete_job_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let (job_id, job) = state
        .inner
        .jobs
        .remove(&id)
        .ok_or_else(|| AppError::NotFound(format!("job not found: {id}")))?;

    if matches!(job.status, JobStatus::Queued | JobStatus::Running) {
        job.cancel_token.cancel();
    }

    let removed_sender = state.inner.progress_senders.remove(&job_id);

    if let Some(persistence) = &state.inner.jobs_persistence {
        let persisted_deleted_rows = persistence
            .delete_job(&job_id)
            .map_err(|e| AppError::Internal(format!("failed to delete job history row: {e:#}")));

        let persisted_deleted_rows = match persisted_deleted_rows {
            Ok(rows) if rows == 1 => rows,
            Ok(rows) => {
                state.inner.jobs.insert(job_id.clone(), job.clone());
                if let Some((sender_id, sender)) = removed_sender {
                    state.inner.progress_senders.insert(sender_id, sender);
                }
                return Err(AppError::Internal(format!(
                    "expected exactly one persisted row deleted for job {job_id}, deleted {rows}"
                )));
            }
            Err(err) => {
                state.inner.jobs.insert(job_id.clone(), job.clone());
                if let Some((sender_id, sender)) = removed_sender {
                    state.inner.progress_senders.insert(sender_id, sender);
                }
                return Err(err);
            }
        };

        debug_assert_eq!(persisted_deleted_rows, 1);
    }

    info!(job_id = %job_id, "Job history row deleted");
    Ok(StatusCode::NO_CONTENT)
}

async fn job_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    if !state.inner.jobs.contains_key(&id) {
        return Err(AppError::NotFound(format!("job not found: {id}")));
    }

    let rx = state
        .inner
        .progress_senders
        .get(&id)
        .map(|sender| sender.subscribe())
        .ok_or_else(|| AppError::NotFound(format!("no progress channel for job: {id}")))?;

    Ok(ws.on_upgrade(move |socket| handle_ws(socket, rx)))
}

async fn handle_ws(mut socket: WebSocket, mut rx: broadcast::Receiver<JobWsEvent>) {
    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(update) => {
                        let json = match serde_json::to_string(&update) {
                            Ok(j) => j,
                            Err(_) => break,
                        };
                        if socket.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket receiver lagged by {n} messages");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

async fn list_nodes() -> Json<Vec<NodeDescriptor>> {
    Json(all_node_descriptors())
}

async fn list_models(State(state): State<AppState>) -> Json<Vec<ModelEntry>> {
    let models = state.inner.model_registry.list().to_vec();
    Json(models)
}

async fn inspect_model(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> Result<Json<ModelInspection>, AppError> {
    model_inspect::sanitize_model_filename(&filename)
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let config = state.inner.config.read().await;
    let models_dir = &config.paths.models_dir;
    let path = models_dir.join(&filename);

    if !path.exists() {
        return Err(AppError::NotFound(format!("model not found: {filename}")));
    }

    let inspection = tokio::task::spawn_blocking(move || model_inspect::inspect_onnx(&path))
        .await
        .map_err(|e| AppError::Internal(format!("task join error: {e}")))?
        .map_err(|e| AppError::Internal(format!("failed to inspect model: {e}")))?;

    Ok(Json(inspection))
}

async fn list_presets(State(state): State<AppState>) -> Json<Vec<PresetResponse>> {
    let presets: Vec<PresetResponse> = state
        .inner
        .presets
        .iter()
        .map(|entry| PresetResponse {
            id: entry.key().clone(),
            name: entry.value().name.clone(),
            description: entry.value().description.clone(),
            workflow: entry.value().workflow.clone(),
        })
        .collect();
    Json(presets)
}

async fn create_preset(
    State(state): State<AppState>,
    Json(payload): Json<CreatePresetRequest>,
) -> (StatusCode, Json<PresetResponse>) {
    let id = Uuid::new_v4().to_string();
    let preset = Preset {
        name: payload.name,
        description: payload.description,
        workflow: payload.workflow,
    };

    let response = PresetResponse {
        id: id.clone(),
        name: preset.name.clone(),
        description: preset.description.clone(),
        workflow: preset.workflow.clone(),
    };

    state.inner.presets.insert(id, preset);

    (StatusCode::CREATED, Json(response))
}

// ---------------------------------------------------------------------------
// Workflow CRUD (user-saved workflows on disk)
// ---------------------------------------------------------------------------

/// Sanitize a workflow filename: reject path separators, `..`, and empty names.
fn sanitize_workflow_filename(filename: &str) -> Result<(), AppError> {
    let trimmed = filename.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest("filename must not be empty".into()));
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err(AppError::BadRequest(
            "filename must not contain path separators".into(),
        ));
    }
    if trimmed.contains("..") {
        return Err(AppError::BadRequest(
            "filename must not contain '..'".into(),
        ));
    }
    Ok(())
}

async fn list_workflows(State(state): State<AppState>) -> Json<Vec<WorkflowEntry>> {
    let dir = state.resolve_workflows_dir().await;

    let mut entries = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if !path.extension().is_some_and(|e| e == "json") {
                continue;
            }
            if let Ok(contents) = std::fs::read_to_string(&path) {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&contents) {
                    let name = parsed
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let description = parsed
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let workflow = parsed.get("workflow").cloned().unwrap_or_default();
                    let has_interface = workflow
                        .get("interface")
                        .and_then(|i| i.get("inputs"))
                        .and_then(|arr| arr.as_array())
                        .is_some_and(|a| !a.is_empty());
                    let filename = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    entries.push(WorkflowEntry {
                        filename,
                        name,
                        description,
                        workflow,
                        has_interface,
                    });
                }
            }
        }
    }
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Json(entries)
}

async fn save_workflow(
    State(state): State<AppState>,
    Json(payload): Json<SaveWorkflowRequest>,
) -> Result<(StatusCode, Json<WorkflowEntry>), AppError> {
    let trimmed = payload.name.trim().to_string();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest(
            "workflow name must not be empty".into(),
        ));
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err(AppError::BadRequest(
            "workflow name must not contain path separators".into(),
        ));
    }
    if trimmed.contains("..") {
        return Err(AppError::BadRequest(
            "workflow name must not contain '..'".into(),
        ));
    }

    let filename = if trimmed.ends_with(".json") {
        trimmed.clone()
    } else {
        format!("{trimmed}.json")
    };

    sanitize_workflow_filename(&filename)?;

    let dir = state.resolve_workflows_dir().await;

    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Internal(format!("failed to create workflows dir: {e}")))?;

    let doc = serde_json::json!({
        "name": trimmed,
        "description": payload.description,
        "workflow": payload.workflow,
    });

    let path = dir.join(&filename);
    let bytes = serde_json::to_vec_pretty(&doc)
        .map_err(|e| AppError::Internal(format!("failed to serialize workflow: {e}")))?;
    std::fs::write(&path, bytes)
        .map_err(|e| AppError::Internal(format!("failed to write workflow file: {e}")))?;

    let has_interface = payload
        .workflow
        .get("interface")
        .and_then(|i| i.get("inputs"))
        .and_then(|arr| arr.as_array())
        .is_some_and(|a| !a.is_empty());

    Ok((
        StatusCode::CREATED,
        Json(WorkflowEntry {
            filename,
            name: trimmed,
            description: payload.description,
            workflow: payload.workflow,
            has_interface,
        }),
    ))
}

async fn delete_workflow(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> Result<StatusCode, AppError> {
    sanitize_workflow_filename(&filename)?;

    if !filename.ends_with(".json") {
        return Err(AppError::BadRequest(
            "only .json workflow files can be deleted".into(),
        ));
    }

    let dir = state.resolve_workflows_dir().await;
    let path = dir.join(&filename);

    if !path.exists() {
        return Err(AppError::NotFound(format!(
            "workflow not found: {filename}"
        )));
    }

    std::fs::remove_file(&path)
        .map_err(|e| AppError::Internal(format!("failed to delete workflow: {e}")))?;

    Ok(StatusCode::NO_CONTENT)
}

async fn get_workflow_interface(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    sanitize_workflow_filename(&filename)?;

    let workflows_dir = state.resolve_workflows_dir().await;
    let config = state.inner.config.read().await;
    let workflows_path = workflows_dir.join(&filename);
    let presets_path = config.paths.presets_dir.join(&filename);

    let contents = if workflows_path.exists() {
        std::fs::read_to_string(&workflows_path)
    } else if presets_path.exists() {
        std::fs::read_to_string(&presets_path)
    } else {
        return Err(AppError::NotFound(format!(
            "workflow not found: {filename}"
        )));
    };

    let contents =
        contents.map_err(|e| AppError::Internal(format!("failed to read workflow: {e}")))?;
    let parsed: serde_json::Value = serde_json::from_str(&contents)
        .map_err(|e| AppError::BadRequest(format!("invalid JSON: {e}")))?;

    let workflow = parsed.get("workflow").unwrap_or(&parsed);
    let interface = workflow
        .get("interface")
        .cloned()
        .unwrap_or(serde_json::json!({"inputs": [], "outputs": []}));

    Ok(Json(interface))
}

async fn list_fs(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<FsListQuery>,
) -> Result<Json<Vec<FsEntry>>, AppError> {
    let workflows_resolved = state.resolve_workflows_dir().await;
    let config = state.inner.config.read().await;

    let base_name = params.base.as_deref().unwrap_or("models");
    let base_dir: PathBuf = match base_name {
        "models" => config.paths.models_dir.clone(),
        "presets" => config.paths.presets_dir.clone(),
        "workflows" => workflows_resolved,
        _ => {
            return Err(AppError::Forbidden(format!(
                "unknown base directory: {base_name}"
            )));
        }
    };

    if !base_dir.exists() {
        return Ok(Json(vec![]));
    }

    let canonical_base = base_dir.canonicalize().map_err(|e| {
        AppError::Internal(format!(
            "failed to canonicalize base dir {}: {e}",
            base_dir.display()
        ))
    })?;

    let (list_dir, name_filter) = if let Some(ref prefix) = params.prefix {
        let joined = canonical_base.join(prefix);
        if joined.is_dir() {
            (joined, None)
        } else {
            let parent = joined.parent().unwrap_or(&canonical_base).to_path_buf();
            let filter = joined.file_name().map(|n| n.to_string_lossy().to_string());
            (parent, filter)
        }
    } else {
        (canonical_base.clone(), None)
    };

    if !list_dir.exists() {
        return Ok(Json(vec![]));
    }

    let canonical_list = list_dir.canonicalize().map_err(|e| {
        AppError::Internal(format!(
            "failed to canonicalize list dir {}: {e}",
            list_dir.display()
        ))
    })?;

    // SECURITY: reject paths that escape the sandboxed base directory
    if !canonical_list.starts_with(&canonical_base) {
        return Err(AppError::Forbidden("path traversal detected".to_string()));
    }

    let read_dir = match std::fs::read_dir(&canonical_list) {
        Ok(rd) => rd,
        Err(_) => return Ok(Json(vec![])),
    };

    let mut entries: Vec<FsEntry> = Vec::new();
    for entry in read_dir.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();

        if file_name.starts_with('.') {
            continue;
        }

        if let Some(ref filter) = name_filter {
            if !file_name.starts_with(filter.as_str()) {
                continue;
            }
        }

        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);

        let rel = canonical_list
            .join(&file_name)
            .strip_prefix(&canonical_base)
            .unwrap_or(StdPath::new(&file_name))
            .to_string_lossy()
            .to_string();

        let display_path = format!("{base_name}/{rel}");

        entries.push(FsEntry {
            name: file_name,
            is_dir,
            path: display_path,
        });
    }

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(Json(entries))
}

async fn browse_fs(
    axum::extract::Query(params): axum::extract::Query<FsBrowseQuery>,
) -> Result<Json<Vec<FsEntry>>, AppError> {
    let raw_path = params
        .path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(".");

    let resolved_path = if raw_path.starts_with('~') {
        #[cfg(unix)]
        let home = std::env::var("HOME").unwrap_or_default();
        #[cfg(windows)]
        let home = std::env::var("USERPROFILE").unwrap_or_default();
        format!("{home}{}", &raw_path[1..])
    } else {
        raw_path.to_string()
    };

    let browse_dir = PathBuf::from(resolved_path);
    if !browse_dir.exists() || !browse_dir.is_dir() {
        return Ok(Json(vec![]));
    }

    let canonical_browse = browse_dir.canonicalize().map_err(|e| {
        AppError::Internal(format!(
            "failed to canonicalize browse dir {}: {e}",
            browse_dir.display()
        ))
    })?;

    #[cfg(unix)]
    {
        if canonical_browse.starts_with(StdPath::new("/proc"))
            || canonical_browse.starts_with(StdPath::new("/sys"))
            || canonical_browse.starts_with(StdPath::new("/dev"))
        {
            return Err(AppError::Forbidden(
                "browsing this directory is not allowed".to_string(),
            ));
        }
    }

    let read_dir = match std::fs::read_dir(&canonical_browse) {
        Ok(rd) => rd,
        Err(_) => return Ok(Json(vec![])),
    };

    let mut entries: Vec<FsEntry> = Vec::new();
    for entry in read_dir.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();

        if file_name.starts_with('.') {
            continue;
        }

        let canonical_entry = match entry.path().canonicalize() {
            Ok(path) => path,
            Err(_) => continue,
        };

        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);

        entries.push(FsEntry {
            name: file_name,
            is_dir,
            path: canonical_entry.to_string_lossy().to_string(),
        });
    }

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries.truncate(200);

    Ok(Json(entries))
}

async fn extract_frames(
    State(state): State<AppState>,
    Json(payload): Json<ExtractFramesRequest>,
) -> Result<(StatusCode, Json<ExtractFramesResponse>), AppError> {
    if payload.count == 0 || payload.count > 100 {
        return Err(AppError::BadRequest(
            "count must be between 1 and 100".to_string(),
        ));
    }

    let video_path = StdPath::new(&payload.video_path);
    if !video_path.exists() {
        return Err(AppError::BadRequest(format!(
            "video file not found: {}",
            payload.video_path
        )));
    }

    let preview_id = Uuid::new_v4().to_string();
    let temp_dir = std::env::temp_dir().join(format!("videnoa-preview-{preview_id}"));
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| AppError::Internal(format!("failed to create temp dir: {e}")))?;

    let probe = crate::runtime::command_for("ffprobe")
        .args([
            "-v",
            "error",
            "-count_frames",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=nb_read_frames",
            "-of",
            "csv=p=0",
            &payload.video_path,
        ])
        .output()
        .map_err(|e| AppError::Internal(format!("ffprobe failed: {e}")))?;

    let total_frames: u64 = String::from_utf8_lossy(&probe.stdout)
        .trim()
        .parse()
        .unwrap_or(1000);
    let interval = (total_frames / payload.count as u64).max(1);

    let output_pattern = temp_dir.join("frame_%04d.png");
    let status = crate::runtime::command_for("ffmpeg")
        .args([
            "-i",
            &payload.video_path,
            "-vf",
            &format!("select='not(mod(n\\,{interval}))'"),
            "-frames:v",
            &payload.count.to_string(),
            "-vsync",
            "vfn",
            output_pattern
                .to_str()
                .ok_or_else(|| AppError::Internal("invalid path encoding".to_string()))?,
        ])
        .output()
        .map_err(|e| AppError::Internal(format!("ffmpeg failed: {e}")))?;

    if !status.status.success() {
        let stderr = String::from_utf8_lossy(&status.stderr);
        return Err(AppError::Internal(format!("ffmpeg error: {stderr}")));
    }

    let mut frames = Vec::new();
    for i in 1..=payload.count {
        let filename = format!("frame_{i:04}.png");
        let frame_path = temp_dir.join(&filename);
        if frame_path.exists() {
            frames.push(FrameInfo {
                index: i - 1,
                url: format!("/api/preview/frames/{preview_id}/{filename}"),
            });
        }
    }

    if frames.is_empty() {
        return Err(AppError::Internal("ffmpeg produced no frames".to_string()));
    }

    state
        .inner
        .preview_sessions
        .insert(preview_id.clone(), temp_dir);

    info!(preview_id = %preview_id, frame_count = frames.len(), "Extracted preview frames");

    Ok((
        StatusCode::CREATED,
        Json(ExtractFramesResponse { preview_id, frames }),
    ))
}

async fn serve_preview_frame(
    State(state): State<AppState>,
    Path((preview_id, filename)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let session_dir = state
        .inner
        .preview_sessions
        .get(&preview_id)
        .ok_or_else(|| AppError::NotFound(format!("preview session not found: {preview_id}")))?;

    let file_path = session_dir.join(&filename);
    if !file_path.exists() {
        return Err(AppError::NotFound(format!("frame not found: {filename}")));
    }

    let bytes = tokio::fs::read(&file_path)
        .await
        .map_err(|e| AppError::Internal(format!("failed to read frame: {e}")))?;

    Ok((StatusCode::OK, [("content-type", "image/png")], bytes).into_response())
}

async fn process_frame(
    State(state): State<AppState>,
    Json(payload): Json<ProcessFrameRequest>,
) -> Result<Json<ProcessFrameResponse>, AppError> {
    let session_dir = state
        .inner
        .preview_sessions
        .get(&payload.preview_id)
        .ok_or_else(|| {
            AppError::NotFound(format!("preview session not found: {}", payload.preview_id))
        })?;

    let filename = format!("frame_{:04}.png", payload.frame_index + 1);
    let frame_path = session_dir.join(&filename);
    if !frame_path.exists() {
        return Err(AppError::NotFound(format!(
            "frame not found: index {}",
            payload.frame_index
        )));
    }

    // TODO(task 4.3): actual frame processing through inference pipeline
    let processed_url = format!("/api/preview/frames/{}/{}", payload.preview_id, filename);

    Ok(Json(ProcessFrameResponse { processed_url }))
}

#[derive(Deserialize)]
pub struct JellyfinProxyQuery {
    pub url: String,
    pub api_key: String,
    pub library_id: Option<String>,
}

async fn jellyfin_libraries(
    axum::extract::Query(params): axum::extract::Query<JellyfinProxyQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let client = JellyfinClient::new(&params.url, &params.api_key)
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let libraries = client
        .get_libraries()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::to_value(libraries).unwrap_or_default()))
}

async fn jellyfin_items(
    axum::extract::Query(params): axum::extract::Query<JellyfinProxyQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let client = JellyfinClient::new(&params.url, &params.api_key)
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let query = ItemQuery {
        parent_id: params.library_id,
        include_item_types: Some("Movie,Episode".to_string()),
        fields: Some("Path,Overview".to_string()),
        recursive: Some(true),
        ..Default::default()
    };

    let items = client
        .get_items(&query)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::to_value(items).unwrap_or_default()))
}

async fn run_job(state: AppState, job_id: String) {
    let _permit = {
        let cancel_token = {
            let job = match state.inner.jobs.get(&job_id) {
                Some(j) => j,
                None => return,
            };
            job.cancel_token.clone()
        };

        tokio::select! {
            permit = state.inner.gpu_semaphore.clone().acquire_owned() => {
                match permit {
                    Ok(p) => p,
                    Err(_) => return,
                }
            }
            _ = cancel_token.cancelled() => {
                return;
            }
        }
    };

    let running_snapshot = {
        if let Some(mut job) = state.inner.jobs.get_mut(&job_id) {
            if job.status == JobStatus::Cancelled {
                return;
            }
            job.status = JobStatus::Running;
            job.started_at = Some(Utc::now());
            Some(job.clone())
        } else {
            None
        }
    };

    if let Some(snapshot) = running_snapshot {
        if let Err(err) = state.persist_job_snapshot(&snapshot) {
            error!(job_id = %job_id, error = ?err, "Failed to persist running transition");
        }
    }

    let result = {
        let (mut workflow, mut job_params, cancel_token) = {
            let Some(job) = state.inner.jobs.get(&job_id) else {
                return;
            };
            (
                job.workflow.clone(),
                job.params.clone(),
                job.cancel_token.clone(),
            )
        };
        let inner = Arc::clone(&state.inner);
        let trt_cache_dir = state.inner.config.read().await.paths.trt_cache_dir.clone();

        // Clone the broadcast sender before entering the blocking closure
        // to avoid holding the DashMap read lock across the block_in_place boundary.
        let ws_tx = state.inner.progress_senders.get(&job_id).map(|r| r.clone());

        let job_id_for_closure = job_id.clone();

        if workflow.has_video_frames_edges() {
            if let Some(params) = job_params.as_ref() {
                workflow.inject_workflow_input_params(params);
            }
            job_params = None;
        }

        if let Some(params) = job_params {
            tokio::task::block_in_place(move || {
                let mut debug_throttle =
                    NodeDebugEventThrottle::new(Duration::from_millis(PRINT_PREVIEW_THROTTLE_MS));
                let ws_tx_for_debug = ws_tx.clone();
                let mut node_debug_cb = move |event: NodeDebugValueEvent| {
                    if !debug_throttle.should_emit(&event.node_id, Instant::now()) {
                        return;
                    }
                    if let Some(tx) = &ws_tx_for_debug {
                        let _ = tx.send(JobWsEvent::from(event));
                    }
                };

                // Convert JSON params to PortData (infer type from JSON value)
                let mut port_params = HashMap::new();
                for (key, value) in &params {
                    let port_data = if let Some(i) = value.as_i64() {
                        crate::types::PortData::Int(i)
                    } else if let Some(f) = value.as_f64() {
                        crate::types::PortData::Float(f)
                    } else if let Some(b) = value.as_bool() {
                        crate::types::PortData::Bool(b)
                    } else if let Some(s) = value.as_str() {
                        crate::types::PortData::Str(s.to_string())
                    } else {
                        crate::types::PortData::Str(value.to_string())
                    };
                    port_params.insert(key.clone(), port_data);
                }
                let ctx = crate::node::ExecutionContext::default();
                SequentialExecutor::execute_with_params_and_debug_hook(
                    &workflow,
                    &inner.node_registry,
                    port_params,
                    &ctx,
                    Some(&mut node_debug_cb),
                )
            })
        } else {
            // No params: use execute_with_context with video compile support
            // Use block_in_place (NOT spawn_blocking) because the executor internally
            // calls block_in_place at executor.rs:67. Nesting block_in_place inside
            // spawn_blocking panics; block_in_place inside block_in_place is a no-op.
            tokio::task::block_in_place(move || {
                let compile_ctx = VideoCompileContext::new(trt_cache_dir);
                let fps_baseline = Mutex::new(None::<ProgressFpsBaseline>);
                let ws_tx_for_progress = ws_tx.clone();
                let ws_tx_for_debug = ws_tx.clone();

                let inner_for_cb = Arc::clone(&inner);
                let progress_cb: Box<dyn Fn(u64, Option<u64>, Option<u64>) + Send> =
                    Box::new(move |current_frame, total_frames, _hint| {
                        let now = Instant::now();
                        let fps = {
                            let mut baseline_guard = match fps_baseline.lock() {
                                Ok(guard) => guard,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                            let (next_baseline, next_fps) = estimate_input_fps_from_second_frame(
                                *baseline_guard,
                                current_frame,
                                now,
                            );
                            *baseline_guard = next_baseline;
                            next_fps as f64
                        };
                        let eta = total_frames.and_then(|total| {
                            if fps > 0.0 && current_frame < total {
                                Some((total - current_frame) as f64 / fps)
                            } else {
                                None
                            }
                        });

                        let update = ProgressUpdate {
                            current_frame,
                            total_frames,
                            fps: fps as f32,
                            eta_seconds: eta,
                        };

                        if let Some(mut job) = inner_for_cb.jobs.get_mut(&job_id_for_closure) {
                            job.progress = Some(update.clone());
                        }

                        if let Some(tx) = &ws_tx_for_progress {
                            let _ = tx.send(JobWsEvent::from(update));
                        }
                    });

                let mut debug_throttle =
                    NodeDebugEventThrottle::new(Duration::from_millis(PRINT_PREVIEW_THROTTLE_MS));
                let mut node_debug_cb = move |event: NodeDebugValueEvent| {
                    if !debug_throttle.should_emit(&event.node_id, Instant::now()) {
                        return;
                    }
                    if let Some(tx) = &ws_tx_for_debug {
                        let _ = tx.send(JobWsEvent::from(event));
                    }
                };

                let (cancel_watch_tx, cancel_watch_rx) = tokio::sync::watch::channel(false);
                let _cancel_bridge = tokio::spawn({
                    let token = cancel_token.clone();
                    async move {
                        token.cancelled().await;
                        let _ = cancel_watch_tx.send(true);
                    }
                });

                SequentialExecutor::execute_with_context_and_debug_hook(
                    &workflow,
                    &inner.node_registry,
                    Some(&compile_ctx),
                    Some(progress_cb),
                    Some(cancel_watch_rx),
                    Some(&mut node_debug_cb),
                )
            })
        }
    };

    match result {
        Ok(_outputs) => {
            let mut completed_snapshot = None;
            if let Some(mut job) = state.inner.jobs.get_mut(&job_id) {
                if job.status == JobStatus::Cancelled {
                    return;
                }
                job.status = JobStatus::Completed;
                job.completed_at = Some(Utc::now());
                completed_snapshot = Some(job.clone());
            }

            if let Some(snapshot) = completed_snapshot {
                if let Err(err) = state.persist_job_snapshot(&snapshot) {
                    error!(job_id = %job_id, error = ?err, "Failed to persist completed transition");
                }
            }
        }
        Err(err) => {
            error!(job_id = %job_id, error = ?err, "Job execution failed");
            let mut failed_snapshot = None;
            if let Some(mut job) = state.inner.jobs.get_mut(&job_id) {
                if job.status == JobStatus::Cancelled {
                    return;
                }
                job.status = JobStatus::Failed;
                job.error = Some(format!("{:#}", err));
                job.completed_at = Some(Utc::now());
                failed_snapshot = Some(job.clone());
            }

            if let Some(snapshot) = failed_snapshot {
                if let Err(persist_err) = state.persist_job_snapshot(&snapshot) {
                    error!(
                        job_id = %job_id,
                        error = ?persist_err,
                        "Failed to persist failed transition"
                    );
                }
            }
        }
    }

    state.inner.progress_senders.remove(&job_id);

    info!(job_id = %job_id, "Job completed");
}

#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    Forbidden(String),
    NotFound(String),
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AppError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            AppError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };

        let body = Json(ErrorResponse { error: message });
        (status, body).into_response()
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        AppError::Internal(format!("{:#}", err))
    }
}

fn job_to_response(job: &Job) -> JobResponse {
    JobResponse {
        id: job.id.clone(),
        status: job.status,
        created_at: job.created_at,
        started_at: job.started_at,
        completed_at: job.completed_at,
        progress: job.progress.clone(),
        error: job.error.clone(),
        workflow_name: job.workflow_name.clone(),
        workflow_source: job.workflow_source.clone(),
        params: job.params.clone(),
        rerun_of_job_id: job.rerun_of_job_id.clone(),
        duration_ms: job_duration_ms(job),
    }
}

fn workflow_name_from_request(workflow: &serde_json::Value, fallback: &str) -> String {
    workflow
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn extract_workflow_input_params(
    workflow: &serde_json::Value,
) -> Option<HashMap<String, serde_json::Value>> {
    let nodes = workflow.get("nodes")?.as_array()?;
    let workflow_input = nodes
        .iter()
        .find(|node| node.get("node_type").and_then(|t| t.as_str()) == Some("WorkflowInput"))?;
    let params = workflow_input.get("params")?.as_object()?;

    let mut extracted = HashMap::new();
    for (key, value) in params {
        if matches!(
            key.as_str(),
            "ports" | "interface_inputs" | "interface_outputs"
        ) {
            continue;
        }

        extracted.insert(key.clone(), value.clone());
    }

    if extracted.is_empty() {
        None
    } else {
        Some(extracted)
    }
}

fn job_duration_ms(job: &Job) -> Option<i64> {
    let completed_at = job.completed_at?;
    let started_at = job.started_at.unwrap_or(job.created_at);
    Some((completed_at - started_at).num_milliseconds().max(0))
}

pub fn default_app_state() -> AppState {
    let dd = crate::config::data_dir(None);
    let cfg_path = crate::config::config_path(&dd);
    let config = match AppConfig::load_from_path(&cfg_path) {
        Ok(config) => config,
        Err(err) => {
            warn!(error = %err, "Failed to load config file, using defaults");
            AppConfig::default()
        }
    };
    app_state_with_config(config, cfg_path, dd)
}

pub fn app_state_with_config(
    config: AppConfig,
    config_path: PathBuf,
    data_dir: PathBuf,
) -> AppState {
    let mut node_registry = NodeRegistry::new();
    register_all_nodes(&mut node_registry);
    let mut model_registry = ModelRegistry::with_builtin_models(config.paths.models_dir.clone());
    if let Err(e) = model_registry.discover() {
        tracing::warn!(error = %e, "Failed to discover models on disk");
    }
    let presets = load_builtin_presets(&config.paths.presets_dir);
    AppState::new(
        node_registry,
        model_registry,
        presets,
        config,
        config_path,
        data_dir,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug_event::NodeDebugValueEvent;
    use crate::types::PortType;
    use axum::body::Body;
    use axum::http::Request;
    use rusqlite::Connection;
    use tower::{Service, ServiceExt};

    fn test_state() -> AppState {
        test_state_with_data_dir(test_data_dir())
    }

    fn test_state_with_data_dir(data_dir: PathBuf) -> AppState {
        let mut node_registry = NodeRegistry::new();
        node_registry.register("test_source", |_params| {
            Ok(Box::new(TestNode {
                node_type: "test_source".to_string(),
                inputs: vec![],
                outputs: vec![crate::node::PortDefinition {
                    name: "output".to_string(),
                    port_type: PortType::VideoFrames,
                    required: true,
                    default_value: None,
                }],
            }))
        });
        node_registry.register("test_sink", |_params| {
            Ok(Box::new(TestNode {
                node_type: "test_sink".to_string(),
                inputs: vec![crate::node::PortDefinition {
                    name: "input".to_string(),
                    port_type: PortType::VideoFrames,
                    required: true,
                    default_value: None,
                }],
                outputs: vec![],
            }))
        });
        node_registry.register("test_delay", |params| {
            let sleep_ms = params
                .get("sleep_ms")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            Ok(Box::new(DelayNode { sleep_ms }))
        });

        let model_registry = ModelRegistry::with_builtin_models(test_models_dir());
        AppState::new(
            node_registry,
            model_registry,
            DashMap::new(),
            AppConfig::default(),
            test_config_path(),
            data_dir,
        )
    }

    fn test_config_path() -> PathBuf {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "videnoa-core-server-test-{}-{timestamp}.toml",
            std::process::id()
        ))
    }

    fn test_models_dir() -> PathBuf {
        std::env::temp_dir().join("models")
    }

    fn test_data_dir() -> PathBuf {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "videnoa-test-data-{}-{timestamp}",
            std::process::id()
        ))
    }

    fn temp_path(path: &str) -> PathBuf {
        std::env::temp_dir().join(path)
    }

    fn temp_path_str(path: &str) -> String {
        temp_path(path).to_string_lossy().to_string()
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{timestamp}", std::process::id()))
    }

    fn write_json_file(path: &StdPath, value: &serde_json::Value) {
        let bytes = serde_json::to_vec_pretty(value).expect("serialize test workflow JSON");
        std::fs::write(path, bytes).expect("write test workflow JSON");
    }

    async fn set_workflow_lookup_dirs(
        state: &AppState,
        workflows_dir: PathBuf,
        presets_dir: PathBuf,
    ) {
        let mut config = state.inner.config.write().await;
        config.paths.workflows_dir = workflows_dir;
        config.paths.presets_dir = presets_dir;
    }

    fn test_router() -> Router {
        app_router(test_state())
    }

    fn valid_workflow_json() -> serde_json::Value {
        serde_json::json!({
            "nodes": [
                {"id": "src", "node_type": "test_source", "params": {}},
                {"id": "dst", "node_type": "test_sink", "params": {}}
            ],
            "connections": [
                {
                    "from_node": "src",
                    "from_port": "output",
                    "to_node": "dst",
                    "to_port": "input",
                    "port_type": "VideoFrames"
                }
            ]
        })
    }

    fn delay_workflow_json(sleep_ms: u64) -> serde_json::Value {
        serde_json::json!({
            "nodes": [
                {
                    "id": "delay",
                    "node_type": "test_delay",
                    "params": {
                        "sleep_ms": sleep_ms
                    }
                }
            ],
            "connections": []
        })
    }

    fn workflow_input_output_json() -> serde_json::Value {
        serde_json::json!({
            "nodes": [
                {"id": "wi", "node_type": "WorkflowInput", "params": {
                    "ports": [{"name": "greeting", "port_type": "Str"}]
                }},
                {"id": "wo", "node_type": "WorkflowOutput", "params": {
                    "ports": [{"name": "greeting", "port_type": "Str"}]
                }}
            ],
            "connections": [
                {
                    "from_node": "wi",
                    "from_port": "greeting",
                    "to_node": "wo",
                    "to_port": "greeting",
                    "port_type": "Str"
                }
            ],
            "interface": {
                "inputs": [{"name": "greeting", "port_type": "Str"}],
                "outputs": [{"name": "greeting", "port_type": "Str"}]
            }
        })
    }

    fn persisted_job_status(data_dir: &StdPath, job_id: &str) -> Option<String> {
        let db_path = data_dir.join("jobs.db");
        let conn = Connection::open(db_path).ok()?;
        conn.query_row(
            "SELECT status FROM jobs WHERE id = ?1",
            rusqlite::params![job_id],
            |row| row.get(0),
        )
        .ok()
    }

    fn build_test_job(
        id: String,
        status: JobStatus,
        params: Option<HashMap<String, serde_json::Value>>,
    ) -> Job {
        let workflow: PipelineGraph =
            serde_json::from_value(valid_workflow_json()).expect("workflow should deserialize");
        let created_at = Utc::now() - chrono::Duration::seconds(5);
        let started_at = if status == JobStatus::Queued {
            None
        } else {
            Some(created_at + chrono::Duration::seconds(1))
        };
        let completed_at = if matches!(
            status,
            JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled
        ) {
            Some(created_at + chrono::Duration::seconds(2))
        } else {
            None
        };
        let error = match status {
            JobStatus::Failed => Some("source failed".to_string()),
            JobStatus::Cancelled => Some("source cancelled".to_string()),
            _ => None,
        };

        Job {
            id,
            status,
            workflow,
            created_at,
            started_at,
            completed_at,
            progress: None,
            error,
            cancel_token: CancellationToken::new(),
            params,
            workflow_name: "Source Workflow".to_string(),
            workflow_source: WORKFLOW_SOURCE_API_JOBS.to_string(),
            rerun_of_job_id: None,
        }
    }

    fn insert_test_job(state: &AppState, job: Job) {
        state
            .persist_job_snapshot(&job)
            .expect("persist source job snapshot");
        state.inner.jobs.insert(job.id.clone(), job);
    }

    struct TestNode {
        node_type: String,
        inputs: Vec<crate::node::PortDefinition>,
        outputs: Vec<crate::node::PortDefinition>,
    }

    struct DelayNode {
        sleep_ms: u64,
    }

    impl crate::node::Node for TestNode {
        fn node_type(&self) -> &str {
            &self.node_type
        }
        fn input_ports(&self) -> Vec<crate::node::PortDefinition> {
            self.inputs.clone()
        }
        fn output_ports(&self) -> Vec<crate::node::PortDefinition> {
            self.outputs.clone()
        }
        fn execute(
            &mut self,
            _inputs: &std::collections::HashMap<String, crate::types::PortData>,
            _ctx: &crate::node::ExecutionContext,
        ) -> Result<std::collections::HashMap<String, crate::types::PortData>> {
            Ok(std::collections::HashMap::new())
        }
    }

    impl crate::node::Node for DelayNode {
        fn node_type(&self) -> &str {
            "test_delay"
        }

        fn input_ports(&self) -> Vec<crate::node::PortDefinition> {
            vec![]
        }

        fn output_ports(&self) -> Vec<crate::node::PortDefinition> {
            vec![]
        }

        fn execute(
            &mut self,
            _inputs: &std::collections::HashMap<String, crate::types::PortData>,
            _ctx: &crate::node::ExecutionContext,
        ) -> Result<std::collections::HashMap<String, crate::types::PortData>> {
            if self.sleep_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(self.sleep_ms));
            }
            Ok(std::collections::HashMap::new())
        }
    }

    async fn send_request(router: &mut Router, request: Request<Body>) -> axum::response::Response {
        router
            .as_service()
            .ready()
            .await
            .unwrap()
            .call(request)
            .await
            .unwrap()
    }

    async fn wait_for_job_terminal_status(state: &AppState, job_id: &str) -> JobStatus {
        const MAX_POLLS: usize = 80;
        const POLL_INTERVAL_MS: u64 = 50;

        for _ in 0..MAX_POLLS {
            if let Some(job) = state.inner.jobs.get(job_id) {
                if matches!(
                    job.status,
                    JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled
                ) {
                    return job.status;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS)).await;
        }

        panic!("job {job_id} did not reach terminal status within timeout");
    }

    async fn wait_for_persisted_status(data_dir: &StdPath, job_id: &str, expected: &str) -> bool {
        const MAX_POLLS: usize = 80;
        const POLL_INTERVAL_MS: u64 = 25;

        for _ in 0..MAX_POLLS {
            if persisted_job_status(data_dir, job_id).as_deref() == Some(expected) {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS)).await;
        }

        false
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let mut app = test_router();
        let req = Request::builder()
            .uri("/api/health")
            .body(Body::empty())
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
    }

    #[tokio::test]
    async fn test_get_config_endpoint() {
        let mut app = test_router();
        let req = Request::builder()
            .uri("/api/config")
            .body(Body::empty())
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let config: AppConfig = serde_json::from_slice(&body).unwrap();

        assert_eq!(config.paths.models_dir, PathBuf::from("models"));
        assert_eq!(config.server.port, 3000);
    }

    #[tokio::test]
    async fn test_put_config_endpoint() {
        let state = test_state();
        let config_path = state.inner.config_path.clone();
        let mut app = app_router(state);

        let updated = AppConfig {
            paths: crate::config::PathsConfig {
                models_dir: PathBuf::from("models_custom"),
                trt_cache_dir: PathBuf::from("cache_custom"),
                presets_dir: PathBuf::from("presets_custom"),
                workflows_dir: PathBuf::from("workflows_custom"),
            },
            server: crate::config::ServerConfig {
                port: 4321,
                host: "127.0.0.1".to_string(),
            },
            locale: "zh-CN".to_string(),
            performance: crate::config::PerformanceConfig {
                profiling_enabled: true,
            },
        };

        let req = Request::builder()
            .method("PUT")
            .uri("/api/config")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&updated).unwrap()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let returned: AppConfig = serde_json::from_slice(&body).unwrap();
        assert_eq!(returned, updated);

        let req = Request::builder()
            .uri("/api/config")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let reloaded: AppConfig = serde_json::from_slice(&body).unwrap();
        assert_eq!(reloaded, updated);

        assert!(config_path.exists());
        let _ = std::fs::remove_file(config_path);
    }

    #[tokio::test]
    async fn test_create_job_valid() {
        let mut app = test_router();
        let body = serde_json::json!({
            "workflow": valid_workflow_json()
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/jobs")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["id"].is_string());
        assert_eq!(json["status"], "queued");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_create_job_video_pipeline() {
        let mut node_registry = NodeRegistry::new();
        register_all_nodes(&mut node_registry);
        let model_registry = ModelRegistry::with_builtin_models(test_models_dir());
        let state = AppState::new(
            node_registry,
            model_registry,
            DashMap::new(),
            AppConfig::default(),
            test_config_path(),
            test_data_dir(),
        );
        let mut app = app_router(state.clone());

        let workflow = serde_json::json!({
            "nodes": [
                {"id": "input", "node_type": "VideoInput", "params": {
                    "path": temp_path_str("nonexistent-video-videnoa-test.mkv")
                }},
                {"id": "output", "node_type": "VideoOutput", "params": {}}
            ],
            "connections": [
                {
                    "from_node": "input",
                    "from_port": "source_path",
                    "to_node": "output",
                    "to_port": "source_path",
                    "port_type": "Path"
                },
                {
                    "from_node": "input",
                    "from_port": "frames",
                    "to_node": "output",
                    "to_port": "frames",
                    "port_type": "VideoFrames"
                }
            ]
        });
        let body = serde_json::json!({ "workflow": workflow });

        let req = Request::builder()
            .method("POST")
            .uri("/api/jobs")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp_body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let create_json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
        let job_id = create_json["id"].as_str().unwrap().to_string();

        let status = wait_for_job_terminal_status(&state, &job_id).await;
        let job = state.inner.jobs.get(&job_id).unwrap();
        assert_eq!(status, JobStatus::Failed);
        let err_msg = job.error.as_deref().unwrap_or("");
        assert!(
            !err_msg.contains("CompileContext"),
            "should not fail due to missing CompileContext, got: {err_msg}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_create_job_with_params() {
        let mut node_registry = NodeRegistry::new();
        register_all_nodes(&mut node_registry);
        let model_registry = ModelRegistry::with_builtin_models(test_models_dir());
        let state = AppState::new(
            node_registry,
            model_registry,
            DashMap::new(),
            AppConfig::default(),
            test_config_path(),
            test_data_dir(),
        );
        let mut app = app_router(state.clone());

        let workflow = workflow_input_output_json();
        let body = serde_json::json!({
            "workflow": workflow,
            "params": {"greeting": "hello world"}
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/jobs")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp_body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let create_json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
        let job_id = create_json["id"].as_str().unwrap().to_string();

        let status = wait_for_job_terminal_status(&state, &job_id).await;

        let job = state.inner.jobs.get(&job_id).unwrap();
        assert_eq!(
            status,
            JobStatus::Completed,
            "expected Completed, got {:?}, error: {:?}",
            job.status,
            job.error
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_create_job_infers_workflow_input_params_when_top_level_params_missing() {
        let mut node_registry = NodeRegistry::new();
        register_all_nodes(&mut node_registry);
        let model_registry = ModelRegistry::with_builtin_models(test_models_dir());
        let state = AppState::new(
            node_registry,
            model_registry,
            DashMap::new(),
            AppConfig::default(),
            test_config_path(),
            test_data_dir(),
        );
        let mut app = app_router(state.clone());

        let body = serde_json::json!({
            "workflow": {
                "nodes": [
                    {"id": "wi", "node_type": "WorkflowInput", "params": {
                        "ports": [{"name": "greeting", "port_type": "Str"}],
                        "greeting": "hello from interface"
                    }},
                    {"id": "wo", "node_type": "WorkflowOutput", "params": {
                        "ports": [{"name": "greeting", "port_type": "Str"}]
                    }}
                ],
                "connections": [
                    {
                        "from_node": "wi",
                        "from_port": "greeting",
                        "to_node": "wo",
                        "to_port": "greeting",
                        "port_type": "Str"
                    }
                ],
                "interface": {
                    "inputs": [{"name": "greeting", "port_type": "Str"}],
                    "outputs": [{"name": "greeting", "port_type": "Str"}]
                }
            }
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/jobs")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp_body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let create_json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
        let job_id = create_json["id"].as_str().unwrap().to_string();

        let job = state
            .inner
            .jobs
            .get(&job_id)
            .expect("job should remain available");

        let params = job
            .params
            .as_ref()
            .expect("params should be inferred from WorkflowInput node params");
        assert_eq!(
            params.get("greeting"),
            Some(&serde_json::json!("hello from interface"))
        );
    }

    #[tokio::test]
    async fn test_create_job_prefers_explicit_workflow_name_over_workflow_payload_name() {
        let state = test_state();
        let mut app = app_router(state.clone());

        let body = serde_json::json!({
            "workflow_name": "Named from request",
            "workflow": {
                "name": "Name inside workflow JSON",
                "nodes": [
                    {"id": "src", "node_type": "test_source", "params": {}},
                    {"id": "dst", "node_type": "test_sink", "params": {}}
                ],
                "connections": [
                    {
                        "from_node": "src",
                        "from_port": "output",
                        "to_node": "dst",
                        "to_port": "input",
                        "port_type": "VideoFrames"
                    }
                ]
            }
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/jobs")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let job_id = json["id"]
            .as_str()
            .expect("job id should be present")
            .to_string();

        let job = state
            .inner
            .jobs
            .get(&job_id)
            .expect("job should exist in memory");
        assert_eq!(job.workflow_name, "Named from request");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_create_job_jellyfin_video_manual_node_params_execute_successfully() {
        let mut node_registry = NodeRegistry::new();
        register_all_nodes(&mut node_registry);
        let model_registry = ModelRegistry::with_builtin_models(test_models_dir());
        let state = AppState::new(
            node_registry,
            model_registry,
            DashMap::new(),
            AppConfig::default(),
            test_config_path(),
            test_data_dir(),
        );
        let mut app = app_router(state.clone());

        let workflow = serde_json::json!({
            "nodes": [
                {"id": "jelly", "node_type": "JellyfinVideo", "params": {
                    "jellyfin_url": "http://localhost:8096",
                    "api_key": "test-api-key",
                    "item_id": "episode-01"
                }},
                {"id": "wo", "node_type": "WorkflowOutput", "params": {
                    "ports": [{"name": "video_url", "port_type": "Str"}]
                }}
            ],
            "connections": [
                {
                    "from_node": "jelly",
                    "from_port": "video_url",
                    "to_node": "wo",
                    "to_port": "video_url",
                    "port_type": "Str"
                }
            ],
            "interface": {
                "inputs": [],
                "outputs": [{"name": "video_url", "port_type": "Str"}]
            }
        });

        let body = serde_json::json!({ "workflow": workflow });
        let req = Request::builder()
            .method("POST")
            .uri("/api/jobs")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp_body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let create_json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
        let job_id = create_json["id"]
            .as_str()
            .expect("job id should be present")
            .to_string();

        let status = wait_for_job_terminal_status(&state, &job_id).await;
        let job = state
            .inner
            .jobs
            .get(&job_id)
            .expect("job should remain queryable in state");

        assert_eq!(
            status,
            JobStatus::Completed,
            "expected Completed, got {:?}, error: {:?}",
            job.status,
            job.error
        );
        assert!(
            job.error.is_none(),
            "manual node params path should not fail JellyfinVideo Str input resolution"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_create_job_constant_str_param_validates_and_executes_across_boundary() {
        let mut node_registry = NodeRegistry::new();
        register_all_nodes(&mut node_registry);
        let model_registry = ModelRegistry::with_builtin_models(test_models_dir());
        let state = AppState::new(
            node_registry,
            model_registry,
            DashMap::new(),
            AppConfig::default(),
            test_config_path(),
            test_data_dir(),
        );
        let mut app = app_router(state.clone());

        let workflow = serde_json::json!({
            "nodes": [
                {"id": "constant", "node_type": "Constant", "params": {
                    "type": "Str",
                    "value": "hello-from-constant"
                }},
                {"id": "wo", "node_type": "WorkflowOutput", "params": {
                    "ports": [{"name": "value", "port_type": "Str"}]
                }}
            ],
            "connections": [
                {
                    "from_node": "constant",
                    "from_port": "value",
                    "to_node": "wo",
                    "to_port": "value",
                    "port_type": "Str"
                }
            ],
            "interface": {
                "inputs": [],
                "outputs": [{"name": "value", "port_type": "Str"}]
            }
        });

        let body = serde_json::json!({ "workflow": workflow });
        let req = Request::builder()
            .method("POST")
            .uri("/api/jobs")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp_body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let create_json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
        let job_id = create_json["id"]
            .as_str()
            .expect("job id should be present")
            .to_string();

        let status = wait_for_job_terminal_status(&state, &job_id).await;
        let job = state
            .inner
            .jobs
            .get(&job_id)
            .expect("job should remain queryable in state");

        assert_eq!(
            status,
            JobStatus::Completed,
            "expected Completed, got {:?}, error: {:?}",
            job.status,
            job.error
        );
        assert!(
            job.error.is_none(),
            "Constant type=Str should validate and execute without Int/Str boundary mismatch"
        );
    }

    #[tokio::test]
    async fn test_create_job_invalid_workflow() {
        let mut app = test_router();
        let body = serde_json::json!({
            "workflow": {"invalid": true}
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/jobs")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_run_workflow_name_creates_single_job_and_persists_metadata() {
        let data_dir = test_data_dir();
        let state = test_state_with_data_dir(data_dir.clone());
        let mut app = app_router(state.clone());

        let workflows_dir = unique_temp_dir("videnoa-run-workflows");
        let presets_dir = unique_temp_dir("videnoa-run-presets");
        std::fs::create_dir_all(&workflows_dir).expect("create workflows dir");
        std::fs::create_dir_all(&presets_dir).expect("create presets dir");
        set_workflow_lookup_dirs(&state, workflows_dir.clone(), presets_dir.clone()).await;

        let workflow_doc = serde_json::json!({
            "name": "Inner Name Should Not Override",
            "description": "Run API test",
            "workflow": valid_workflow_json()
        });
        write_json_file(&workflows_dir.join("named-run.json"), &workflow_doc);

        let body = serde_json::json!({
            "workflow_name": "named-run",
            "params": {
                "input": "/tmp/input-video.mkv",
                "seed": 42
            }
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/run")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let job_id = json["id"]
            .as_str()
            .expect("job id should be present")
            .to_string();
        assert_eq!(json["status"], "queued");
        assert_eq!(
            state.inner.jobs.len(),
            1,
            "run endpoint must create exactly one job"
        );

        let job = state
            .inner
            .jobs
            .get(&job_id)
            .expect("job should remain available");
        assert_eq!(job.workflow_name, "named-run");
        assert_eq!(job.workflow_source, WORKFLOW_SOURCE_API_RUN_WORKFLOWS);
        let params = job.params.as_ref().expect("params should be preserved");
        assert_eq!(
            params.get("input"),
            Some(&serde_json::json!("/tmp/input-video.mkv"))
        );
        assert_eq!(params.get("seed"), Some(&serde_json::json!(42)));

        let conn = Connection::open(data_dir.join("jobs.db")).expect("open jobs db");
        let (workflow_name, workflow_source, params_json): (String, String, Option<String>) = conn
            .query_row(
                "SELECT workflow_name, workflow_source, params_json FROM jobs WHERE id = ?1",
                rusqlite::params![job_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("query run job metadata");

        assert_eq!(workflow_name, "named-run");
        assert_eq!(workflow_source, WORKFLOW_SOURCE_API_RUN_WORKFLOWS);
        let params_value: serde_json::Value = serde_json::from_str(
            &params_json.expect("params_json should be persisted for /api/run"),
        )
        .expect("params_json should deserialize");
        assert_eq!(params_value["input"], "/tmp/input-video.mkv");
        assert_eq!(params_value["seed"], 42);

        let _ = std::fs::remove_dir_all(&workflows_dir);
        let _ = std::fs::remove_dir_all(&presets_dir);
    }

    #[tokio::test]
    async fn test_run_workflow_name_rejects_json_suffix() {
        let mut app = test_router();
        let body = serde_json::json!({
            "workflow_name": "named-run.json"
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/run")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "workflow_name must not include .json suffix");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_run_workflow_name_prefers_workflows_dir_over_presets_dir() {
        let state = test_state();
        let mut app = app_router(state.clone());

        let workflows_dir = unique_temp_dir("videnoa-run-precedence-workflows");
        let presets_dir = unique_temp_dir("videnoa-run-precedence-presets");
        std::fs::create_dir_all(&workflows_dir).expect("create workflows dir");
        std::fs::create_dir_all(&presets_dir).expect("create presets dir");
        set_workflow_lookup_dirs(&state, workflows_dir.clone(), presets_dir.clone()).await;

        write_json_file(
            &workflows_dir.join("shared-name.json"),
            &serde_json::json!({"workflow": valid_workflow_json()}),
        );
        write_json_file(
            &presets_dir.join("shared-name.json"),
            &serde_json::json!({"workflow": {"invalid": true}}),
        );

        let req = Request::builder()
            .method("POST")
            .uri("/api/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({"workflow_name": "shared-name"})).unwrap(),
            ))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let job_id = json["id"]
            .as_str()
            .expect("job id should be present")
            .to_string();

        let job = state
            .inner
            .jobs
            .get(&job_id)
            .expect("job should exist in memory");
        assert_eq!(job.workflow_source, WORKFLOW_SOURCE_API_RUN_WORKFLOWS);

        let _ = std::fs::remove_dir_all(&workflows_dir);
        let _ = std::fs::remove_dir_all(&presets_dir);
    }

    #[tokio::test]
    async fn test_run_workflow_name_rejects_missing_or_empty_workflow_name() {
        let mut app = test_router();

        let missing_req = Request::builder()
            .method("POST")
            .uri("/api/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({})).unwrap(),
            ))
            .unwrap();
        let missing_resp = send_request(&mut app, missing_req).await;
        assert_eq!(missing_resp.status(), StatusCode::BAD_REQUEST);

        let missing_body = axum::body::to_bytes(missing_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let missing_json: serde_json::Value = serde_json::from_slice(&missing_body).unwrap();
        assert_eq!(missing_json["error"], "workflow_name is required");

        let empty_req = Request::builder()
            .method("POST")
            .uri("/api/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({"workflow_name": "   "})).unwrap(),
            ))
            .unwrap();
        let empty_resp = send_request(&mut app, empty_req).await;
        assert_eq!(empty_resp.status(), StatusCode::BAD_REQUEST);

        let empty_body = axum::body::to_bytes(empty_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let empty_json: serde_json::Value = serde_json::from_slice(&empty_body).unwrap();
        assert_eq!(empty_json["error"], "workflow_name is required");
    }

    #[tokio::test]
    async fn test_run_workflow_name_rejects_batch_file_paths_payload() {
        let state = test_state();
        let mut app = app_router(state.clone());

        let req = Request::builder()
            .method("POST")
            .uri("/api/run")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "workflow_name": "shared-name",
                    "file_paths": ["/tmp/a.mkv", "/tmp/b.mkv"]
                }))
                .unwrap(),
            ))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            state.inner.jobs.len(),
            0,
            "batch payload must not create jobs"
        );
    }

    async fn assert_legacy_node_rejected(node_id: &str, node_type: &str) {
        let mut app = test_router();
        let body = serde_json::json!({
            "workflow": {
                "nodes": [
                    {
                        "id": node_id,
                        "node_type": node_type,
                        "params": {}
                    }
                ],
                "connections": []
            }
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/jobs")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let resp_body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let err_json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
        let err = err_json["error"]
            .as_str()
            .expect("error payload should include message");

        assert!(
            err.contains("workflow validation failed"),
            "expected validation failure prefix, got: {err}"
        );
        assert!(
            err.contains(&format!(
                "failed to instantiate node '{node_id}' of type '{node_type}'"
            )),
            "expected node id + type in error, got: {err}"
        );
        assert!(
            err.contains(&format!("unknown node type: {node_type}")),
            "expected unknown node type detail, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_create_job_rejects_legacy_stream_input_node_type() {
        assert_legacy_node_rejected("legacy_stream", "StreamInput").await;
    }

    #[tokio::test]
    async fn test_create_job_rejects_legacy_jellyfin_input_node_type() {
        assert_legacy_node_rejected("legacy_jellyfin", "JellyfinInput").await;
    }

    #[tokio::test]
    async fn test_list_jobs_returns_created() {
        let state = test_state();
        let mut app = app_router(state.clone());

        let body = serde_json::json!({
            "workflow": valid_workflow_json()
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/jobs")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let _ = send_request(&mut app, req).await;

        let req = Request::builder()
            .uri("/api/jobs")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(!json.is_empty());
        assert!(json[0].get("workflow_name").is_some());
        assert!(json[0].get("workflow_source").is_some());
        assert!(json[0].get("params").is_some());
        assert!(json[0].get("rerun_of_job_id").is_some());
        assert!(json[0].get("duration_ms").is_some());
    }

    #[tokio::test]
    async fn test_get_job_found() {
        let state = test_state();
        let mut app = app_router(state.clone());

        let body = serde_json::json!({
            "workflow": valid_workflow_json()
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/jobs")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = send_request(&mut app, req).await;
        let resp_body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let create_json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
        let job_id = create_json["id"].as_str().unwrap();

        let req = Request::builder()
            .uri(format!("/api/jobs/{job_id}"))
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["id"], job_id);
    }

    #[tokio::test]
    async fn test_get_job_includes_metadata_for_ad_hoc_job() {
        let state = test_state();
        let mut app = app_router(state.clone());

        let mut workflow = valid_workflow_json();
        workflow
            .as_object_mut()
            .expect("workflow should be object")
            .insert(
                "name".to_string(),
                serde_json::Value::String("Manual Workflow".to_string()),
            );

        let body = serde_json::json!({
            "workflow": workflow,
            "params": {"input": "/tmp/input-video.mkv"}
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/jobs")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp_body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let create_json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
        let job_id = create_json["id"].as_str().unwrap();

        let req = Request::builder()
            .uri(format!("/api/jobs/{job_id}"))
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["workflow_name"], "Manual Workflow");
        assert_eq!(json["workflow_source"], WORKFLOW_SOURCE_API_JOBS);
        assert_eq!(json["params"]["input"], "/tmp/input-video.mkv");
        assert!(json["rerun_of_job_id"].is_null());
        assert!(json.get("duration_ms").is_some());
    }

    #[tokio::test]
    async fn test_get_batch_job_includes_default_metadata() {
        let state = test_state();
        let mut app = app_router(state.clone());

        let body = serde_json::json!({
            "file_paths": [temp_path_str("video1.mkv")],
            "workflow": valid_workflow_json()
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/batch")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let job_id = json["job_ids"][0].as_str().unwrap();

        let req = Request::builder()
            .uri(format!("/api/jobs/{job_id}"))
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["workflow_name"], DEFAULT_WORKFLOW_NAME_API_BATCH);
        assert_eq!(json["workflow_source"], WORKFLOW_SOURCE_API_BATCH);
        assert!(json["params"].is_null());
        assert!(json["rerun_of_job_id"].is_null());
        assert!(json.get("duration_ms").is_some());
    }

    #[tokio::test]
    async fn test_get_job_not_found() {
        let mut app = test_router();
        let req = Request::builder()
            .uri("/api/jobs/nonexistent-id")
            .body(Body::empty())
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_job_history_removes_only_target_row_and_views() {
        let data_dir = test_data_dir();
        let state = test_state_with_data_dir(data_dir.clone());
        let mut app = app_router(state.clone());

        let target_id = format!("delete-target-{}", Uuid::new_v4());
        let target_job = build_test_job(target_id.clone(), JobStatus::Completed, None);
        insert_test_job(&state, target_job);

        let other_id = format!("delete-other-{}", Uuid::new_v4());
        let other_job = build_test_job(other_id.clone(), JobStatus::Failed, None);
        insert_test_job(&state, other_job);

        let req = Request::builder()
            .method("DELETE")
            .uri(format!("/api/jobs/{target_id}"))
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        assert!(state.inner.jobs.get(&target_id).is_none());
        assert!(state.inner.jobs.get(&other_id).is_some());

        let req = Request::builder()
            .uri(format!("/api/jobs/{target_id}"))
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        let req = Request::builder()
            .uri("/api/jobs")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let listed_jobs: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(!listed_jobs
            .iter()
            .any(|job| job["id"].as_str() == Some(target_id.as_str())));
        assert!(listed_jobs
            .iter()
            .any(|job| job["id"].as_str() == Some(other_id.as_str())));

        assert_eq!(persisted_job_status(&data_dir, &target_id), None);
        assert_eq!(
            persisted_job_status(&data_dir, &other_id).as_deref(),
            Some("failed")
        );
    }

    #[tokio::test]
    async fn test_delete_job_history_cancels_active_job_then_removes_row() {
        let data_dir = test_data_dir();
        let state = test_state_with_data_dir(data_dir.clone());
        let mut app = app_router(state.clone());

        let active_id = format!("delete-active-{}", Uuid::new_v4());
        let active_job = build_test_job(active_id.clone(), JobStatus::Running, None);
        let cancel_probe = active_job.cancel_token.clone();
        insert_test_job(&state, active_job);

        let req = Request::builder()
            .method("DELETE")
            .uri(format!("/api/jobs/{active_id}"))
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        assert!(cancel_probe.is_cancelled());
        assert!(state.inner.jobs.get(&active_id).is_none());
        assert_eq!(persisted_job_status(&data_dir, &active_id), None);

        let req = Request::builder()
            .uri(format!("/api/jobs/{active_id}"))
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_rerun_allows_non_completed_statuses_and_creates_new_job() {
        let source_statuses = [
            JobStatus::Queued,
            JobStatus::Running,
            JobStatus::Failed,
            JobStatus::Cancelled,
        ];

        for source_status in source_statuses {
            let state = test_state();
            let mut app = app_router(state.clone());

            let source_id = format!("rerun-source-{}", Uuid::new_v4());
            let source_params = Some(HashMap::from([(
                "seed".to_string(),
                serde_json::json!(source_status as u8),
            )]));
            let source_job =
                build_test_job(source_id.clone(), source_status, source_params.clone());
            insert_test_job(&state, source_job.clone());

            let req = Request::builder()
                .method("POST")
                .uri(format!("/api/jobs/{source_id}/rerun"))
                .body(Body::empty())
                .unwrap();
            let resp = send_request(&mut app, req).await;
            assert_eq!(
                resp.status(),
                StatusCode::CREATED,
                "expected rerun to be allowed for status {source_status:?}"
            );

            let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let rerun_id = json["id"]
                .as_str()
                .expect("rerun response should include id")
                .to_string();

            assert_ne!(rerun_id, source_id);
            assert_eq!(json["status"], "queued");
            assert!(json.get("created_at").is_some());

            let rerun_job = state
                .inner
                .jobs
                .get(&rerun_id)
                .expect("rerun job should exist in state");
            assert_eq!(
                rerun_job.rerun_of_job_id.as_deref(),
                Some(source_id.as_str())
            );
            assert_eq!(rerun_job.workflow_name, source_job.workflow_name);
            assert_eq!(rerun_job.workflow_source, source_job.workflow_source);
            assert_eq!(rerun_job.params, source_params);
        }
    }

    #[tokio::test]
    async fn test_rerun_rejects_completed_source_job() {
        let state = test_state();
        let mut app = app_router(state.clone());

        let source_id = format!("rerun-completed-source-{}", Uuid::new_v4());
        let source_job = build_test_job(source_id.clone(), JobStatus::Completed, None);
        insert_test_job(&state, source_job.clone());

        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/jobs/{source_id}/rerun"))
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json["error"],
            format!("{RERUN_COMPLETED_REJECTION}: {source_id}")
        );
        assert_eq!(state.inner.jobs.len(), 1);

        let source_after = state
            .inner
            .jobs
            .get(&source_id)
            .expect("source job should remain present");
        assert_eq!(source_after.status, JobStatus::Completed);
        assert!(source_after.rerun_of_job_id.is_none());
    }

    #[tokio::test]
    async fn test_rerun_preserves_source_row_immutability() {
        let data_dir = test_data_dir();
        let state = test_state_with_data_dir(data_dir.clone());
        let mut app = app_router(state.clone());

        let source_id = format!("rerun-row-source-{}", Uuid::new_v4());
        let mut source_job = build_test_job(
            source_id.clone(),
            JobStatus::Failed,
            Some(HashMap::from([(
                "input".to_string(),
                serde_json::json!("/tmp/source.mkv"),
            )])),
        );
        source_job.rerun_of_job_id = Some("older-ancestor-id".to_string());
        insert_test_job(&state, source_job.clone());

        let conn = Connection::open(data_dir.join("jobs.db")).expect("open jobs db");
        let source_row_before: (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
            String,
        ) = conn
            .query_row(
                "SELECT status, error, params_json, rerun_of_job_id, workflow_name, workflow_source
                 FROM jobs
                 WHERE id = ?1",
                rusqlite::params![source_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .expect("query source row before rerun");

        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/jobs/{source_id}/rerun"))
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let rerun_id = json["id"]
            .as_str()
            .expect("rerun id should exist")
            .to_string();

        let source_row_after: (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
            String,
        ) = conn
            .query_row(
                "SELECT status, error, params_json, rerun_of_job_id, workflow_name, workflow_source
                 FROM jobs
                 WHERE id = ?1",
                rusqlite::params![source_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .expect("query source row after rerun");
        assert_eq!(source_row_before, source_row_after);

        let rerun_row_rerun_of: Option<String> = conn
            .query_row(
                "SELECT rerun_of_job_id FROM jobs WHERE id = ?1",
                rusqlite::params![rerun_id],
                |row| row.get(0),
            )
            .expect("query rerun row linkage");
        assert_eq!(rerun_row_rerun_of.as_deref(), Some(source_id.as_str()));

        let source_after = state
            .inner
            .jobs
            .get(&source_id)
            .expect("source job should remain in state");
        assert_eq!(source_after.status, JobStatus::Failed);
        assert_eq!(
            source_after.rerun_of_job_id.as_deref(),
            Some("older-ancestor-id")
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_job_lifecycle_is_persisted_to_data_dir_jobs_db() {
        let data_dir = test_data_dir();
        let state = test_state_with_data_dir(data_dir.clone());
        let mut app = app_router(state.clone());

        let mut workflow = delay_workflow_json(350);
        workflow
            .as_object_mut()
            .expect("workflow should be object")
            .insert(
                "name".to_string(),
                serde_json::Value::String("Persisted Delay Workflow".to_string()),
            );

        let body = serde_json::json!({
            "workflow": workflow,
            "params": {
                "seed": 7
            }
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/jobs")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp_body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let create_json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
        let job_id = create_json["id"].as_str().unwrap().to_string();

        assert!(
            data_dir.join("jobs.db").exists(),
            "expected jobs.db at {}",
            data_dir.join("jobs.db").display()
        );

        assert!(
            wait_for_persisted_status(&data_dir, &job_id, "running").await,
            "expected running transition to be persisted"
        );

        let terminal = wait_for_job_terminal_status(&state, &job_id).await;
        assert_eq!(terminal, JobStatus::Completed);

        assert!(
            wait_for_persisted_status(&data_dir, &job_id, "completed").await,
            "expected completed transition to be persisted"
        );

        let conn = Connection::open(data_dir.join("jobs.db")).expect("open jobs db");
        let (workflow_name, workflow_source, params_json, rerun_of_job_id, started_at, completed_at): (
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = conn
            .query_row(
                "SELECT workflow_name, workflow_source, params_json, rerun_of_job_id, started_at, completed_at
                 FROM jobs
                 WHERE id = ?1",
                rusqlite::params![job_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .expect("query persisted metadata");

        assert_eq!(workflow_name, "Persisted Delay Workflow");
        assert_eq!(workflow_source, WORKFLOW_SOURCE_API_JOBS);
        assert!(rerun_of_job_id.is_none());
        assert!(started_at.is_some());
        assert!(completed_at.is_some());

        let params_json = params_json.expect("params_json should be persisted");
        let params_value: serde_json::Value =
            serde_json::from_str(&params_json).expect("params_json should be valid JSON");
        assert_eq!(params_value["seed"], 7);
    }

    #[test]
    fn test_startup_restore_reconciles_running_job_to_cancelled() {
        let data_dir = test_data_dir();
        let initial_state = test_state_with_data_dir(data_dir.clone());

        let workflow: PipelineGraph =
            serde_json::from_value(valid_workflow_json()).expect("workflow should deserialize");
        let created_at = Utc::now() - chrono::Duration::minutes(2);
        let started_at = Some(created_at + chrono::Duration::seconds(5));
        let job_id = format!("restore-{}", Uuid::new_v4());

        let stale_running_job = Job {
            id: job_id.clone(),
            status: JobStatus::Running,
            workflow,
            created_at,
            started_at,
            completed_at: None,
            progress: Some(ProgressUpdate {
                current_frame: 42,
                total_frames: Some(300),
                fps: 12.0,
                eta_seconds: Some(21.5),
            }),
            error: Some("executor interrupted before shutdown".to_string()),
            cancel_token: CancellationToken::new(),
            params: Some(HashMap::from([(
                "input".to_string(),
                serde_json::Value::String("/tmp/input.mkv".to_string()),
            )])),
            workflow_name: "Restore Candidate".to_string(),
            workflow_source: WORKFLOW_SOURCE_API_JOBS.to_string(),
            rerun_of_job_id: Some("older-job-id".to_string()),
        };

        initial_state
            .persist_job_snapshot(&stale_running_job)
            .expect("persist running snapshot");

        let restored_state = test_state_with_data_dir(data_dir.clone());
        let restored_job = restored_state
            .inner
            .jobs
            .get(&job_id)
            .expect("job should be restored from persistence");

        assert_eq!(restored_job.status, JobStatus::Cancelled);
        assert!(restored_job.completed_at.is_some());
        assert!(restored_job.error.is_some());
        assert!(restored_job
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("transitioned to 'cancelled' for retry safety"));
        assert_eq!(restored_job.workflow_name, "Restore Candidate");
        assert_eq!(restored_job.workflow_source, WORKFLOW_SOURCE_API_JOBS);
        assert_eq!(
            restored_job.rerun_of_job_id.as_deref(),
            Some("older-job-id")
        );

        let conn = Connection::open(data_dir.join("jobs.db")).expect("open jobs db");
        let (status, completed_at_raw, error_raw): (String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT status, completed_at, error FROM jobs WHERE id = ?1",
                rusqlite::params![job_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("query reconciled row");

        assert_eq!(status, "cancelled");
        assert!(completed_at_raw.is_some());
        assert!(error_raw
            .as_deref()
            .unwrap_or_default()
            .contains("transitioned to 'cancelled' for retry safety"));
    }

    #[tokio::test]
    async fn test_job_ws_serializes_progress_and_debug_events() {
        let progress_event = JobWsEvent::from(ProgressUpdate {
            current_frame: 12,
            total_frames: Some(240),
            fps: 23.5,
            eta_seconds: Some(9.7),
        });
        let progress_json = serde_json::to_value(&progress_event).unwrap();
        assert_eq!(progress_json["type"], "progress");
        assert_eq!(progress_json["current_frame"], 12);
        assert_eq!(progress_json["total_frames"], 240);
        assert_eq!(progress_json["fps"], 23.5);
        assert_eq!(progress_json["eta_seconds"], 9.7);
        assert!(progress_json.get("node_id").is_none());

        let parsed_progress: JobWsEvent = serde_json::from_value(progress_json).unwrap();
        assert_eq!(parsed_progress, progress_event);

        let debug_event = JobWsEvent::from(NodeDebugValueEvent {
            node_id: "print_1".to_string(),
            node_type: "Print".to_string(),
            value_preview: "hello".to_string(),
            truncated: false,
            preview_max_chars: 512,
        });
        let debug_json = serde_json::to_value(&debug_event).unwrap();
        assert_eq!(debug_json["type"], "node_debug_value");
        assert_eq!(debug_json["node_id"], "print_1");
        assert_eq!(debug_json["node_type"], "Print");
        assert_eq!(debug_json["value_preview"], "hello");
        assert_eq!(debug_json["truncated"], false);
        assert_eq!(debug_json["preview_max_chars"], 512);
        assert!(debug_json.get("current_frame").is_none());

        let parsed_debug: JobWsEvent = serde_json::from_value(debug_json).unwrap();
        assert_eq!(parsed_debug, debug_event);
    }

    #[test]
    fn test_print_preview_throttle_per_node() {
        let window = std::time::Duration::from_millis(PRINT_PREVIEW_THROTTLE_MS);
        let start = std::time::Instant::now();

        let mut job_a_throttle = NodeDebugEventThrottle::new(window);
        assert!(job_a_throttle.should_emit("node-a", start));
        assert!(
            !job_a_throttle.should_emit("node-a", start + std::time::Duration::from_millis(149))
        );
        assert!(job_a_throttle.should_emit("node-b", start + std::time::Duration::from_millis(149)));
        assert!(job_a_throttle.should_emit("node-a", start + std::time::Duration::from_millis(150)));
        assert!(
            !job_a_throttle.should_emit("node-b", start + std::time::Duration::from_millis(298))
        );
        assert!(job_a_throttle.should_emit("node-b", start + std::time::Duration::from_millis(299)));

        let mut job_b_throttle = NodeDebugEventThrottle::new(window);
        assert!(job_b_throttle.should_emit("node-a", start + std::time::Duration::from_millis(1)));
    }

    #[test]
    fn test_estimate_input_fps_from_second_frame_ignores_first_frame_delay() {
        let started_at = Instant::now();
        let delayed_first_frame = started_at + Duration::from_secs(10);
        let (baseline, first_fps) =
            estimate_input_fps_from_second_frame(None, 1, delayed_first_frame);
        assert_eq!(first_fps, 0.0);

        let (_baseline, second_fps) = estimate_input_fps_from_second_frame(
            baseline,
            2,
            delayed_first_frame + Duration::from_secs(1),
        );
        assert!((second_fps - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_estimate_input_fps_from_second_frame_resets_when_frame_counter_rewinds() {
        let started_at = Instant::now();
        let (baseline, _) = estimate_input_fps_from_second_frame(None, 5, started_at);
        let (rewound_baseline, rewound_fps) =
            estimate_input_fps_from_second_frame(baseline, 2, started_at + Duration::from_secs(1));
        assert_eq!(rewound_fps, 0.0);

        let (_, resumed_fps) = estimate_input_fps_from_second_frame(
            rewound_baseline,
            3,
            started_at + Duration::from_secs(2),
        );
        assert!((resumed_fps - 1.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_list_nodes() {
        let mut app = test_router();
        let req = Request::builder()
            .uri("/api/nodes")
            .body(Body::empty())
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.len(), 22);
        let node_types: Vec<&str> = json
            .iter()
            .map(|n| n["node_type"].as_str().unwrap())
            .collect();
        assert!(node_types.contains(&"Downloader"));
        assert!(node_types.contains(&"PathDivider"));
        assert!(node_types.contains(&"PathJoiner"));
        assert!(node_types.contains(&"StringReplace"));
        assert!(node_types.contains(&"StringTemplate"));
        assert!(node_types.contains(&"TypeConversion"));
        assert!(node_types.contains(&"HttpRequest"));
        assert!(node_types.contains(&"Print"));
        assert!(node_types.contains(&"VideoInput"));
        assert!(node_types.contains(&"SuperResolution"));
        assert!(node_types.contains(&"VideoOutput"));
        assert!(node_types.contains(&"Constant"));
        assert!(node_types.contains(&"WorkflowInput"));
        assert!(node_types.contains(&"WorkflowOutput"));
        assert!(node_types.contains(&"Workflow"));

        let downloader = json
            .iter()
            .find(|node| node["node_type"] == "Downloader")
            .expect("Downloader descriptor should be present");
        let outputs = downloader["outputs"]
            .as_array()
            .expect("Downloader outputs should be an array");
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0]["name"], "path");
    }

    #[tokio::test]
    async fn test_list_models() {
        let mut app = test_router();
        let req = Request::builder()
            .uri("/api/models")
            .body(Body::empty())
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.len(), 3);
    }

    #[tokio::test]
    async fn test_list_presets() {
        let presets = DashMap::new();
        presets.insert(
            "test-preset".to_string(),
            Preset {
                name: "Test Preset".to_string(),
                description: "A test preset".to_string(),
                workflow: serde_json::json!({"nodes": [], "connections": []}),
            },
        );

        let mut node_registry = NodeRegistry::new();
        node_registry.register("test_source", |_params| {
            Ok(Box::new(TestNode {
                node_type: "test_source".to_string(),
                inputs: vec![],
                outputs: vec![],
            }))
        });
        let model_registry = ModelRegistry::with_builtin_models(test_models_dir());
        let state = AppState::new(
            node_registry,
            model_registry,
            presets,
            AppConfig::default(),
            test_config_path(),
            test_data_dir(),
        );
        let mut app = app_router(state);

        let req = Request::builder()
            .uri("/api/presets")
            .body(Body::empty())
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.len(), 1);
        assert_eq!(json[0]["id"], "test-preset");
        assert_eq!(json[0]["name"], "Test Preset");
        assert!(json[0]["workflow"].is_object());
    }

    #[tokio::test]
    async fn test_create_batch() {
        let state = test_state();
        let mut app = app_router(state.clone());

        let body = serde_json::json!({
            "file_paths": [temp_path_str("video1.mkv"), temp_path_str("video2.mp4")],
            "workflow": valid_workflow_json()
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/batch")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["total"], 2);
        assert_eq!(json["job_ids"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_create_batch_empty_paths() {
        let mut app = test_router();
        let body = serde_json::json!({
            "file_paths": [],
            "workflow": valid_workflow_json()
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/batch")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_extract_frames_missing_file() {
        let mut app = test_router();
        let body = serde_json::json!({
            "video_path": temp_path_str("nonexistent-video-file-12345.mkv"),
            "count": 3
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/preview/extract")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"].as_str().unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn test_extract_frames_invalid_count() {
        let mut app = test_router();
        let body = serde_json::json!({
            "video_path": temp_path_str("some_video.mkv"),
            "count": 0
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/preview/extract")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_serve_preview_frame_not_found() {
        let mut app = test_router();
        let req = Request::builder()
            .uri("/api/preview/frames/nonexistent-session/frame_0001.png")
            .body(Body::empty())
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_process_frame_session_not_found() {
        let mut app = test_router();
        let body = serde_json::json!({
            "preview_id": "nonexistent-session",
            "frame_index": 0,
            "workflow": {"nodes": [], "connections": []}
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/preview/process")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_create_preset() {
        let mut app = test_router();
        let body = serde_json::json!({
            "name": "My Custom Preset",
            "description": "Custom workflow",
            "workflow": {"nodes": [], "connections": []}
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/presets")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["id"].is_string());
        assert_eq!(json["name"], "My Custom Preset");
    }

    fn fs_test_state(models_dir: PathBuf) -> AppState {
        let mut node_registry = NodeRegistry::new();
        node_registry.register("test_source", |_params| {
            Ok(Box::new(TestNode {
                node_type: "test_source".to_string(),
                inputs: vec![],
                outputs: vec![],
            }))
        });
        let model_registry = ModelRegistry::with_builtin_models(test_models_dir());
        let config = AppConfig {
            paths: crate::config::PathsConfig {
                models_dir,
                trt_cache_dir: temp_path("trt_cache"),
                presets_dir: temp_path("videnoa-test-presets-nonexistent"),
                workflows_dir: temp_path("videnoa-test-workflows-nonexistent"),
            },
            ..AppConfig::default()
        };
        AppState::new(
            node_registry,
            model_registry,
            DashMap::new(),
            config,
            test_config_path(),
            test_data_dir(),
        )
    }

    // Assembles a minimal ONNX protobuf (Add node, 2 float32[1,3] inputs, 1 output)
    // using raw prost wire encoding since server crate can't access videnoa's generated proto types.
    fn build_test_onnx_bytes() -> Vec<u8> {
        use prost::encoding::*;

        fn embed(field: u32, msg: &[u8], buf: &mut Vec<u8>) {
            encode_key(field, WireType::LengthDelimited, buf);
            encode_varint(msg.len() as u64, buf);
            buf.extend_from_slice(msg);
        }

        fn encode_value_info(name: &str) -> Vec<u8> {
            let mut dim1 = Vec::new();
            int64::encode(1, &1i64, &mut dim1);
            let mut dim3 = Vec::new();
            int64::encode(1, &3i64, &mut dim3);

            let mut shape = Vec::new();
            embed(1, &dim1, &mut shape);
            embed(1, &dim3, &mut shape);

            let mut tensor = Vec::new();
            int32::encode(1, &1i32, &mut tensor); // elem_type = FLOAT
            embed(2, &shape, &mut tensor);

            let mut type_proto = Vec::new();
            embed(1, &tensor, &mut type_proto); // oneof tensor_type

            let mut vi = Vec::new();
            string::encode(1, &name.to_string(), &mut vi);
            embed(2, &type_proto, &mut vi);
            vi
        }

        let vi_a = encode_value_info("A");
        let vi_b = encode_value_info("B");
        let vi_c = encode_value_info("C");

        let mut node = Vec::new();
        string::encode(1, &"A".to_string(), &mut node);
        string::encode(1, &"B".to_string(), &mut node);
        string::encode(2, &"C".to_string(), &mut node);
        string::encode(3, &"add_0".to_string(), &mut node);
        string::encode(4, &"Add".to_string(), &mut node);

        let mut init = Vec::new();
        let mut dims_packed = Vec::new();
        encode_varint(1, &mut dims_packed);
        encode_varint(3, &mut dims_packed);
        embed(1, &dims_packed, &mut init); // dims: [1, 3]
        int32::encode(2, &1i32, &mut init); // data_type = FLOAT
        string::encode(8, &"B".to_string(), &mut init);

        let mut opset = Vec::new();
        int64::encode(2, &17i64, &mut opset);

        let mut graph = Vec::new();
        embed(1, &node, &mut graph);
        embed(5, &init, &mut graph);
        embed(11, &vi_a, &mut graph);
        embed(11, &vi_b, &mut graph);
        embed(12, &vi_c, &mut graph);

        let mut buf = Vec::new();
        int64::encode(1, &8i64, &mut buf);
        string::encode(2, &"test".to_string(), &mut buf);
        string::encode(3, &"1.0".to_string(), &mut buf);
        embed(7, &graph, &mut buf);
        embed(8, &opset, &mut buf);

        buf
    }

    #[tokio::test]
    async fn test_inspect_model_valid() {
        let dir = std::env::temp_dir().join(format!("videnoa-inspect-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("test_model.onnx"), build_test_onnx_bytes()).unwrap();

        let state = fs_test_state(dir.clone());
        let mut app = app_router(state);

        let req = Request::builder()
            .uri("/api/models/test_model.onnx/inspect")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["ir_version"], 8);
        assert_eq!(json["opset_version"], 17);
        assert_eq!(json["producer_name"], "test");
        assert_eq!(json["op_count"], 1);
        assert_eq!(json["param_count"], 3);
        assert!(json["inputs"].as_array().unwrap().len() >= 2);
        assert!(json["outputs"].as_array().unwrap().len() >= 1);
        assert_eq!(json["nodes"][0]["op_type"], "Add");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_inspect_model_path_traversal() {
        let dir = std::env::temp_dir().join(format!("videnoa-inspect-trav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let state = fs_test_state(dir.clone());
        let mut app = app_router(state);

        let req = Request::builder()
            .uri("/api/models/..%2F..%2Fetc%2Fpasswd/inspect")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_inspect_model_not_found() {
        let dir = std::env::temp_dir().join(format!("videnoa-inspect-nf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let state = fs_test_state(dir.clone());
        let mut app = app_router(state);

        let req = Request::builder()
            .uri("/api/models/nonexistent.onnx/inspect")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_fs_list_models_dir() {
        let dir = std::env::temp_dir().join(format!("videnoa-fs-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("model_a.onnx"), b"fake").unwrap();
        std::fs::write(dir.join("model_b.onnx"), b"fake").unwrap();
        std::fs::create_dir_all(dir.join("subdir")).unwrap();
        std::fs::write(dir.join(".hidden"), b"secret").unwrap();

        let state = fs_test_state(dir.clone());
        let mut app = app_router(state);

        let req = Request::builder()
            .uri("/api/fs/list?base=models")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let entries: Vec<FsEntry> = serde_json::from_slice(&body).unwrap();

        assert!(entries[0].is_dir, "directories should come first");
        assert_eq!(entries[0].name, "subdir");

        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"model_a.onnx"));
        assert!(names.contains(&"model_b.onnx"));
        assert!(!names.contains(&".hidden"), "hidden files must be excluded");

        for entry in &entries {
            assert!(entry.path.starts_with("models/"));
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_fs_list_with_prefix() {
        let dir = std::env::temp_dir().join(format!("videnoa-fs-prefix-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("RealESRGAN_x4.onnx"), b"fake").unwrap();
        std::fs::write(dir.join("RealCUGAN_x2.onnx"), b"fake").unwrap();
        std::fs::write(dir.join("RIFE_v4.onnx"), b"fake").unwrap();

        let state = fs_test_state(dir.clone());
        let mut app = app_router(state);

        let req = Request::builder()
            .uri("/api/fs/list?base=models&prefix=Real")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let entries: Vec<FsEntry> = serde_json::from_slice(&body).unwrap();

        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|e| e.name.starts_with("Real")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_fs_list_unknown_base() {
        let mut app = test_router();
        let req = Request::builder()
            .uri("/api/fs/list?base=../../etc")
            .body(Body::empty())
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_fs_list_traversal_blocked() {
        let dir = std::env::temp_dir().join(format!("videnoa-fs-trav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let state = fs_test_state(dir.clone());
        let mut app = app_router(state);

        let req = Request::builder()
            .uri("/api/fs/list?base=models&prefix=../../../etc/passwd")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_fs_list_nonexistent_base_dir() {
        let state = fs_test_state(temp_path("videnoa-nonexistent-dir-xyz"));
        let mut app = app_router(state);

        let req = Request::builder()
            .uri("/api/fs/list?base=models")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let entries: Vec<FsEntry> = serde_json::from_slice(&body).unwrap();
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn test_fs_list_default_base_is_models() {
        let dir = std::env::temp_dir().join(format!("videnoa-fs-default-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("test.onnx"), b"fake").unwrap();

        let state = fs_test_state(dir.clone());
        let mut app = app_router(state);

        let req = Request::builder()
            .uri("/api/fs/list")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let entries: Vec<FsEntry> = serde_json::from_slice(&body).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "test.onnx");
        assert_eq!(entries[0].path, "models/test.onnx");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_browse_fs_tmp() {
        let mut app = test_router();
        let req = Request::builder()
            .uri("/api/fs/browse?path=/tmp")
            .body(Body::empty())
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let _: Vec<FsEntry> = serde_json::from_slice(&body).unwrap();
    }

    #[tokio::test]
    async fn test_browse_fs_tilde() {
        let mut app = test_router();
        let req = Request::builder()
            .uri("/api/fs/browse?path=~")
            .body(Body::empty())
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let entries: Vec<FsEntry> = serde_json::from_slice(&body).unwrap();
        assert!(!entries.is_empty());

        if let Some(first) = entries.first() {
            assert!(first.path.starts_with('/'));
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_browse_fs_denied_proc() {
        let mut app = test_router();
        let req = Request::builder()
            .uri("/api/fs/browse?path=/proc")
            .body(Body::empty())
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_browse_fs_denied_sys() {
        let mut app = test_router();
        let req = Request::builder()
            .uri("/api/fs/browse?path=/sys")
            .body(Body::empty())
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_browse_fs_nonexistent() {
        let mut app = test_router();
        let req = Request::builder()
            .uri("/api/fs/browse?path=/nonexistent_path_xyz_123")
            .body(Body::empty())
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let entries: Vec<FsEntry> = serde_json::from_slice(&body).unwrap();
        assert!(entries.is_empty());
    }

    fn workflow_test_state(workflows_dir: PathBuf) -> AppState {
        let mut node_registry = NodeRegistry::new();
        node_registry.register("test_source", |_params| {
            Ok(Box::new(TestNode {
                node_type: "test_source".to_string(),
                inputs: vec![],
                outputs: vec![],
            }))
        });
        let model_registry = ModelRegistry::with_builtin_models(test_models_dir());
        let config = AppConfig {
            paths: crate::config::PathsConfig {
                models_dir: test_models_dir(),
                trt_cache_dir: temp_path("trt_cache"),
                presets_dir: temp_path("videnoa-test-presets-nonexistent"),
                workflows_dir,
            },
            ..AppConfig::default()
        };
        AppState::new(
            node_registry,
            model_registry,
            DashMap::new(),
            config,
            test_config_path(),
            test_data_dir(),
        )
    }

    #[tokio::test]
    async fn test_list_workflows_empty() {
        let dir = std::env::temp_dir().join(format!("videnoa-wf-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let state = workflow_test_state(dir.clone());
        let mut app = app_router(state);

        let req = Request::builder()
            .uri("/api/workflows")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let entries: Vec<WorkflowEntry> = serde_json::from_slice(&body).unwrap();
        assert!(entries.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_save_and_list_workflows() {
        let dir = std::env::temp_dir().join(format!("videnoa-wf-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let state = workflow_test_state(dir.clone());
        let mut app = app_router(state);

        let body = serde_json::json!({
            "name": "My Workflow",
            "description": "Test workflow",
            "workflow": {
                "nodes": [],
                "connections": [],
                "interface": {
                    "inputs": [{"name": "video", "type": "VideoFrames"}]
                }
            }
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/workflows")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp_body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: WorkflowEntry = serde_json::from_slice(&resp_body).unwrap();
        assert_eq!(created.name, "My Workflow");
        assert!(created.has_interface);
        assert!(created.filename.ends_with(".json"));

        let req = Request::builder()
            .uri("/api/workflows")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let entries: Vec<WorkflowEntry> = serde_json::from_slice(&body).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "My Workflow");
        assert_eq!(entries[0].description, "Test workflow");
        assert!(entries[0].has_interface);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_save_and_delete_workflow() {
        let dir = std::env::temp_dir().join(format!("videnoa-wf-del-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let state = workflow_test_state(dir.clone());
        let mut app = app_router(state);

        let body = serde_json::json!({
            "name": "Deletable",
            "description": "Will be deleted",
            "workflow": {"nodes": [], "connections": []}
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/workflows")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp_body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
        let filename = created["filename"].as_str().unwrap();

        let req = Request::builder()
            .method("DELETE")
            .uri(format!("/api/workflows/{filename}"))
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let req = Request::builder()
            .uri("/api/workflows")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let entries: Vec<WorkflowEntry> = serde_json::from_slice(&body).unwrap();
        assert!(entries.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_save_workflow_path_traversal() {
        let dir = std::env::temp_dir().join(format!("videnoa-wf-trav-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let state = workflow_test_state(dir.clone());
        let mut app = app_router(state);

        let body = serde_json::json!({
            "name": "../etc/passwd",
            "description": "malicious",
            "workflow": {"nodes": [], "connections": []}
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/workflows")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_create_job_video_pipeline_with_params() {
        let mut node_registry = NodeRegistry::new();
        register_all_nodes(&mut node_registry);
        let model_registry = ModelRegistry::with_builtin_models(test_models_dir());
        let state = AppState::new(
            node_registry,
            model_registry,
            DashMap::new(),
            AppConfig::default(),
            test_config_path(),
            test_data_dir(),
        );
        let mut app = app_router(state.clone());

        let body = serde_json::json!({
            "workflow": {
                "nodes": [
                    {"id": "input", "node_type": "VideoInput", "params": {"path": temp_path_str("nonexistent.mp4")}},
                    {"id": "sr", "node_type": "SuperResolution", "params": {"model_path": temp_path_str("model.onnx"), "scale": 2, "tile_size": 0}},
                    {"id": "output", "node_type": "VideoOutput", "params": {
                        "output_path": temp_path_str("out.mp4"), "codec": "libx265", "crf": 18,
                        "pixel_format": "yuv420p10le", "width": 1920, "height": 1080, "fps": "24"
                    }}
                ],
                "connections": [
                    {"from_node": "input", "from_port": "frames", "to_node": "sr", "to_port": "frames", "port_type": "VideoFrames"},
                    {"from_node": "sr", "from_port": "frames", "to_node": "output", "to_port": "frames", "port_type": "VideoFrames"},
                    {"from_node": "input", "from_port": "source_path", "to_node": "output", "to_port": "source_path", "port_type": "Path"}
                ]
            },
            "params": {"greeting": "hello"}
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/jobs")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp_body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let create_json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
        let job_id = create_json["id"].as_str().unwrap().to_string();

        let status = wait_for_job_terminal_status(&state, &job_id).await;
        let job = state.inner.jobs.get(&job_id).unwrap();
        assert_eq!(
            status,
            JobStatus::Failed,
            "expected Failed, got {:?}",
            job.status
        );
        assert_eq!(
            job.params
                .as_ref()
                .and_then(|params| params.get("greeting")),
            Some(&serde_json::json!("hello")),
            "job metadata should preserve submitted params"
        );
        assert!(
            !job.error
                .as_deref()
                .unwrap_or("")
                .contains("do not support workflow parameters"),
            "video workflow should no longer fail on params guard, got: {:?}",
            job.error,
        );
    }

    #[tokio::test]
    async fn test_update_config_invalid_json() {
        let mut app = test_router();

        let req = Request::builder()
            .method("PUT")
            .uri("/api/config")
            .header("content-type", "application/json")
            .body(Body::from(b"not valid json".to_vec()))
            .unwrap();

        let resp = send_request(&mut app, req).await;
        assert!(
            resp.status().is_client_error(),
            "expected client error, got {}",
            resp.status()
        );
    }

    #[tokio::test]
    async fn test_update_config_roundtrip() {
        let mut app = test_router();

        let req = Request::builder()
            .uri("/api/config")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let mut config: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let custom_models = temp_path_str("custom_models");
        config["paths"]["models_dir"] = serde_json::json!(custom_models);

        let req = Request::builder()
            .method("PUT")
            .uri("/api/config")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&config).unwrap()))
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let req = Request::builder()
            .uri("/api/config")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let updated: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(updated["paths"]["models_dir"], custom_models);
    }

    #[tokio::test]
    async fn test_relative_workflows_resolution_uses_current_dir() {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let workflows_rel = PathBuf::from(format!(
            "target/videnoa-wf-cwd-{}-{timestamp}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&workflows_rel);

        let state = workflow_test_state(workflows_rel.clone());
        let mut app = app_router(state);

        let body = serde_json::json!({
            "name": "Resolved Workflow",
            "description": "Tests current-dir resolution",
            "workflow": {"nodes": [], "connections": []}
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/workflows")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let expected_file = workflows_rel.join("Resolved Workflow.json");
        assert!(
            expected_file.exists(),
            "Workflow file should be at {}",
            expected_file.display()
        );

        let _ = std::fs::remove_dir_all(&workflows_rel);
    }

    #[tokio::test]
    async fn test_get_memory_endpoints_are_replaced_by_performance_routes() {
        let mut app = test_router();

        for endpoint in [
            "/api/performance/current",
            "/api/performance/overview",
            "/api/performance/export",
            "/api/performance/capabilities",
        ] {
            let req = Request::builder()
                .uri(endpoint)
                .body(Body::empty())
                .unwrap();
            let resp = send_request(&mut app, req).await;
            assert_eq!(resp.status(), StatusCode::OK, "expected 200 for {endpoint}");

            let content_type = resp
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();
            assert!(
                content_type.contains("application/json"),
                "expected JSON content-type for {endpoint}, got {content_type}"
            );
        }

        for endpoint in [
            "/api/memory/current",
            "/api/memory/overview",
            "/api/memory/export",
            "/api/memory/capabilities",
        ] {
            let req = Request::builder()
                .uri(endpoint)
                .body(Body::empty())
                .unwrap();
            let resp = send_request(&mut app, req).await;
            assert_eq!(
                resp.status(),
                StatusCode::NOT_FOUND,
                "expected deterministic API 404 for {endpoint}"
            );
        }
    }

    #[tokio::test]
    async fn test_memory_endpoints_do_not_fallback_to_spa() {
        let static_dir = unique_temp_dir("videnoa-spa-memory-fallback");
        std::fs::create_dir_all(&static_dir).expect("create static dir");
        std::fs::write(
            static_dir.join("index.html"),
            "<html><body>SPA</body></html>",
        )
        .expect("write index.html");

        let state = test_state();
        let mut app = app_router_with_static(state, Some(&static_dir));

        let req = Request::builder()
            .uri("/api/memory/current")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        let content_type = resp
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        assert!(
            !content_type.contains("text/html"),
            "legacy API route must not fall through to SPA HTML"
        );

        let _ = std::fs::remove_dir_all(&static_dir);
    }

    #[tokio::test]
    async fn test_performance_route_still_returns_json_with_spa_static_fallback_enabled() {
        let static_dir = unique_temp_dir("videnoa-spa-performance-route");
        std::fs::create_dir_all(&static_dir).expect("create static dir");
        std::fs::write(
            static_dir.join("index.html"),
            "<html><body>SPA</body></html>",
        )
        .expect("write index.html");

        let state = test_state();
        let mut app = app_router_with_static(state, Some(&static_dir));

        let req = Request::builder()
            .uri("/api/performance/current")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let content_type = resp
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        assert!(
            content_type.contains("application/json"),
            "performance route must resolve to API JSON even when SPA fallback is enabled"
        );

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["status"], "disabled");
        assert_eq!(payload["enabled"], false);

        let req = Request::builder().uri("/").body(Body::empty()).unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let content_type = resp
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        assert!(
            content_type.contains("text/html"),
            "non-API route should still use SPA fallback"
        );

        let _ = std::fs::remove_dir_all(&static_dir);
    }

    #[tokio::test]
    async fn test_performance_routes_use_enabled_envelopes_when_profiling_is_enabled() {
        let state = test_state();
        {
            let mut config = state.inner.config.write().await;
            config.performance.profiling_enabled = true;
        }

        let mut app = app_router(state);

        for endpoint in [
            "/api/performance/current",
            "/api/performance/overview",
            "/api/performance/export",
            "/api/performance/capabilities",
        ] {
            let req = Request::builder()
                .uri(endpoint)
                .body(Body::empty())
                .unwrap();
            let resp = send_request(&mut app, req).await;
            assert_eq!(resp.status(), StatusCode::OK, "expected 200 for {endpoint}");

            let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(
                payload["enabled"], true,
                "enabled flag mismatch for {endpoint}"
            );
            assert_ne!(
                payload["status"],
                serde_json::json!("disabled"),
                "status should not be disabled for {endpoint}",
            );
        }

        let req = Request::builder()
            .uri("/api/performance/overview")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(payload["metrics"].is_object());
        assert!(payload["metrics"].get("ram_used_bytes").is_some());
        assert!(payload["metrics"].get("ram_total_bytes").is_some());

        let req = Request::builder()
            .uri("/api/performance/export")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["series"].as_array().map(|rows| rows.len()), Some(2));
    }

    #[tokio::test]
    async fn test_performance_export_accumulates_time_series_samples_over_requests() {
        let state = test_state();
        {
            let mut config = state.inner.config.write().await;
            config.performance.profiling_enabled = true;
        }

        let mut app = app_router(state);

        for _ in 0..2 {
            let req = Request::builder()
                .uri("/api/performance/export")
                .body(Body::empty())
                .unwrap();
            let _ = send_request(&mut app, req).await;
            tokio::time::sleep(Duration::from_millis(2)).await;
        }

        let req = Request::builder()
            .uri("/api/performance/export")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(&mut app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let series = payload["series"]
            .as_array()
            .expect("performance export should include a series array");
        assert_eq!(series.len(), 3);

        let ordered_timestamps = series
            .iter()
            .map(|entry| {
                entry["timestamp_ms"]
                    .as_i64()
                    .expect("series entry should include timestamp_ms")
            })
            .collect::<Vec<_>>();

        let mut sorted_unique_timestamps = ordered_timestamps.clone();
        sorted_unique_timestamps.sort_unstable();
        sorted_unique_timestamps.dedup();

        assert_eq!(sorted_unique_timestamps.len(), 3);
        assert_eq!(
            ordered_timestamps, sorted_unique_timestamps,
            "series entries should be ordered by timestamp",
        );
    }

    fn performance_contract_router(
        current_payload: serde_json::Value,
        overview_payload: serde_json::Value,
        export_payload: serde_json::Value,
        capabilities_payload: serde_json::Value,
    ) -> Router {
        Router::new()
            .route(
                "/api/performance/current",
                axum::routing::get(move || {
                    let payload = current_payload.clone();
                    async move { Json(payload) }
                }),
            )
            .route(
                "/api/performance/overview",
                axum::routing::get(move || {
                    let payload = overview_payload.clone();
                    async move { Json(payload) }
                }),
            )
            .route(
                "/api/performance/export",
                axum::routing::get(move || {
                    let payload = export_payload.clone();
                    async move { Json(payload) }
                }),
            )
            .route(
                "/api/performance/capabilities",
                axum::routing::get(move || {
                    let payload = capabilities_payload.clone();
                    async move { Json(payload) }
                }),
            )
    }

    async fn get_performance_contract_json(router: &mut Router, uri: &str) -> serde_json::Value {
        let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let resp = send_request(router, req).await;
        assert_eq!(resp.status(), StatusCode::OK, "unexpected status for {uri}");
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn test_performance_contract_routes_use_deterministic_off_envelopes() {
        let mut app = performance_contract_router(
            serde_json::json!({
                "status": "disabled",
                "enabled": false,
                "reason": "disabled_by_config",
                "message": "telemetry disabled",
                "metrics": null,
            }),
            serde_json::json!({
                "status": "disabled",
                "enabled": false,
                "reason": "disabled_by_config",
                "message": "telemetry disabled",
                "metrics": null,
            }),
            serde_json::json!({
                "status": "disabled",
                "enabled": false,
                "reason": "disabled_by_config",
                "message": "telemetry disabled",
                "series": [],
            }),
            serde_json::json!({
                "status": "disabled",
                "enabled": false,
                "reason": "disabled_by_config",
                "message": "telemetry disabled",
                "supported_statuses": ["disabled", "enabled", "degraded", "partial"],
            }),
        );

        let current = get_performance_contract_json(&mut app, "/api/performance/current").await;
        let overview = get_performance_contract_json(&mut app, "/api/performance/overview").await;
        let exported = get_performance_contract_json(&mut app, "/api/performance/export").await;
        let capabilities =
            get_performance_contract_json(&mut app, "/api/performance/capabilities").await;

        for payload in [&current, &overview, &exported, &capabilities] {
            assert_eq!(payload["status"], "disabled");
            assert_eq!(payload["enabled"], false);
        }

        assert!(current["metrics"].is_null());
        assert!(overview["metrics"].is_null());
        assert_eq!(exported["series"], serde_json::json!([]));
        assert_eq!(
            capabilities["supported_statuses"],
            serde_json::json!(["disabled", "enabled", "degraded", "partial"]),
        );
    }

    #[tokio::test]
    async fn test_performance_contract_routes_keep_partial_enabled_with_sparse_metrics() {
        let mut app = performance_contract_router(
            serde_json::json!({
                "status": "partial",
                "enabled": true,
                "reason": "gpu_missing",
                "message": "partial telemetry",
                "metrics": {
                    "cpu_util_percent": 24.0,
                    "gpu_util_percent": null,
                },
            }),
            serde_json::json!({
                "status": "partial",
                "enabled": true,
                "reason": "gpu_missing",
                "message": "partial telemetry",
                "metrics": {
                    "cpu_util_percent": 24.0,
                    "ram_used_bytes": 2147483648u64,
                },
            }),
            serde_json::json!({
                "status": "partial",
                "enabled": true,
                "reason": "cpu_only",
                "message": "partial export",
                "series": [
                    {
                        "timestamp_ms": 1700000000000u64,
                        "metrics": {
                            "cpu_util_percent": 21.0,
                        },
                    },
                ],
            }),
            serde_json::json!({
                "status": "partial",
                "enabled": true,
                "reason": "cpu_only",
                "message": "partial capabilities",
                "supported_statuses": ["disabled", "enabled", "degraded", "partial"],
            }),
        );

        let current = get_performance_contract_json(&mut app, "/api/performance/current").await;
        let overview = get_performance_contract_json(&mut app, "/api/performance/overview").await;
        let exported = get_performance_contract_json(&mut app, "/api/performance/export").await;
        let capabilities =
            get_performance_contract_json(&mut app, "/api/performance/capabilities").await;

        for payload in [&current, &overview, &exported, &capabilities] {
            assert_eq!(payload["status"], "partial");
            assert_eq!(payload["enabled"], true);
        }

        assert_eq!(
            current["metrics"]["gpu_util_percent"],
            serde_json::Value::Null
        );
        assert!(overview["metrics"].get("gpu_util_percent").is_none());
        assert_eq!(
            exported["series"].as_array().map(|rows| rows.len()),
            Some(1)
        );
        assert_eq!(
            exported["series"][0]["metrics"]["cpu_util_percent"],
            serde_json::json!(21.0),
        );
        assert_eq!(capabilities["supported_statuses"][3], "partial");
    }

    #[tokio::test]
    async fn test_performance_contract_routes_keep_degraded_enabled_with_fallback_series() {
        let mut app = performance_contract_router(
            serde_json::json!({
                "status": "degraded",
                "enabled": true,
                "reason": "sampler_stale",
                "message": "degraded telemetry",
                "metrics": {
                    "cpu_util_percent": 12.0,
                    "gpu_util_percent": null,
                },
            }),
            serde_json::json!({
                "status": "degraded",
                "enabled": true,
                "reason": "sampler_stale",
                "message": "degraded telemetry",
                "metrics": {
                    "cpu_util_percent": 12.0,
                    "ram_used_bytes": 3221225472u64,
                    "ram_total_bytes": 8589934592u64,
                },
            }),
            serde_json::json!({
                "status": "degraded",
                "enabled": true,
                "reason": "export_missing_keys",
                "message": "degraded export",
                "series": [
                    {
                        "timestamp_ms": 1700000000000u64,
                        "metrics": {
                            "temperature_celsius": 61.0,
                        },
                    },
                    {
                        "timestamp_ms": 1700000001000u64,
                        "metrics": {
                            "temperature_celsius": 62.0,
                        },
                    },
                ],
            }),
            serde_json::json!({
                "status": "degraded",
                "enabled": true,
                "reason": "sampler_stale",
                "message": "degraded capabilities",
                "supported_statuses": ["disabled", "enabled", "degraded", "partial"],
            }),
        );

        let current = get_performance_contract_json(&mut app, "/api/performance/current").await;
        let overview = get_performance_contract_json(&mut app, "/api/performance/overview").await;
        let exported = get_performance_contract_json(&mut app, "/api/performance/export").await;
        let capabilities =
            get_performance_contract_json(&mut app, "/api/performance/capabilities").await;

        for payload in [&current, &overview, &exported, &capabilities] {
            assert_eq!(payload["status"], "degraded");
            assert_eq!(payload["enabled"], true);
        }

        assert_eq!(
            current["metrics"]["gpu_util_percent"],
            serde_json::Value::Null
        );
        assert_eq!(
            exported["series"].as_array().map(|rows| rows.len()),
            Some(2),
            "degraded export should keep deterministic fallback rows",
        );
        assert!(
            overview["metrics"].get("gpu_util_percent").is_none(),
            "degraded overview allows sparse/missing fields for deterministic fallback",
        );
        assert_eq!(capabilities["supported_statuses"][2], "degraded");
    }

    #[test]
    fn test_parse_proc_stat_cpu_line_extracts_total_and_idle_ticks() {
        let parsed = parse_proc_stat_cpu_line("cpu  100 20 30 400 50 0 10 0 0 0")
            .expect("cpu line should parse");
        assert_eq!(parsed.total_ticks, 610);
        assert_eq!(parsed.idle_ticks, 450);
    }

    #[test]
    fn test_compute_cpu_util_percent_uses_tick_deltas() {
        let previous = CpuTimes {
            total_ticks: 1_000,
            idle_ticks: 400,
        };
        let current = CpuTimes {
            total_ticks: 1_200,
            idle_ticks: 460,
        };

        let percent =
            compute_cpu_util_percent(previous, current).expect("deltas should produce percent");
        assert!((percent - 70.0).abs() < 1e-6);
    }

    #[test]
    fn test_parse_nvidia_smi_gpu_snapshot_parses_util_and_vram() {
        let snapshot = parse_nvidia_smi_gpu_snapshot("45, 1024, 8192\n")
            .expect("nvidia-smi gpu row should parse");
        assert_eq!(snapshot.gpu_util_percent, 45.0);
        assert_eq!(snapshot.vram_used_bytes, 1024 * BYTES_PER_MIB);
        assert_eq!(snapshot.vram_total_bytes, 8192 * BYTES_PER_MIB);
    }

    #[test]
    fn test_parse_nvidia_smi_compute_apps_vram_sums_matching_pid_rows() {
        let stdout = "111, 32\n222, 64\n111, 128\n111, N/A\n";
        let vram_bytes = parse_nvidia_smi_compute_apps_vram(stdout, 111)
            .expect("matching pid rows should produce a sum");
        assert_eq!(vram_bytes, (32 + 128) * BYTES_PER_MIB);
    }

    #[test]
    fn test_enabled_performance_envelope_status_transitions_match_metric_coverage() {
        let empty_sample = RuntimePerformanceSample {
            metrics: serde_json::Map::new(),
            has_cpu_metrics: false,
            has_memory_metrics: false,
            has_gpu_metrics: false,
            has_vram_metrics: false,
        };
        assert_eq!(
            enabled_performance_envelope(&empty_sample)["status"],
            "degraded"
        );

        let partial_sample = RuntimePerformanceSample {
            metrics: serde_json::Map::new(),
            has_cpu_metrics: true,
            has_memory_metrics: true,
            has_gpu_metrics: false,
            has_vram_metrics: false,
        };
        assert_eq!(
            enabled_performance_envelope(&partial_sample)["status"],
            "partial"
        );

        let full_sample = RuntimePerformanceSample {
            metrics: serde_json::Map::new(),
            has_cpu_metrics: true,
            has_memory_metrics: true,
            has_gpu_metrics: true,
            has_vram_metrics: true,
        };
        assert_eq!(
            enabled_performance_envelope(&full_sample)["status"],
            "enabled"
        );
    }
}
