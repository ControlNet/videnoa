use std::error::Error;
use std::io::{BufRead, IsTerminal};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

#[cfg(debug_assertions)]
use std::path::Path;

use clap::{Parser, Subcommand};
use videnoa_controller::auth::{hash_password, AuthService};
use videnoa_controller::config::ControllerConfig;
use videnoa_controller::paths::PathCapabilities;
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};
use videnoa_controller::{serve_authenticated, FrontendAssets, StartupError};

#[derive(Debug, Parser)]
#[command(
    name = "videnoa-controller",
    version,
    about = "GPU-free Videnoa coordination service"
)]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    host: Option<IpAddr>,
    #[arg(long)]
    port: Option<u16>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    HashPassword,
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
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    if matches!(cli.command, Some(Command::HashPassword)) {
        let password = if std::io::stdin().is_terminal() {
            eprint!("Password: ");
            rpassword::read_password()?
        } else {
            let mut password = String::new();
            std::io::stdin().lock().read_line(&mut password)?;
            password.truncate(password.trim_end_matches(['\r', '\n']).len());
            password
        };
        println!("{}", hash_password(&password)?);
        return Ok(());
    }
    let mut config = ControllerConfig::load(cli.config.as_deref())?;
    if let Some(host) = cli.host {
        config.server.host = host;
    }
    if let Some(port) = cli.port {
        config.server.port = port;
    }
    let address = SocketAddr::new(config.server.host, config.server.port);
    let _paths = PathCapabilities::open(&config.paths)?;
    let database = Database::open(DatabaseOptions::new(
        config.paths.data_root.join("controller.sqlite3"),
    ))
    .await?;
    let auth = AuthService::new(config.auth.clone(), Store::new(database))?;
    if !config.auth.secure_cookie {
        eprintln!("warning: session cookies are running without Secure; use only on trusted HTTP networks");
    }
    let assets = frontend_assets()?;
    serve_authenticated(address, &assets, auth).await?;
    Ok(())
}
