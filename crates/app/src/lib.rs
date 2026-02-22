use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use clap::{ArgAction, Args, Parser, Subcommand};
use tracing::{info, warn};
use tracing_subscriber::prelude::*;

use videnoa_core::config::{config_path, data_dir, initialize_data_dir, AppConfig};
use videnoa_core::executor::SequentialExecutor;
use videnoa_core::graph::PipelineGraph;
use videnoa_core::logging::{
    self, FileSinkPlan, LoggingInitOptions, PanicHookInstallPlan, RuntimeLogMode,
    DEFAULT_LOG_FILTER,
};
use videnoa_core::nodes::compile_context::VideoCompileContext;
use videnoa_core::registry::{register_all_nodes, NodeRegistry};
use videnoa_core::types::PortData;
use videnoa_core::server::{app_router_with_static, app_state_with_config};

#[derive(Parser)]
#[command(
    name = "videnoa",
    about = "AI-powered anime video enhancement",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(
        short = 'v',
        long = "verbose",
        action = ArgAction::Count,
        global = true,
        help = "Increase log verbosity (-v: debug, -vv: trace)"
    )]
    verbose: u8,

    #[arg(
        long = "log-filter",
        value_name = "FILTER",
        global = true,
        help = "Explicit tracing filter (overrides RUST_LOG and -v)"
    )]
    log_filter: Option<String>,

    #[arg(short, long)]
    port: Option<u16>,

    #[arg(long)]
    host: Option<String>,

    #[arg(long)]
    data_dir: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    Run(RunArgs),
}

#[derive(Args)]
struct RunArgs {
    #[arg(help = "Path to workflow JSON file")]
    workflow: PathBuf,
    #[arg(short = 'i', long, help = "Override input video path in the workflow")]
    input: Option<PathBuf>,
    #[arg(short = 'o', long, help = "Override output video path in the workflow")]
    output: Option<PathBuf>,
    #[arg(
        long = "param",
        value_name = "KEY=VALUE",
        help = "Pass parameters to WorkflowInput nodes (repeatable, e.g. --param key=value)"
    )]
    params: Vec<String>,
}

pub async fn run_from_env() -> Result<()> {
    let cli = Cli::parse();
    let mode = if cli.command.is_some() {
        RuntimeLogMode::Cli
    } else {
        RuntimeLogMode::Server
    };
    let resolved_data_dir = data_dir(cli.data_dir.as_deref());

    videnoa_core::runtime::setup_runtime_libs();
    init_logging(
        mode,
        Some(resolved_data_dir.as_path()),
        cli.verbose,
        cli.log_filter.as_deref(),
    );
    videnoa_core::runtime::log_runtime_lib_status();
    log_startup_metadata(mode, Some(resolved_data_dir.as_path()));

    match cli.command {
        Some(Commands::Run(run)) => {
            run_workflow(run.workflow, run.input, run.output, run.params).await
        }
        None => run_server(cli.port, cli.host, resolved_data_dir).await,
    }
}

#[cfg(test)]
fn select_log_filter(
    noise_base: &str,
    rust_log_env: Option<&str>,
    verbose: u8,
    cli_log_filter: Option<&str>,
) -> String {
    let options = LoggingInitOptions {
        mode: RuntimeLogMode::Server,
        data_dir: None,
        verbose,
        cli_log_filter: cli_log_filter.map(ToString::to_string),
        rust_log_env: rust_log_env.map(ToString::to_string),
        default_log_filter: DEFAULT_LOG_FILTER.to_string(),
        noise_filter: noise_base.to_string(),
        include_noise_filter_when_implicit: true,
        retention_files: logging::DEFAULT_LOG_RETENTION_FILES,
    };

    logging::select_log_filter(&options)
}

