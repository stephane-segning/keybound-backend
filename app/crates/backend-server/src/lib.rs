pub(crate) mod api;
pub(crate) mod sms_retry;
pub(crate) mod state;

use axum::body::Body;
use backend_auth::{require_bff_auth, require_kc_signature, require_staff_bearer};
use backend_core::{Config, Result};
use http::{Request, Response, StatusCode};
use std::sync::Arc;
use tower::make::Shared;
use tower::service_fn;
use tracing::info;

pub async fn serve(core_config: &Config) -> Result<()> {
    let listen_addr = core_config.api_listen_addr()?;
    let state = Arc::new(state::AppState::from_config(core_config).await?);

    // Spawn the in-process SNS retry worker.
    sms_retry::spawn(state.clone());

    let api = api::BackendApi::new(state.clone());
    let make_svc = Shared::new(service_fn(move |req: Request<hyper::body::Incoming>| {
        let api = api.clone();
        let state = state.clone();
        async move { Ok::<Response<Body>, std::convert::Infallible>(dispatch(api, state, req).await) }
    }));

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
                axum_server::tls_rustls::RustlsConfig::from_pem_file(cert_path, key_path).await?;

            axum_server::bind_rustls(listen_addr, rustls_config)
                .handle(handle)
                .serve(make_svc)
                .await?;
        }
        None => {
            axum_server::bind(listen_addr)
                .handle(handle)
                .serve(make_svc)
                .await?;
        }
    }

    Ok(())
}

async fn dispatch(
    api: api::BackendApi,
    state: Arc<state::AppState>,
    req: Request<hyper::body::Incoming>,
) -> Response<Body> {
    let req = req.map(Body::new);
    let path = req.uri().path().to_owned();

    if path.starts_with("/v1/") {
        let req = match require_kc_signature(&state.config.kc, req).await {
            Ok(req) => req,
            Err(resp) => return resp,
        };
        return state::call_kc(api, req).await;
    }

    if path.starts_with("/api/registration/") {
        let req = match require_bff_auth(&state.config.bff, req).await {
            Ok(req) => req,
            Err(resp) => return resp,
        };
        return state::call_bff(api, req).await;
    }

    if path.starts_with("/api/kyc/staff/") {
        let req = match require_staff_bearer(&state.config.staff, req).await {
            Ok(req) => req,
            Err(resp) => return resp,
        };
        return state::call_staff(api, req).await;
    }

    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from("Not found"))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}
