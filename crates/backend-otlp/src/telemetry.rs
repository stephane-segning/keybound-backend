use backend_core::Logging;
use std::fs::File;
use std::sync::OnceLock;
use std::{env, fs, path::Path, path::PathBuf};
use tracing_error::ErrorLayer;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

// Keep the guard alive for non-blocking writers; dropping it stops flushing to disk.
static FILE_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

/// Initialize tracing for binaries.
///
/// - Honors `RUST_LOG` if set.
/// - Falls back to `default_filter` (typically `"info"`).
pub fn init_tracing(config: &Logging) {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| config.level.clone().into());
    let mut file_guard_slot = None;
    let mut writer = None;

    if let Some(dir) = config.data_dir.as_ref() {
        if let Err(e) = fs::create_dir_all(Path::new(dir)) {
            return eprintln!("Failed to create SOMA_LOGS_DIR ({}): {}", dir, e);
        }

        let appender = tracing_appender::rolling::weekly(dir, "log");
        let (non_blocking_appender, guard) = tracing_appender::non_blocking(appender);

        // Store the guard so the background worker keeps flushing to disk.
        file_guard_slot = Some(guard);

        writer = Some(non_blocking_appender);
    }

    let fmt_layer = if let Some(true) = config.json {
        let layer = fmt::layer().json();
        if let Some(writer) = writer {
            layer.with_writer(writer).boxed()
        } else {
            layer.boxed()
        }
    } else {
        let layer = fmt::layer();
        if let Some(writer) = writer {
            layer.with_writer(writer).boxed()
        } else {
            layer.boxed()
        }
    };

    let subscriber = tracing_subscriber::registry()
        .with(env_filter)
        .with(ErrorLayer::default())
        .with(fmt_layer);

    let res = if let Some(true) = config.flame {
        let flame_dir = flame_output_dir(config.data_dir.as_deref());
        if let Err(e) = fs::create_dir_all(&flame_dir) {
            eprintln!(
                "Failed to create flame output dir ({}): {}",
                flame_dir.display(),
                e
            );
            subscriber.try_init()
        } else {
            let flame_file = flame_dir.join(format!("{}.folded", current_bin_name()));
            match File::create(&flame_file) {
                Ok(file) => subscriber
                    .with(tracing_flame::FlameLayer::new(file))
                    .try_init(),
                Err(e) => {
                    eprintln!(
                        "Failed to create flame output file at {}: {}",
                        flame_file.display(),
                        e
                    );
                    subscriber.try_init()
                }
            }
        }
    } else {
        subscriber.try_init()
    };

    if let Some(guard) = file_guard_slot {
        let _ = FILE_GUARD.set(guard);
    }

    if let Err(e) = res {
        eprintln!("Failed to initialize tracing: {}", e);
    }
}

fn flame_output_dir(logs_dir: Option<&str>) -> PathBuf {
    if let Some(dir) = logs_dir {
        let logs_path = Path::new(dir);
        let base = logs_path
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or_else(|| Path::new("."));
        base.join("flame")
    } else {
        PathBuf::from("flame")
    }
}

fn current_bin_name() -> String {
    env::current_exe()
        .ok()
        .and_then(|path| path.file_stem().map(|os| os.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "soma".to_string())
}

fn bool_env(var: &str, truthy: &[&str]) -> bool {
    env::var(var)
        .map(|value| truthy.contains(&value.trim().to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}
