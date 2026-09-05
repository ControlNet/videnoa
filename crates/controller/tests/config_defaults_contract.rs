use videnoa_controller::config::ControllerConfig;

#[test]
fn defaults_match_locked_task_two_settings() {
    // Given: the typed Controller defaults.
    let config = ControllerConfig::default();

    // When/Then: every locked path, auth, scheduler, timeout, and retry default is explicit.
    let workspace = std::env::current_dir().expect("test working directory");
    assert_eq!(config.paths.input_roots, [workspace.clone()]);
    assert_eq!(config.paths.output_roots, [workspace.clone()]);
    assert_eq!(config.paths.data_root, workspace.join("data"));
    assert_eq!(config.paths.temp_root, workspace.join("data"));
    assert!(!config.auth.secure_cookie);
    assert_eq!(config.auth.session_absolute.as_secs(), 86_400);
    assert_eq!(config.auth.session_idle.as_secs(), 3_600);
    assert!(!config.scheduler.paused);
    assert_eq!(config.scheduler.default_compute_slots.get(), 1);
    assert_eq!(config.scheduler.prefetch_per_worker, 1);
    assert_eq!(config.scheduler.max_concurrent_uploads.get(), 1);
    assert_eq!(config.scheduler.max_concurrent_downloads.get(), 1);
    assert_eq!(config.timeouts.health.as_secs(), 10);
    assert_eq!(config.timeouts.poll.as_secs(), 5);
    assert_eq!(config.timeouts.transfer.as_secs(), 300);
    assert_eq!(config.retry.initial.as_secs(), 1);
    assert_eq!(config.retry.maximum.as_secs(), 60);
    assert_eq!(config.retry.max_attempts.get(), 5);
}
