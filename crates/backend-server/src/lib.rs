mod api;
mod services;
mod sms_retry;
mod state;

use axum::Router;
use axum::routing::get;
use backend_auth::attach_request_context;
use backend_core::{Config, Result};
use std::sync::Arc;
use tracing::info;

pub async fn serve(core_config: &Config) -> Result<()> {
    let listen_addr = core_config.api_listen_addr()?;
    let state = Arc::new(state::AppState::from_config(core_config).await?);

    // Spawn the in-process SNS retry worker.
    sms_retry::spawn(state.clone());

    let router = router(state.clone());

    info!("Listening on {}", listen_addr);

    let handle = axum_server::Handle::new();
    let shutdown_handle = handle.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        shutdown_handle.graceful_shutdown(None);
    });

    match core_config.api_tls_files() {
        Some((cert_path, key_path)) => {
            let rustls_config =
                axum_server::tls_rustls::RustlsConfig::from_pem_file(cert_path, key_path)
                    .await?;
            axum_server::bind_rustls(listen_addr, rustls_config)
                .handle(handle)
                .serve(router.into_make_service())
                .await?;
        }
        None => {
            axum_server::bind(listen_addr)
                .handle(handle)
                .serve(router.into_make_service())
                .await?;
        }
    }

    Ok(())
}

fn router(state: Arc<state::AppState>) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .merge(api::router(state))
        .layer(axum::middleware::from_fn(attach_request_context))
}
