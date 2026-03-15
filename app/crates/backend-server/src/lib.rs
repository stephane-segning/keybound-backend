//! Main backend server library for the tokenization backend.
#![allow(clippy::result_large_err)]
//!
//! This crate provides the HTTP server, API surfaces (KC, BFF, Staff),
//! state machine engine, background workers, and all application logic.
//! It handles user storage, device binding, KYC flows, and integrations.

pub(crate) mod api;
pub(crate) mod file_storage;
pub(crate) mod health;
pub(crate) mod state;
pub(crate) mod state_machine;
#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;
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

/// Starts the HTTP server with all API surfaces and background workers.
///
/// This is the main entry point for running the application server.
/// It initializes the database connection, application state, builds the router,
/// and starts listening for requests.
///
/// # Arguments
/// * `core_config` - Application configuration
///
/// # Returns
/// `Result<()>` indicating successful server shutdown or error
///
/// # Errors
/// Returns an error if initialization fails or the server cannot start
pub async fn serve(core_config: &Config) -> Result<()> {
    let listen_addr = core_config.api_listen_addr()?;
    let pool = connect_postgres_and_migrate(&core_config.database.url).await?;
    let state = Arc::new(state::AppState::from_config(core_config, pool).await?);

    let api = api::BackendApi::new(
        state.clone(),
        state.oidc_state.clone(),
        state.signature_state.clone(),
    );
    let app = build_router(api, &state.config, state.oidc_state.clone());

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

/// Runs the background worker for async tasks and state machine processing.
///
/// This function starts the worker that processes state machine steps and notifications.
/// It acquires a distributed lock to ensure only one worker instance runs at a time.
/// An optional health check server is started if in worker-only mode.
///
/// # Arguments
/// * `core_config` - Application configuration
///
/// # Returns
/// `Result<()>` indicating successful worker shutdown or error
///
/// # Errors
/// Returns an error if Redis is unavailable or worker initialization fails
pub async fn run_worker(core_config: &Config) -> Result<()> {
    let pool = connect_postgres_and_migrate(&core_config.database.url).await?;
    let _conn = pool
        .get()
        .await
        .map_err(|error| backend_core::Error::DieselPool(error.to_string()))?;
    worker::ensure_redis_ready(&core_config.redis.url).await?;
    let worker_lock = worker::acquire_worker_consumer_lock(&core_config.redis.url).await?;

    let state = Arc::new(state::AppState::from_config(core_config, pool).await?);

    let health_server = if core_config.runtime.mode == backend_core::RuntimeMode::Worker {
        let listen_addr = core_config.api_listen_addr()?;
        let health_app = health::health_router();

        info!("Worker health check listening on {}", listen_addr);

        let handle = axum_server::Handle::new();
        let shutdown_handle = handle.clone();

        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            shutdown_handle.graceful_shutdown(None);
        });

        let tls_files = core_config.api_tls_files();
        Some(tokio::spawn(async move {
            match tls_files {
                Some((cert_path, key_path)) => {
                    let rustls_config =
                        axum_server::tls_rustls::RustlsConfig::from_pem_file(cert_path, key_path)
                            .await
                            .expect("failed to load tls config for worker health");

                    axum_server::bind_rustls(listen_addr, rustls_config)
                        .handle(handle)
                        .serve(health_app.into_make_service())
                        .await
                        .expect("worker health server failed");
                }
                None => {
                    axum_server::bind(listen_addr)
                        .handle(handle)
                        .serve(health_app.into_make_service())
                        .await
                        .expect("worker health server failed");
                }
            }
        }))
    } else {
        None
    };

    let worker_res = worker::run(state).await;
    if let Err(error) = worker_lock.release().await {
        tracing::warn!("failed to release worker consumer lock: {}", error);
    }
    if let Some(hs) = health_server {
        hs.abort();
    }
    worker_res
}

/// Builds the main Axum router with all API surfaces and middleware.
///
/// Configures and mounts all API routers (KC, BFF, Staff) onto the main router.
/// Applies authentication layers (KC signature verification and JWT validation)
/// and request logging middleware based on configuration.
///
/// # Arguments
/// * `api` - Backend API handler
/// * `config` - Application configuration
/// * `oidc_state` - OIDC authentication state
///
/// # Returns
/// Configured `Router` ready to serve requests
fn build_router(
    api: api::BackendApi,
    config: &Config,
    oidc_state: Arc<backend_auth::OidcState>,
) -> Router {
    // Mount sub-routers onto a fresh root router
    let mut router = Router::new().merge(health::health_router());

    // Mount KC router if base path is provided
    let kc_base = config.kc.base_path.trim();
    if !kc_base.is_empty() && kc_base != "/" {
        let layer = kc_signature_layer(config.kc.enabled, api.signature_state.clone());
        let kc_router = gen_oas_server_kc::server::new(api.clone()).layer(layer);
        router = router.nest(kc_base, kc_router);
    }

    // Mount BFF router if base path is provided
    let bff_base = config.bff.base_path.trim();
    if !bff_base.is_empty() && bff_base != "/" {
        let bff_router = gen_oas_server_bff::server::new(api.clone());
        let bff_revamp_router = api::bff_revamp::router(api.clone());
        let bff_router = bff_router.merge(bff_revamp_router);
        router = router.nest(bff_base, bff_router);
    }

    // Mount Staff router if base path is provided
    let staff_base = config.staff.base_path.trim();
    if !staff_base.is_empty() && staff_base != "/" {
        let staff_router = gen_oas_server_staff::server::new(api.clone());
        router = router.nest(staff_base, staff_router);
    }

    // Mount Auth router if base path is provided
    let auth_base = config.auth.base_path.trim();
    if config.auth.enabled && !auth_base.is_empty() && auth_base != "/" {
        let auth_router = api::auth::router(api.clone());
        router = router.nest(auth_base, auth_router);
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
    let mut jwks_base_paths: Vec<String> = config
        .oauth2
        .base_paths
        .iter()
        .map(|p| p.trim().to_owned())
        .filter(|p| !p.is_empty() && p != "/")
        .collect();
    if jwks_base_paths.is_empty() {
        let mut defaults: Vec<&str> = Vec::new();
        if config.bff.enabled {
            defaults.push(&config.bff.base_path);
        }
        if config.staff.enabled {
            defaults.push(&config.staff.base_path);
        }
        jwks_base_paths.extend(
            defaults
                .iter()
                .map(|p| p.trim().to_owned())
                .filter(|p| !p.is_empty() && p != "/"),
        );
    }

    router = router.layer(jwks_auth_layer(oidc_state, jwks_base_paths));

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

/// Extracts the request path for logging purposes.
///
/// Prefers the OriginalUri extension if available (for nested routers),
/// otherwise falls back to the request's direct URI path.
///
/// # Arguments
/// * `req` - HTTP request
///
/// # Returns
/// String representation of the request path
fn request_path(req: &HttpRequest<Body>) -> String {
    req.extensions()
        .get::<axum::extract::OriginalUri>()
        .map(|uri| uri.0.path().to_owned())
        .unwrap_or_else(|| req.uri().path().to_owned())
}
