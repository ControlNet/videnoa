use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[cfg(debug_assertions)]
use std::path::Path;

use clap::Parser;
use videnoa_controller::{serve, FrontendAssets, StartupError};

#[derive(Debug, Parser)]
#[command(
    name = "videnoa-controller",
    version,
    about = "GPU-free Videnoa coordination service"
)]
struct Cli {
    #[arg(long, default_value_t = IpAddr::V4(Ipv4Addr::LOCALHOST))]
    host: IpAddr,
    #[arg(long, default_value_t = 3001)]
    port: u16,
}

fn frontend_assets() -> Result<FrontendAssets, StartupError> {
    #[cfg(debug_assertions)]
    {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../controller-web/dist");
        FrontendAssets::from_dist(directory)
    }

    #[cfg(not(debug_assertions))]
    {
        FrontendAssets::embedded()
    }
}

#[tokio::main]
async fn main() -> Result<(), StartupError> {
    let cli = Cli::parse();
    let address = SocketAddr::new(cli.host, cli.port);
    let assets = frontend_assets()?;
    serve(address, &assets).await
}