fn init_logging(
    mode: RuntimeLogMode,
    data_dir: Option<&Path>,
    verbose: u8,
    cli_log_filter: Option<&str>,
) {
    let panic_hook_plan = logging::install_panic_hook(data_dir);
    if let PanicHookInstallPlan::Fallback {
        attempted_crash_dir,
        reason,
    } = &panic_hook_plan
    {
        let attempted_crash_dir = attempted_crash_dir
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<none>".to_string());
        eprintln!(
            "Warning: panic crash artifact hook unavailable (path: {attempted_crash_dir}; reason: {reason}). Panics will not be persisted to crash logs."
        );
    }

    let init_options = LoggingInitOptions {
        mode,
        data_dir: data_dir.map(Path::to_path_buf),
        verbose,
        cli_log_filter: cli_log_filter.map(ToString::to_string),
        rust_log_env: std::env::var("RUST_LOG").ok(),
        ..Default::default()
    };
    let init_plan = logging::compose_logging_init_plan(&init_options);
    let console_filter = init_plan.filters.console_filter;
    let file_filter = init_plan.filters.file_filter;

    match init_plan.file_sink {
        FileSinkPlan::Ready(ready) => {
            let console_env_filter = parse_env_filter_with_fallback(&console_filter, "console");
            let file_env_filter = parse_env_filter_with_fallback(&file_filter, "file");

            let subscriber = tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_writer(std::io::stderr)
                        .with_filter(console_env_filter),
                )
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_ansi(false)
                        .with_writer(logging::redacting_make_writer(ready.appender))
                        .with_filter(file_env_filter),
                );

            if let Err(error) = tracing::subscriber::set_global_default(subscriber) {
                eprintln!(
                    "Failed to initialize tracing subscriber: {error}. Continuing without structured tracing."
                );
            }
        }
        FileSinkPlan::Fallback(fallback) => {
            let attempted_log_dir = fallback
                .attempted_log_dir
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "<none>".to_string());
            let reason = fallback.reason;

            let console_env_filter = parse_env_filter_with_fallback(&console_filter, "console");
            let subscriber = tracing_subscriber::registry().with(
                tracing_subscriber::fmt::layer()
                    .with_writer(std::io::stderr)
                    .with_filter(console_env_filter),
            );

            if let Err(error) = tracing::subscriber::set_global_default(subscriber) {
                eprintln!(
                    "Failed to initialize tracing subscriber: {error}. Continuing without structured tracing."
                );
                return;
            }

            eprintln!(
                "Warning: persistent file logging unavailable (path: {attempted_log_dir}; reason: {reason}). Continuing with console-only logging."
            );
            warn!(
                attempted_log_dir = %attempted_log_dir,
                reason = %reason,
                "Persistent file logging unavailable; continuing with console-only logging"
            );
        }
    }

    if let PanicHookInstallPlan::Fallback {
        attempted_crash_dir,
        reason,
    } = panic_hook_plan
    {
        warn!(
            attempted_crash_dir = ?attempted_crash_dir,
            reason = %reason,
            "Panic crash artifact hook unavailable; continuing without panic artifacts"
        );
    }
}

fn parse_env_filter_with_fallback(filter: &str, sink_name: &str) -> tracing_subscriber::EnvFilter {
    tracing_subscriber::EnvFilter::try_new(filter).unwrap_or_else(|error| {
        eprintln!(
            "Invalid {sink_name} log filter '{filter}': {error}. Falling back to '{DEFAULT_LOG_FILTER}'."
        );
        tracing_subscriber::EnvFilter::new(DEFAULT_LOG_FILTER)
    })
}

fn runtime_mode_name(mode: RuntimeLogMode) -> &'static str {
    match mode {
        RuntimeLogMode::Cli => "cli",
        RuntimeLogMode::Server => "server",
        RuntimeLogMode::Desktop => "desktop",
    }
}

fn log_startup_metadata(mode: RuntimeLogMode, data_dir: Option<&Path>) {
    let pid = std::process::id();
    if let Some(data_dir) = data_dir {
        let cfg_path = config_path(data_dir);
        info!(
            mode = runtime_mode_name(mode),
            pid,
            data_dir = %data_dir.display(),
            config_path = %cfg_path.display(),
            "Runtime startup metadata"
        );
    } else {
        info!(mode = runtime_mode_name(mode), pid, "Runtime startup metadata");
    }
}

