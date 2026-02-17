pub(crate) mod api;
pub(crate) mod sms_retry;
pub(crate) mod state;
pub(crate) mod worker;

use axum::Router;
use axum::body::Body;
use axum::http::Request as HttpRequest;
use axum::response::Response;
use backend_auth::{jwks_auth_layer, kc_signature_layer};
use backend_core::{Config, Result};
use backend_migrate::connect_postgres_and_migrate;
use hyper::StatusCode;
use std::convert::Infallible;
use std::sync::Arc;
use tower::service_fn;
use tower_http::trace::TraceLayer;
use tracing::info;

pub async fn serve(core_config: &Config) -> Result<()> {
    let listen_addr = core_config.api_listen_addr()?;
    let pool = connect_postgres_and_migrate(&core_config.database.url).await?;
    let state = Arc::new(state::AppState::from_config(core_config, pool).await?);

    let api = api::BackendApi::new(state.clone());
    let app = build_router(api, &state.config);

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
                .serve(app.into_make_service())
                .await?;
        }
        None => {
            axum_server::bind(listen_addr)
                .handle(handle)
                .serve(app.into_make_service())
                .await?;
        }
    }

    Ok(())
}

pub async fn run_worker(core_config: &Config) -> Result<()> {
    let pool = connect_postgres_and_migrate(&core_config.database.url).await?;
    let state = Arc::new(state::AppState::from_config(core_config, pool).await?);
    worker::run(state).await
}

fn build_router(api: api::BackendApi, config: &Config) -> Router {
    // Mount sub-routers onto a fresh root router
    let mut router = Router::new();

    // Mount KC router if base path is provided
    let kc_base = config.kc.base_path.trim();
    if !kc_base.is_empty() && kc_base != "/" {
        let kc_router = build_kc_router(api.clone(), config.kc.clone());
        router = router.nest(kc_base, kc_router);
    }

    // Mount BFF router if base path is provided
    let bff_base = config.bff.base_path.trim();
    if !bff_base.is_empty() && bff_base != "/" {
        let bff_router = build_bff_router(api.clone(), config.bff.clone());
        router = router.nest(bff_base, bff_router);
    }

    // Mount Staff router if base path is provided
    let staff_base = config.staff.base_path.trim();
    if !staff_base.is_empty() && staff_base != "/" {
        let staff_router = build_staff_router(api.clone(), config.staff.clone());
        router = router.nest(staff_base, staff_router);
    }

    // 404 fallback for unmatched routes
    router = router.fallback_service(service_fn(|_| async {
        let res = Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Not Found"))
            .unwrap();
        Ok::<_, Infallible>(res)
    }));

    // Apply JWKS auth layer
    let mut jwks_base_paths = config.oauth2.base_paths.clone();
    if jwks_base_paths.is_empty() {
        jwks_base_paths.push(config.bff.base_path.clone());
        jwks_base_paths.push(config.staff.base_path.clone());
    }

    router = router.layer(jwks_auth_layer(
        config.oauth2.jwks_url.clone(),
        jwks_base_paths,
    ));

    if config.logging.log_requests_enabled {
        router.layer(
            TraceLayer::new_for_http().make_span_with(|req: &HttpRequest<_>| {
                tracing::info_span!(
                    "http-request",
                    method = %req.method(),
                    path = %request_path(req)
                )
            }),
        )
    } else {
        router
    }
}

fn build_kc_router(api: api::BackendApi, cfg: backend_core::KcAuth) -> Router {
    let layer = kc_signature_layer(cfg);
    let router = gen_oas_server_kc::server::new(api);
    router.layer(layer)
}

fn build_bff_router(api: api::BackendApi, _cfg: backend_core::BffAuth) -> Router {
    gen_oas_server_bff::server::new(api)
}

fn build_staff_router(api: api::BackendApi, _cfg: backend_core::StaffAuth) -> Router {
    gen_oas_server_staff::server::new(api)
}

fn request_path(req: &HttpRequest<Body>) -> String {
    req.extensions()
        .get::<axum::extract::OriginalUri>()
        .map(|uri| uri.0.path().to_owned())
        .unwrap_or_else(|| req.uri().path().to_owned())
}
