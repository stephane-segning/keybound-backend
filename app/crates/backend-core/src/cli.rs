use crate::RuntimeMode;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "user-storage", author, version, about = "UserStorage App", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Legacy command kept for backward-compatibility.
    Serve {
        #[arg(long, short, env = "CONFIG_PATH")]
        config_path: String,
        #[arg(long, env = "APP_MODE", value_enum, default_value_t = RuntimeMode::Shared)]
        mode: RuntimeMode,
        #[arg(short = 'i', long)]
        import: Option<PathBuf>,
    },
    /// Start API server only.
    Server {
        #[arg(long, short, env = "CONFIG_PATH")]
        config_path: String,
        #[arg(short = 'i', long)]
        import: Option<PathBuf>,
    },
    /// Start worker only.
    Worker {
        #[arg(long, short, env = "CONFIG_PATH")]
        config_path: String,
        #[arg(short = 'i', long)]
        import: Option<PathBuf>,
    },
    /// Start server and worker in one process.
    Shared {
        #[arg(long, short, env = "CONFIG_PATH")]
        config_path: String,
        #[arg(short = 'i', long)]
        import: Option<PathBuf>,
    },
    /// Export flow definitions to stdout or a file.
    Export {
        /// Export specific target (session type, flow type, step type, or human id)
        target: Option<String>,
        /// Export all bundled definitions
        #[arg(long)]
        all: bool,
        /// Output file path. If omitted, writes to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Import flow definitions from JSON/YAML file or directory.
    Import {
        path: PathBuf,
        #[arg(long)]
        dry_run: bool,
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
