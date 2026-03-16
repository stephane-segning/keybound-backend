#![allow(clippy::result_large_err)]
mod branding;

#[allow(unused_imports)]
use openssl_sys as _;

use backend_core::{Cli, Commands, Result, RuntimeMode, init_tracing, load_from_path};
use backend_flow_sdk::export::{export_flow_definition, export_session_definition};
use backend_flow_sdk::flow::{FlowDefinition, FlowMetadata, FlowSpec, FlowStepDefinition};
use backend_flow_sdk::{ExportFormat, ImportFormat, SessionDefinition, import_flow_definition};
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
        Some(Commands::Import { path, dry_run }) => {
            let imports = load_imports(&path)?;
            if dry_run {
                info!(path = %path.display(), flows = imports.flows.len(), sessions = imports.sessions.len(), "import dry-run validation succeeded");
            } else {
                // If not dry-run, we should probably print that we can't persist them without DB,
                // but wait, imports are runtime only in this system right now!
                // "Startup import becomes real runtime behavior" implies they're just loaded into the registry at boot!
                info!(path = %path.display(), flows = imports.flows.len(), sessions = imports.sessions.len(), "flow definition import validated");
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

    let imports = if let Some(path) = import {
        let loaded = load_imports(path)?;
        info!(path = %path.display(), flows = loaded.flows.len(), sessions = loaded.sessions.len(), "validated startup import definitions");
        loaded
    } else {
        backend_server::flow_registry::RegistryImports::default()
    };

    match config.runtime.mode {
        RuntimeMode::Server => {
            info!("starting in server mode");
            serve(&config, imports).await?;
        }
        RuntimeMode::Worker => {
            info!("starting in worker mode");
            run_worker(&config, imports).await?;
        }
        RuntimeMode::Shared => {
            info!("starting in shared mode");
            tokio::try_join!(
                serve(&config, imports.clone()),
                run_worker(&config, imports)
            )?;
        }
    }

    Ok(())
}

fn load_imports(path: &Path) -> Result<backend_server::flow_registry::RegistryImports> {
    let mut imports = backend_server::flow_registry::RegistryImports::default();

    if path.is_dir() {
        let entries = std::fs::read_dir(path)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                    if ext.eq_ignore_ascii_case("json")
                        || ext.eq_ignore_ascii_case("yaml")
                        || ext.eq_ignore_ascii_case("yml")
                    {
                        parse_import_file(&path, &mut imports)?;
                    }
                }
            }
        }
    } else {
        parse_import_file(path, &mut imports)?;
    }

    Ok(imports)
}

fn parse_import_file(
    path: &Path,
    imports: &mut backend_server::flow_registry::RegistryImports,
) -> Result<()> {
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
        let definition = import_flow_definition(&content, format)
            .map_err(|error| backend_core::Error::Server(error.to_string()))?;
        imports.flows.push(definition);
    } else if kind.eq_ignore_ascii_case("session") {
        let definition = backend_flow_sdk::import_session_definition(&content, format)
            .map_err(|error| backend_core::Error::Server(error.to_string()))?;
        imports.sessions.push(definition);
    } else {
        return Err(backend_core::Error::bad_request(
            "UNSUPPORTED_IMPORT_KIND",
            "Only Flow and Session definitions are supported",
        ));
    }

    Ok(())
}

fn run_export(target: Option<&str>, all: bool, output: Option<&Path>) -> Result<()> {
    let imports = backend_server::flow_registry::RegistryImports::default();
    let registry = backend_server::flow_registry::build_registry(imports)
        .map_err(|e| backend_core::Error::Server(e.to_string()))?;

    let mut flow_definitions = Vec::new();
    for flow_type in registry.flow_types() {
        if let Some(flow) = registry.get_flow(&flow_type) {
            let mut steps = Vec::new();
            for step in flow.steps() {
                let mut on_success = None;
                let mut on_failure = None;
                if let Some(transition) = flow.transitions().get(step.step_type()) {
                    on_success = Some(transition.on_success.clone());
                    on_failure = transition.on_failure.clone();
                }

                steps.push(FlowStepDefinition {
                    step_type: step.step_type().to_owned(),
                    actor: step.actor().clone(),
                    human_id: step.human_id().to_owned(),
                    feature: step.feature().map(|f| f.to_owned()),
                    config: Some(serde_json::json!({})),
                    on_success,
                    on_failure,
                });
            }

            flow_definitions.push(FlowDefinition {
                api_version: "flow/v1".to_owned(),
                kind: "Flow".to_owned(),
                metadata: FlowMetadata {
                    flow_type: flow.flow_type().to_owned(),
                    human_id_prefix: flow.human_id().to_owned(),
                    feature: flow.feature().map(|f| f.to_owned()),
                    override_existing: None,
                },
                spec: FlowSpec { steps },
            });
        }
    }

    let session_definitions: Vec<SessionDefinition> = registry
        .sessions()
        .into_iter()
        .map(|s| SessionDefinition {
            session_type: s.session_type.clone(),
            human_id_prefix: s.human_id_prefix.clone(),
            feature: s.feature.clone(),
            allowed_flows: s.allowed_flows.clone(),
            override_existing: None,
        })
        .collect();

    let (selected_flows, selected_sessions) = if all || target.is_none() {
        (flow_definitions, session_definitions)
    } else {
        let target_val = target.unwrap_or_default();
        let flows: Vec<FlowDefinition> = flow_definitions
            .into_iter()
            .filter(|d| d.metadata.flow_type.eq_ignore_ascii_case(target_val))
            .collect();
        let sessions: Vec<SessionDefinition> = session_definitions
            .into_iter()
            .filter(|s| s.session_type.eq_ignore_ascii_case(target_val))
            .collect();
        (flows, sessions)
    };

    if selected_flows.is_empty() && selected_sessions.is_empty() {
        return Err(backend_core::Error::not_found(
            "DEFINITION_NOT_FOUND",
            "No matching flow or session definitions found",
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

    let payload = if selected_flows.len() == 1 && selected_sessions.is_empty() {
        export_flow_definition(&selected_flows[0], format)
            .map_err(|error| backend_core::Error::Server(error.to_string()))?
    } else if selected_sessions.len() == 1 && selected_flows.is_empty() {
        export_session_definition(&selected_sessions[0], format)
            .map_err(|error| backend_core::Error::Server(error.to_string()))?
    } else {
        #[derive(serde::Serialize)]
        struct ExportBundle {
            flows: Vec<FlowDefinition>,
            sessions: Vec<SessionDefinition>,
        }
        let bundle = ExportBundle {
            flows: selected_flows,
            sessions: selected_sessions,
        };
        backend_flow_sdk::export_registry(&bundle, format)
            .map_err(|error| backend_core::Error::Server(error.to_string()))?
    };

    if let Some(path) = output {
        std::fs::write(path, payload)?;
        info!(file = %path.display(), "definition export written");
    } else {
        println!("{payload}");
    }

    Ok(())
}
