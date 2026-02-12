mod api;
mod services;
mod sms_retry;
mod state;

use axum::Router;
use axum::routing::get;
use backend_core::{Config, Result};
use http::{Request, Response, StatusCode};
use std::convert::Infallible;
use std::sync::Arc;
use tower::service_fn;
use tracing::{error, info};

pub async fn serve(core_config: &Config) -> Result<()> {
    let listen_addr = core_config.api_listen_addr()?;
    let state = Arc::new(state::AppState::from_config(core_config).await?);

    // Spawn the in-process SNS retry worker.
    sms_retry::spawn(state.clone());

    let api = api::BackendApi::new(state.clone());
    let router = router(api.clone());

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
                    .await
                    .map_err(|e| backend_core::Error::Server(e.to_string()))?;
            axum_server::bind_rustls(listen_addr, rustls_config)
                .handle(handle)
                .serve(router.into_make_service())
                .await
                .map_err(|e| backend_core::Error::Server(e.to_string()))?;
        }
        None => {
            axum_server::bind(listen_addr)
                .handle(handle)
                .serve(router.into_make_service())
                .await
                .map_err(|e| backend_core::Error::Server(e.to_string()))?;
        }
    }

    Ok(())
}

fn router(api: api::BackendApi) -> Router {
    let fallback = service_fn(move |req: Request<axum::body::Body>| {
        let api = api.clone();
        async move { Ok::<Response<axum::body::Body>, Infallible>(dispatch(api, req).await) }
    });

    Router::new()
        .route("/health", get(|| async { "ok" }))
        .fallback_service(fallback)
}

async fn dispatch(
    api: api::BackendApi,
    req: Request<axum::body::Body>,
) -> Response<axum::body::Body> {
    let path = req.uri().path();

    if path.starts_with("/v1/") {
        return state::call_kc(api, req).await;
    }

    if path.starts_with("/api/registration/") {
        let ctx = backend_auth::ServiceContext::from_request(&req);
        return state::call_bff(api, ctx, req).await;
    }

    if path.starts_with("/api/kyc/staff/") {
        let ctx = backend_auth::ServiceContext::from_request(&req);
        return state::call_staff(api, ctx, req).await;
    }

    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(axum::body::Body::from("Not found"))
        .unwrap_or_else(|e| {
            error!("failed to build 404 response: {e}");
            Response::new(axum::body::Body::empty())
        })
}
