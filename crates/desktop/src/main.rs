#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::net::TcpListener;
use std::path::Path;

use tauri::{webview::WebviewWindowBuilder, WebviewUrl};
use tracing::{error, info, warn};
use tracing_subscriber::prelude::*;

use videnoa_core::config::{config_path, data_dir, initialize_data_dir, AppConfig};
use videnoa_core::logging::{
    compose_logging_init_plan, install_panic_hook, FileSinkPlan, LoggingInitOptions,
    PanicHookInstallPlan, RuntimeLogMode, DEFAULT_LOG_FILTER,
};
use videnoa_core::server::{app_router_with_static, app_state_with_config};

fn init_logging(data_dir: std::path::PathBuf) {
    let panic_hook_plan = install_panic_hook(Some(data_dir.as_path()));
    if let PanicHookInstallPlan::Fallback {
        attempted_crash_dir,
        reason,
    } = &panic_hook_plan
    {
        eprintln!(
            "Warning: panic crash artifact hook unavailable (path: {}; reason: {}). Panics will not be persisted to crash logs.",
            attempted_crash_dir
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "<none>".to_string()),
            reason,
        );
    }

    let init_plan = compose_logging_init_plan(&LoggingInitOptions {
        mode: RuntimeLogMode::Desktop,
        data_dir: Some(data_dir),
        rust_log_env: std::env::var("RUST_LOG").ok(),
        ..Default::default()
    });

    let console_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(parse_env_filter_with_fallback(
            &init_plan.filters.console_filter,
            "console",
        ));
    let file_filter = init_plan.filters.file_filter;
    let file_sink = init_plan.file_sink;

    let mut fallback_warning = None;

    match file_sink {
        FileSinkPlan::Ready(ready_file_sink) => {
            let subscriber = tracing_subscriber::registry().with(console_layer).with(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(videnoa_core::logging::redacting_make_writer(
                        ready_file_sink.appender,
                    ))
                    .with_filter(parse_env_filter_with_fallback(&file_filter, "file")),
            );
            tracing::subscriber::set_global_default(subscriber)
                .expect("failed to install desktop tracing subscriber");
        }
        FileSinkPlan::Fallback(fallback_file_sink) => {
            fallback_warning = Some(fallback_file_sink);
            let subscriber = tracing_subscriber::registry().with(console_layer);
            tracing::subscriber::set_global_default(subscriber)
                .expect("failed to install desktop tracing subscriber");
        }
    }

    if let Some(fallback) = fallback_warning {
        warn!(
            attempted_log_dir = ?fallback.attempted_log_dir,
            retention_files = fallback.retention_files,
            reason = %fallback.reason,
            "Desktop file sink unavailable, continuing with console-only logging"
        );
    }

    if let PanicHookInstallPlan::Fallback {
        attempted_crash_dir,
        reason,
    } = panic_hook_plan
    {
        warn!(
            attempted_crash_dir = ?attempted_crash_dir,
            reason = %reason,
            "Desktop panic crash artifact hook unavailable; continuing without panic artifacts"
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

fn select_startup_window_size<R: tauri::Runtime>(app: &tauri::App<R>) -> (f64, f64) {
    let default_size = (1280.0, 720.0);

    let (monitor_width_px, monitor_height_px, scale_factor) = match app.primary_monitor() {
        Ok(Some(monitor)) => {
            let size = monitor.size();
            (
                f64::from(size.width),
                f64::from(size.height),
                monitor.scale_factor(),
            )
        }
        Ok(None) => return default_size,
        Err(err) => {
            warn!(error = %err, "Failed to read primary monitor size, using default startup window size");
            return default_size;
        }
    };

    if scale_factor <= 0.0 {
        warn!(
            scale_factor,
            "Invalid monitor scale factor, using default startup window size"
        );
        return default_size;
    }

    let (target_width_px, target_height_px) = if monitor_height_px >= 2160.0 {
        (2560.0, 1440.0)
    } else if monitor_height_px >= 1440.0 {
        (1920.0, 1080.0)
    } else {
        (1280.0, 720.0)
    };

    let monitor_width = monitor_width_px / scale_factor;
    let monitor_height = monitor_height_px / scale_factor;
    let target_width = target_width_px / scale_factor;
    let target_height = target_height_px / scale_factor;

    let scale = (monitor_width / target_width)
        .min(monitor_height / target_height)
        .min(1.0);
    let window_width = (target_width * scale).floor().max(1.0);
    let window_height = (target_height * scale).floor().max(1.0);

    info!(
        monitor_width_px,
        monitor_height_px,
        scale_factor,
        monitor_width,
        monitor_height,
        target_width_px,
        target_height_px,
        window_width,
        window_height,
        "Selected desktop startup window size"
    );

    (window_width, window_height)
}

fn detect_startup_locale() -> String {
    sys_locale::get_locale()
        .map(|locale| videnoa_core::config::normalize_supported_locale(&locale))
        .unwrap_or_else(|| videnoa_core::config::FALLBACK_LOCALE.to_string())
}

fn main() {
    videnoa_core::runtime::setup_runtime_libs();
    let startup_data_dir = data_dir(None);
    init_logging(startup_data_dir.clone());
    videnoa_core::runtime::log_runtime_lib_status();

    tauri::Builder::default()
        .setup(move |app| {
            let data_dir = startup_data_dir.clone();
            let cfg_path = config_path(&data_dir);
            let first_launch = !cfg_path.exists();

            if let Err(e) = initialize_data_dir(&data_dir) {
                warn!(error = %e, "Failed to initialize data directory");
            }
            let mut config = match AppConfig::load_from_path(&cfg_path) {
                Ok(config) => config,
                Err(err) => {
                    warn!(error = %err, "Failed to load config file, using defaults");
                    AppConfig::default()
                }
            };

            if first_launch {
                let startup_locale = detect_startup_locale();
                if config.locale != startup_locale {
                    config.locale = startup_locale.clone();
                    if let Err(err) = config.save_to_path(&cfg_path) {
                        warn!(
                            error = %err,
                            locale = %startup_locale,
                            "Failed to persist first-launch locale to config"
                        );
                    } else {
                        info!(
                            locale = %startup_locale,
                            "Initialized desktop locale from system language"
                        );
                    }
                }
            }

            let state = app_state_with_config(config, cfg_path, data_dir.clone());

            #[cfg(debug_assertions)]
            let static_path = {
                let dir = Path::new("web/dist");
                if dir.is_dir() {
                    Some(dir)
                } else {
                    info!(
                        "web/dist/ not found — serving API only (run `cd web && npm run build` first)"
                    );
                    None
                }
            };
            #[cfg(not(debug_assertions))]
            let static_path: Option<&Path> = None;

            let router = app_router_with_static(state, static_path);

            let listener = TcpListener::bind("127.0.0.1:0")?;
            listener.set_nonblocking(true)?;
            let port = listener.local_addr()?.port();

            tauri::async_runtime::spawn(async move {
                let listener = match tokio::net::TcpListener::from_std(listener) {
                    Ok(listener) => listener,
                    Err(err) => {
                        error!(error = %err, "Failed to create Tokio listener");
                        return;
                    }
                };

                if let Err(err) = axum::serve(listener, router).await {
                    error!(error = %err, "Axum server stopped");
                }
            });

            let local_server_url = format!("http://localhost:{port}");
            info!(
                mode = "desktop",
                pid = std::process::id(),
                data_dir = %data_dir.display(),
                local_server_url = %local_server_url,
                local_server_port = port,
                "Desktop runtime startup metadata"
            );
            let url = url::Url::parse(&local_server_url)?;
            let (window_width, window_height) = select_startup_window_size(app);

            WebviewWindowBuilder::new(app, "main".to_string(), WebviewUrl::External(url))
                .disable_drag_drop_handler()
                .decorations(false)
                .inner_size(window_width, window_height)
                .title("Videnoa")
                .build()?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