async fn run_server(
    port_override: Option<u16>,
    host_override: Option<String>,
    data_dir: PathBuf,
) -> Result<()> {
    if let Err(e) = initialize_data_dir(&data_dir) {
        warn!(error = %e, "Failed to initialize data directory");
    }
    let cfg_path = config_path(&data_dir);
    let config = match AppConfig::load_from_path(&cfg_path) {
        Ok(config) => config,
        Err(err) => {
            warn!(error = %err, "Failed to load config file, using defaults");
            AppConfig::default()
        }
    };

    let port = port_override
        .or_else(|| std::env::var("PORT").ok().and_then(|v| v.parse().ok()))
        .unwrap_or(config.server.port);
    let host = host_override.unwrap_or_else(|| config.server.host.clone());

    let state = app_state_with_config(config, cfg_path, data_dir);

    #[cfg(not(debug_assertions))]
    {
        info!("Serving embedded frontend assets");
    }

    #[cfg(debug_assertions)]
    let static_path = {
        use std::path::Path;
        let dir = Path::new("web/dist");
        if dir.is_dir() {
            Some(dir)
        } else {
            info!("web/dist/ not found — serving API only (run `cd web && npm run build` first)");
            None
        }
    };
    #[cfg(not(debug_assertions))]
    let static_path: Option<&std::path::Path> = None;

    let app = app_router_with_static(state, static_path);

    let addr = format!("{host}:{port}");
    info!(%addr, "Starting videnoa server");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn format_duration(secs: f64) -> String {
    let total = secs.round() as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

const PROGRESS_BAR_WIDTH: usize = 30;
const FPS_WARMUP_INPUT_FRAMES: u64 = 2;

fn print_progress(
    output_written: u64,
    total_output: Option<u64>,
    total_input: Option<u64>,
    total_elapsed: f64,
    fps_elapsed: f64,
) {
    let input_done = estimate_input_processed(output_written, total_output, total_input);
    let input_fps = compute_input_fps(input_done, fps_elapsed);

    if let Some(total) = total_output {
        let fraction = if total > 0 {
            (output_written as f64 / total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let percent = fraction * 100.0;
        let filled = (fraction * PROGRESS_BAR_WIDTH as f64).round() as usize;
        let empty = PROGRESS_BAR_WIDTH.saturating_sub(filled);
        let bar: String = "█".repeat(filled) + &"░".repeat(empty);

        let input_total = total_input.unwrap_or(total);

        let eta = if input_fps > 0.0 {
            let remaining_input = input_total.saturating_sub(input_done) as f64;
            format!(" | ETA: {}", format_duration(remaining_input / input_fps))
        } else {
            String::new()
        };

        eprint!(
            "\r[{}] {:5.1}% | Frame {}/{} | {:.1} fps | Elapsed: {}{}    ",
            bar,
            percent,
            input_done,
            input_total,
            input_fps,
            format_duration(total_elapsed),
            eta,
        );
    } else {
        eprint!(
            "\rFrame {} | {:.1} fps | Elapsed: {}    ",
            output_written,
            input_fps,
            format_duration(total_elapsed),
        );
    }
}

fn compute_input_fps(input_done: u64, elapsed: f64) -> f64 {
    if elapsed <= 0.0 || input_done <= FPS_WARMUP_INPUT_FRAMES {
        return 0.0;
    }

    (input_done - FPS_WARMUP_INPUT_FRAMES) as f64 / elapsed
}

fn estimate_input_processed(
    output_written: u64,
    total_output: Option<u64>,
    total_input: Option<u64>,
) -> u64 {
    match (total_output, total_input) {
        (Some(out_total), Some(in_total)) if out_total > 0 => {
            let ratio = in_total as f64 / out_total as f64;
            (output_written as f64 * ratio).round() as u64
        }
        _ => output_written,
    }
}

fn make_progress_callback() -> (Arc<AtomicU64>, Box<dyn Fn(u64, Option<u64>, Option<u64>) + Send>)
{
    let start = Instant::now();
    let fps_start = Arc::new(Mutex::new(None::<Instant>));
    let frames_written = Arc::new(AtomicU64::new(0));
    let frames_written_cb = frames_written.clone();
    let fps_start_cb = fps_start.clone();
    let callback: Box<dyn Fn(u64, Option<u64>, Option<u64>) + Send> =
        Box::new(move |current, total_output, total_input| {
            frames_written_cb.store(current, Ordering::Relaxed);
            let total_elapsed = start.elapsed().as_secs_f64();
            let input_done = estimate_input_processed(current, total_output, total_input);
            let fps_elapsed = {
                let mut start_opt = fps_start_cb
                    .lock()
                    .expect("progress callback mutex poisoned");
                if start_opt.is_none() && input_done > FPS_WARMUP_INPUT_FRAMES {
                    *start_opt = Some(Instant::now());
                }
                start_opt
                    .as_ref()
                    .map(|s| s.elapsed().as_secs_f64())
                    .unwrap_or(0.0)
            };

            print_progress(current, total_output, total_input, total_elapsed, fps_elapsed);
        });
    (frames_written, callback)
}

fn unwrap_workflow(value: serde_json::Value) -> serde_json::Value {
    if value.get("nodes").is_some() {
        return value;
    }
    if let Some(inner) = value.get("workflow").cloned() {
        if inner.get("nodes").is_some() {
            return inner;
        }
    }
    value
}

fn inject_params_into_workflow_input(
    workflow: &serde_json::Value,
    params: &HashMap<String, String>,
) -> Result<serde_json::Value> {
    if params.is_empty() {
        return Ok(workflow.clone());
    }

    let mut wf = workflow.clone();
    let nodes = wf
        .get_mut("nodes")
        .and_then(|n| n.as_array_mut())
        .context("workflow missing 'nodes' array")?;

    let mut found = false;
    for node in nodes.iter_mut() {
        let node_type = node
            .get("node_type")
            .and_then(|t| t.as_str())
            .unwrap_or_default();

        if node_type == "WorkflowInput" {
            let node_params = node
                .get_mut("params")
                .and_then(|p| p.as_object_mut())
                .context("WorkflowInput node missing 'params' object")?;

            for (key, value) in params {
                node_params.insert(
                    key.clone(),
                    serde_json::Value::String(value.clone()),
                );
            }
            found = true;
        }
    }

    if !found {
        bail!(
            "workflow has no WorkflowInput node — cannot inject parameters. \
             Add a WorkflowInput node to the workflow or use a preset that includes one."
        );
    }

    Ok(wf)
}

const KNOWN_FLAGS: &[&str] = &[
    "--input", "-i", "--output", "-o", "--param", "--help", "-h",
    "--version", "-V", "--verbose", "--log-filter", "--port", "--host", "--data-dir",
];

fn parse_dynamic_args(args: &[String], workflow_ports: &[String]) -> HashMap<String, String> {
    let mut dynamic = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg.starts_with("--") && !KNOWN_FLAGS.contains(&arg.as_str()) {
            let name = arg.trim_start_matches('-');
            if workflow_ports.contains(&name.to_string()) {
                if i + 1 < args.len() {
                    dynamic.insert(name.to_string(), args[i + 1].clone());
                    i += 2;
                    continue;
                }
            }
        }
        i += 1;
    }
    dynamic
}

async fn run_workflow(
    workflow_path: PathBuf,
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    raw_params: Vec<String>,
) -> Result<()> {
    if !workflow_path.exists() {
        bail!("Workflow file does not exist: {}", workflow_path.display());
    }

    info!("Loading workflow: {}", workflow_path.display());
    let json_str = std::fs::read_to_string(&workflow_path)
        .with_context(|| format!("Failed to read workflow file: {}", workflow_path.display()))?;

    let workflow_value: serde_json::Value = serde_json::from_str(&json_str)
        .with_context(|| format!("Failed to parse workflow JSON: {}", workflow_path.display()))?;
    let workflow_value = unwrap_workflow(workflow_value);

    let probe_graph: PipelineGraph = serde_json::from_value(workflow_value.clone())
        .with_context(|| format!("Failed to parse workflow JSON: {}", workflow_path.display()))?;

    let interface_inputs: Vec<String> = probe_graph
        .interface
        .as_ref()
        .map(|iface| iface.inputs.iter().map(|p| p.name.clone()).collect())
        .unwrap_or_default();

    if !interface_inputs.is_empty() {
        info!(
            "Workflow accepts parameters: {} (or use --param key=value)",
            interface_inputs
                .iter()
                .map(|n| format!("--{n} <VALUE>"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }

    let raw_cli_args: Vec<String> = std::env::args().collect();
    let dynamic_args = parse_dynamic_args(&raw_cli_args, &interface_inputs);

    let mut all_params: HashMap<String, String> = HashMap::new();

    if let Some(ref inp) = input {
        all_params.insert("input".to_string(), inp.display().to_string());
    }
    if let Some(ref out) = output {
        all_params.insert("output".to_string(), out.display().to_string());
    }

    for (key, value) in &dynamic_args {
        all_params.insert(key.clone(), value.clone());
    }

    for item in &raw_params {
        let (key, value) = item
            .split_once('=')
            .with_context(|| format!("invalid --param format '{}' (expected KEY=VALUE)", item))?;
        all_params.insert(key.to_string(), value.to_string());
    }

    let workflow_value = inject_params_into_workflow_input(&workflow_value, &all_params)?;

    let graph: PipelineGraph = serde_json::from_value(workflow_value)
        .with_context(|| format!("Failed to parse workflow JSON: {}", workflow_path.display()))?;

    let registry = build_registry();

    info!("Validating workflow...");
    graph
        .validate(&registry)
        .context("Workflow validation failed")?;

    if !all_params.is_empty() {
        info!(
            "Executing with params: {:?}",
            all_params.keys().collect::<Vec<_>>()
        );
    }

    let compile_ctx = VideoCompileContext::default();
    let (_frames_written, progress_callback) = make_progress_callback();

    info!("Executing workflow...");
    let outputs = SequentialExecutor::execute_with_context(
        &graph,
        &registry,
        Some(&compile_ctx),
        Some(progress_callback),
        None,
    )
    .context("Workflow execution failed")?;

    eprintln!();
    info!("Workflow completed successfully");
    for (node_id, node_outputs) in &outputs {
        for (port_name, port_data) in node_outputs {
            info!(
                "  {}:{} = {}",
                node_id,
                port_name,
                format_port_data(port_data)
            );
        }
    }

    Ok(())
}

fn build_registry() -> NodeRegistry {
    let mut registry = NodeRegistry::new();

    register_all_nodes(&mut registry);

    registry
}

fn format_port_data(data: &PortData) -> String {
    match data {
        PortData::Int(v) => format!("{}", v),
        PortData::Float(v) => format!("{}", v),
        PortData::Str(v) => format!("\"{}\"", v),
        PortData::Bool(v) => format!("{}", v),
        PortData::Path(v) => format!("{}", v.display()),
        PortData::Metadata(_) => "<MediaMetadata>".to_string(),
    }
}

#[cfg(test)]
mod param_injection_tests {
    use super::*;

    #[test]
    fn injects_params_into_workflow_input_node() {
        let workflow = serde_json::json!({
            "nodes": [
                {
                    "id": "wi",
                    "node_type": "WorkflowInput",
                    "params": {
                        "ports": [
                            {"name": "input", "port_type": "Path"},
                            {"name": "output", "port_type": "Path"}
                        ]
                    }
                },
                {"id": "vi", "node_type": "VideoInput", "params": {}},
                {"id": "vo", "node_type": "VideoOutput", "params": {}}
            ],
            "connections": []
        });

        let mut params = HashMap::new();
        params.insert("input".to_string(), "/data/video.mkv".to_string());
        params.insert("output".to_string(), "/data/out.mkv".to_string());

        let result = inject_params_into_workflow_input(&workflow, &params).unwrap();
        let nodes = result["nodes"].as_array().unwrap();
        assert_eq!(nodes[0]["params"]["input"], "/data/video.mkv");
        assert_eq!(nodes[0]["params"]["output"], "/data/out.mkv");
        assert!(nodes[1]["params"].get("path").is_none());
        assert!(nodes[2]["params"].get("output_path").is_none());
    }

    #[test]
    fn empty_params_returns_unchanged() {
        let workflow = serde_json::json!({
            "nodes": [
                {"id": "vi", "node_type": "VideoInput", "params": {"path": "/original"}}
            ],
            "connections": []
        });

        let result =
            inject_params_into_workflow_input(&workflow, &HashMap::new()).unwrap();
        assert_eq!(result, workflow);
    }

    #[test]
    fn errors_when_no_workflow_input_node() {
        let workflow = serde_json::json!({
            "nodes": [
                {"id": "vi", "node_type": "VideoInput", "params": {}}
            ],
            "connections": []
        });

        let mut params = HashMap::new();
        params.insert("input".to_string(), "/path".to_string());

        let result = inject_params_into_workflow_input(&workflow, &params);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no WorkflowInput node"));
    }

    #[test]
    fn preserves_existing_ports_definition() {
        let workflow = serde_json::json!({
            "nodes": [{
                "id": "wi",
                "node_type": "WorkflowInput",
                "params": {
                    "ports": [{"name": "input", "port_type": "Path"}]
                }
            }],
            "connections": []
        });

        let mut params = HashMap::new();
        params.insert("input".to_string(), "/new/path.mkv".to_string());

        let result = inject_params_into_workflow_input(&workflow, &params).unwrap();
        let wi_params = &result["nodes"][0]["params"];
        assert!(wi_params["ports"].is_array());
        assert_eq!(wi_params["input"], "/new/path.mkv");
    }
}

#[cfg(test)]
mod duration_tests {
    use super::*;

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(6595.0), "01:49:55");
        assert_eq!(format_duration(45.0), "00:00:45");
        assert_eq!(format_duration(3600.0), "01:00:00");
        assert_eq!(format_duration(0.0), "00:00:00");
        assert_eq!(format_duration(86400.0), "24:00:00");
    }
}

#[cfg(test)]
mod progress_fps_tests {
    use super::*;

    #[test]
    fn fps_is_zero_until_warmup_frames_pass() {
        assert_eq!(compute_input_fps(0, 1.0), 0.0);
        assert_eq!(compute_input_fps(1, 1.0), 0.0);
        assert_eq!(compute_input_fps(2, 1.0), 0.0);
    }

    #[test]
    fn fps_excludes_first_two_input_frames() {
        let fps = compute_input_fps(12, 5.0);
        assert!((fps - 2.0).abs() < 1e-9);
    }

    #[test]
    fn estimate_input_processed_still_handles_interpolation_ratio() {
        let done = estimate_input_processed(50, Some(100), Some(51));
        assert_eq!(done, 26);
    }
}

#[cfg(test)]
mod unwrap_workflow_tests {
    use super::*;

    #[test]
    fn bare_workflow_returned_as_is() {
        let wf = serde_json::json!({"nodes": [], "connections": []});
        assert_eq!(unwrap_workflow(wf.clone()), wf);
    }

    #[test]
    fn preset_envelope_is_unwrapped() {
        let inner = serde_json::json!({"nodes": [{"id": "a"}], "connections": []});
        let envelope = serde_json::json!({"name": "preset", "workflow": inner.clone()});
        assert_eq!(unwrap_workflow(envelope), inner);
    }

    #[test]
    fn unrecognised_shape_returned_as_is() {
        let unknown = serde_json::json!({"something": "else"});
        assert_eq!(unwrap_workflow(unknown.clone()), unknown);
    }
}

#[cfg(test)]
mod format_port_data_tests {
    use super::*;
    use std::path::PathBuf;

    fn test_temp_path(suffix: &str) -> PathBuf {
        std::env::temp_dir().join(suffix)
    }

    #[test]
    fn formats_all_variants() {
        assert_eq!(format_port_data(&PortData::Int(42)), "42");
        assert_eq!(format_port_data(&PortData::Float(3.14)), "3.14");
        assert_eq!(format_port_data(&PortData::Str("hi".into())), "\"hi\"");
        assert_eq!(format_port_data(&PortData::Bool(true)), "true");
        let path = test_temp_path("x");
        let path_str = path.to_string_lossy().to_string();
        assert_eq!(
            format_port_data(&PortData::Path(path)),
            path_str
        );
    }
}

#[cfg(test)]
mod dynamic_args_tests {
    use super::*;

    #[test]
    fn extracts_workflow_ports() {
        let args: Vec<String> = vec![
            "videnoa", "run", "workflow.json",
            "--input_path", "/path/to/video",
            "--scale", "4",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let ports = vec!["input_path".to_string(), "scale".to_string()];
        let result = parse_dynamic_args(&args, &ports);
        assert_eq!(result.get("input_path").unwrap(), "/path/to/video");
        assert_eq!(result.get("scale").unwrap(), "4");
    }

    #[test]
    fn ignores_known_flags() {
        let args: Vec<String> = vec![
            "videnoa", "run", "workflow.json",
            "--input", "/path/to/video",
            "--output", "/path/to/output",
            "--scale", "4",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let ports = vec![
            "input".to_string(),
            "output".to_string(),
            "scale".to_string(),
        ];
        let result = parse_dynamic_args(&args, &ports);
        assert!(!result.contains_key("input"));
        assert!(!result.contains_key("output"));
        assert_eq!(result.get("scale").unwrap(), "4");
    }

    #[test]
    fn ignores_unknown_ports() {
        let args: Vec<String> = vec![
            "videnoa", "run", "workflow.json",
            "--unknown_arg", "value",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let ports = vec!["input_path".to_string()];
        let result = parse_dynamic_args(&args, &ports);
        assert!(result.is_empty());
    }

    #[test]
    fn trailing_flag_without_value_is_skipped() {
        let args: Vec<String> = vec![
            "videnoa", "run", "workflow.json",
            "--scale",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let ports = vec!["scale".to_string()];
        let result = parse_dynamic_args(&args, &ports);
        assert!(result.is_empty());
    }

    #[test]
    fn empty_ports_returns_empty() {
        let args: Vec<String> = vec![
            "videnoa", "run", "workflow.json",
            "--scale", "4",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let ports: Vec<String> = vec![];
        let result = parse_dynamic_args(&args, &ports);
        assert!(result.is_empty());
    }
}

#[cfg(test)]
mod log_filter_tests {
    use super::*;

    const NOISE: &str = "ort=error,ffmpeg_stderr=error,ffmpeg_encode_stderr=error,ffmpeg_stream_stderr=error";

    #[test]
    fn uses_noise_and_default_info_without_overrides() {
        let selected = select_log_filter(NOISE, None, 0, None);
        assert_eq!(selected, format!("{NOISE},info"));
    }

    #[test]
    fn uses_noise_with_rust_log_when_no_cli_overrides() {
        let selected = select_log_filter(NOISE, Some("debug"), 0, None);
        assert_eq!(selected, format!("{NOISE},debug"));
    }

    #[test]
    fn verbose_flag_overrides_rust_log() {
        let selected = select_log_filter(NOISE, Some("info"), 1, None);
        assert_eq!(selected, "debug");
    }

    #[test]
    fn double_verbose_enables_trace() {
        let selected = select_log_filter(NOISE, Some("info"), 2, None);
        assert_eq!(selected, "trace");
    }

    #[test]
    fn explicit_log_filter_has_highest_precedence() {
        let selected = select_log_filter(NOISE, Some("warn"), 2, Some("videnoa_core=trace"));
        assert_eq!(selected, "videnoa_core=trace");
    }
}
