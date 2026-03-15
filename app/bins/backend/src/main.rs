#![allow(clippy::result_large_err)]
mod branding;

#[allow(unused_imports)]
use openssl_sys as _;

use backend_core::{Cli, Commands, Result, RuntimeMode, init_tracing, load_from_path};
use backend_flow_sdk::export::export_flow_definition;
use backend_flow_sdk::flow::{FlowDefinition, FlowMetadata, FlowSpec, FlowStepDefinition};
use backend_flow_sdk::{Actor, ExportFormat, ImportFormat, import_flow_definition};
use backend_server::{run_worker, serve};
use branding::banner::BANNER;
use clap::Parser;
use mimalloc::MiMalloc;
use std::path::Path;
use tracing::info;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[tokio::main]
async fn main() -> Result<()> {
    print!("{}", BANNER);

    match Cli::parse().command {
        Some(Commands::Serve {
            config_path,
            mode,
            import,
        }) => {
            run_runtime(&config_path, mode, import.as_deref()).await?;
        }
        Some(Commands::Server {
            config_path,
            import,
        }) => {
            run_runtime(&config_path, RuntimeMode::Server, import.as_deref()).await?;
        }
        Some(Commands::Worker {
            config_path,
            import,
        }) => {
            run_runtime(&config_path, RuntimeMode::Worker, import.as_deref()).await?;
        }
        Some(Commands::Shared {
            config_path,
            import,
        }) => {
            run_runtime(&config_path, RuntimeMode::Shared, import.as_deref()).await?;
        }
        Some(Commands::Export {
            target,
            all,
            output,
        }) => {
            run_export(target.as_deref(), all, output.as_deref())?;
        }
        Some(Commands::Import { file, dry_run }) => {
            import_definition_file(&file)?;
            if dry_run {
                info!(file = %file.display(), "import dry-run validation succeeded");
            } else {
                info!(file = %file.display(), "flow definition import validated");
            }
        }
        Some(Commands::Migrate { config_path }) => {
            let config = load_from_path(&config_path)?;
            init_tracing(&config.logging);

            backend_migrate::connect_postgres_and_migrate(&config.database.url).await?;
        }
        Some(Commands::Config { config_path }) => {
            let _ = load_from_path(&config_path)?;
        }
        None => {
            info!("No command provided. Use --help for more information.");
        }
    }

    Ok(())
}

async fn run_runtime(config_path: &str, mode: RuntimeMode, import: Option<&Path>) -> Result<()> {
    let mut config = load_from_path(config_path)?;
    config.runtime.mode = mode;
    init_tracing(&config.logging);

    if let Some(path) = import {
        import_definition_file(path)?;
        info!(file = %path.display(), "validated startup import definition");
    }

    match config.runtime.mode {
        RuntimeMode::Server => {
            info!("starting in server mode");
            serve(&config).await?;
        }
        RuntimeMode::Worker => {
            info!("starting in worker mode");
            run_worker(&config).await?;
        }
        RuntimeMode::Shared => {
            info!("starting in shared mode");
            tokio::try_join!(serve(&config), run_worker(&config))?;
        }
    }

    Ok(())
}

fn import_definition_file(path: &Path) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let format = ImportFormat::from_path(path);

    let parsed: serde_json::Value = match format {
        ImportFormat::Json => serde_json::from_str(&content)
            .map_err(|error| backend_core::Error::Server(error.to_string()))?,
        ImportFormat::Yaml => serde_yaml::from_str(&content)
            .map_err(|error| backend_core::Error::Server(error.to_string()))?,
    };

    let kind = parsed
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();

    if kind.eq_ignore_ascii_case("flow") {
        import_flow_definition(&content, format)
            .map_err(|error| backend_core::Error::Server(error.to_string()))?;
    } else {
        return Err(backend_core::Error::bad_request(
            "UNSUPPORTED_IMPORT_KIND",
            "Only Flow definitions are currently supported",
        ));
    }

    Ok(())
}

