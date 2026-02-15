mod branding;

use backend_cli::{AppCli, AppCommands, Parser, RuntimeMode as CliRuntimeMode};
use backend_core::{Result, RuntimeMode as ConfigRuntimeMode, load_from_path};
use backend_otlp::init_tracing;
use backend_otlp::tracing::info;
use backend_server::{run_worker, serve};
use branding::banner::BANNER;
use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[tokio::main]
async fn main() -> Result<()> {
    print!("{}", BANNER);

    match AppCli::parse().command {
        Some(AppCommands::Serve { config_path, mode }) => {
            let mut config = load_from_path(&config_path)?;
            config.runtime.mode = match mode {
                CliRuntimeMode::Server => ConfigRuntimeMode::Server,
                CliRuntimeMode::Worker => ConfigRuntimeMode::Worker,
                CliRuntimeMode::Shared => ConfigRuntimeMode::Shared,
            };
            init_tracing(&config.logging);

            match config.runtime.mode {
                ConfigRuntimeMode::Server => {
                    info!("starting in server mode");
                    serve(&config).await?;
                }
                ConfigRuntimeMode::Worker => {
                    info!("starting in worker mode");
                    run_worker(&config).await?;
                }
                ConfigRuntimeMode::Shared => {
                    info!("starting in shared mode");
                    tokio::try_join!(serve(&config), run_worker(&config))?;
                }
            }
        }
        Some(AppCommands::Migrate { config_path }) => {
            let config = load_from_path(&config_path)?;
            init_tracing(&config.logging);

            backend_migrate::connect_postgres_and_migrate(&config.database.url).await?;
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
