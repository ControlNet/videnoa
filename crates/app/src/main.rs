fn main() {
    // SAFETY: configure malloc before Tokio and the native inference libraries start threads.
    unsafe { videnoa_core::runtime::configure_host_memory() };
    run();
}

#[tokio::main]
async fn run() {
    if let Err(error) = videnoa_app::run_from_env().await {
        tracing::error!("{error:#}");
        std::process::exit(1);
    }
}
