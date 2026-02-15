use crate::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "user-storage", author, version, about = "UserStorage App", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    Serve {
        #[arg(long, short, env = "CONFIG_PATH")]
        config_path: String,
        #[arg(long, env = "APP_MODE", value_enum, default_value_t = RuntimeMode::Shared)]
        mode: RuntimeMode,
    },
    Config {
        #[arg(long, short, env = "CONFIG_PATH")]
        config_path: String,
    },
    Migrate {
        #[arg(long, short, env = "CONFIG_PATH")]
        config_path: String,
    },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum RuntimeMode {
    Server,
    Worker,
    Shared,
}
