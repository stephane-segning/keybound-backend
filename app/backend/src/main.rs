mod branding;

use backend_cli::{AppCli, AppCommands, Parser};
use branding::banner::BANNER;
use mimalloc::MiMalloc;
use backend_core::{load_from_path, Result};
use backend_otlp::init_tracing;
use backend_otlp::tracing::info;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[tokio::main]
async fn main() -> Result<()> {
    match AppCli::parse().command {
        Some(AppCommands::Serve { config_path }) => {
            info!("{}", BANNER);

            let config = load_from_path(&config_path)?;
            init_tracing(&config.logging);

            info!("Starting the server");
        }
        Some(AppCommands::Migrate { config_path }) => {
            let config = load_from_path(&config_path)?;
            backend_migrate::migrate(&config.database.url).await?;
        }
        Some(AppCommands::Config { config_path }) => {
            let _ = load_from_path(&config_path)?;
        }
        None => {
            info!("No command provided. Use --help for more information.");
        }
    }

    Ok(())
}