fn run_export(target: Option<&str>, all: bool, output: Option<&Path>) -> Result<()> {
    let definitions = bundled_flow_definitions();

    let selected = if all || target.is_none() {
        definitions
    } else {
        definitions
            .into_iter()
            .filter(|definition| {
                definition
                    .metadata
                    .flow_type
                    .eq_ignore_ascii_case(target.unwrap_or_default())
            })
            .collect()
    };

    if selected.is_empty() {
        return Err(backend_core::Error::not_found(
            "FLOW_DEFINITION_NOT_FOUND",
            "No matching flow definitions found",
        ));
    }

    let format = output
        .map(|path| {
            if path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
            {
                ExportFormat::Json
            } else {
                ExportFormat::Yaml
            }
        })
        .unwrap_or(ExportFormat::Yaml);

    let payload = if selected.len() == 1 {
        export_flow_definition(&selected[0], format)
            .map_err(|error| backend_core::Error::Server(error.to_string()))?
    } else {
        backend_flow_sdk::export_registry(&selected, format)
            .map_err(|error| backend_core::Error::Server(error.to_string()))?
    };

    if let Some(path) = output {
        std::fs::write(path, payload)?;
        info!(file = %path.display(), "flow definition export written");
    } else {
        println!("{payload}");
    }

    Ok(())
}

fn bundled_flow_definitions() -> Vec<FlowDefinition> {
    vec![
        FlowDefinition {
            api_version: "flow/v1".to_owned(),
            kind: "Flow".to_owned(),
            metadata: FlowMetadata {
                flow_type: "PHONE_OTP".to_owned(),
                human_id_prefix: "phone_otp".to_owned(),
                feature: Some("flow-phone-otp".to_owned()),
            },
            spec: FlowSpec {
                steps: vec![
                    FlowStepDefinition {
                        step_type: "SEND_OTP".to_owned(),
                        actor: Actor::System,
                        human_id: "send".to_owned(),
                        feature: Some("flow-phone-otp".to_owned()),
                        config: Some(serde_json::json!({"ttl_seconds": 300})),
                        on_success: Some("VERIFY_OTP".to_owned()),
                        on_failure: Some("FAILED".to_owned()),
                    },
                    FlowStepDefinition {
                        step_type: "VERIFY_OTP".to_owned(),
                        actor: Actor::EndUser,
                        human_id: "verify".to_owned(),
                        feature: Some("flow-phone-otp".to_owned()),
                        config: Some(serde_json::json!({"max_attempts": 5})),
                        on_success: Some("COMPLETE".to_owned()),
                        on_failure: Some("FAILED".to_owned()),
                    },
                ],
            },
        },
        FlowDefinition {
            api_version: "flow/v1".to_owned(),
            kind: "Flow".to_owned(),
            metadata: FlowMetadata {
                flow_type: "EMAIL_MAGIC".to_owned(),
                human_id_prefix: "email_magic".to_owned(),
                feature: Some("flow-email-magic".to_owned()),
            },
            spec: FlowSpec {
                steps: vec![
                    FlowStepDefinition {
                        step_type: "ISSUE_MAGIC".to_owned(),
                        actor: Actor::System,
                        human_id: "issue".to_owned(),
                        feature: Some("flow-email-magic".to_owned()),
                        config: Some(serde_json::json!({"ttl_seconds": 900})),
                        on_success: Some("VERIFY_MAGIC".to_owned()),
                        on_failure: Some("FAILED".to_owned()),
                    },
                    FlowStepDefinition {
                        step_type: "VERIFY_MAGIC".to_owned(),
                        actor: Actor::EndUser,
                        human_id: "verify".to_owned(),
                        feature: Some("flow-email-magic".to_owned()),
                        config: Some(serde_json::json!({})),
                        on_success: Some("COMPLETE".to_owned()),
                        on_failure: Some("FAILED".to_owned()),
                    },
                ],
            },
        },
        FlowDefinition {
            api_version: "flow/v1".to_owned(),
            kind: "Flow".to_owned(),
            metadata: FlowMetadata {
                flow_type: "FIRST_DEPOSIT".to_owned(),
                human_id_prefix: "first_deposit".to_owned(),
                feature: Some("flow-first-deposit".to_owned()),
            },
            spec: FlowSpec {
                steps: vec![
                    FlowStepDefinition {
                        step_type: "AWAIT_PAYMENT_CONFIRMATION".to_owned(),
                        actor: Actor::Admin,
                        human_id: "await_payment".to_owned(),
                        feature: Some("flow-first-deposit".to_owned()),
                        config: Some(serde_json::json!({})),
                        on_success: Some("APPROVE_AND_DEPOSIT".to_owned()),
                        on_failure: Some("FAILED".to_owned()),
                    },
                    FlowStepDefinition {
                        step_type: "APPROVE_AND_DEPOSIT".to_owned(),
                        actor: Actor::System,
                        human_id: "approve".to_owned(),
                        feature: Some("flow-first-deposit".to_owned()),
                        config: Some(serde_json::json!({})),
                        on_success: Some("COMPLETE".to_owned()),
                        on_failure: Some("FAILED".to_owned()),
                    },
                ],
            },
        },
    ]
}
