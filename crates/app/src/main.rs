#[tokio::main]
async fn main() {
    if let Err(error) = videnoa_app::run_from_env().await {
        tracing::error!("{error:#}");
        std::process::exit(1);
    }
}
