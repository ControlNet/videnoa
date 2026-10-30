fn main() {
    // SAFETY: configure malloc before Tokio and the native inference libraries start threads.
    unsafe { videnoa_core::runtime::configure_host_memory() };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to start the Tokio runtime");
    let result = runtime.block_on(videnoa_app::run_from_env());
    // A job executor that ignored its shutdown cancel would otherwise keep
    // the process alive: dropping a runtime waits for blocking work.
    runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    if let Err(error) = result {
        tracing::error!("{error:#}");
        std::process::exit(1);
    }
}
