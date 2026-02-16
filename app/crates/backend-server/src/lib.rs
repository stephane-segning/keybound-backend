pub(crate) mod api;
pub(crate) mod sms_retry;
pub(crate) mod state;
pub(crate) mod worker;

use axum::Router;
use axum::body::Body;
use backend_auth::{bff_bearer_layer, kc_signature_layer, staff_bearer_layer};
use backend_core::{Config, Result};
use http::uri::PathAndQuery;
use http::{Request, Response, StatusCode, Uri};
use std::sync::Arc;
use tower::make::Shared;
use tower::service_fn;
use tracing::{error, info};

pub async fn serve(core_config: &Config) -> Result<()> {
    let listen_addr = core_config.api_listen_addr()?;
    let state = Arc::new(state::AppState::from_config(core_config).await?);

    let api = api::BackendApi::new(state.clone());
    let router = build_router(&api, &state.config);
    let make_svc = Shared::new(service_fn(move |req: Request<hyper::body::Incoming>| {
        let router = router.clone();
        async move {
            let req = req.map(Body::new);
            match router.oneshot(req).await {
                Ok(resp) => Ok::<Response<Body>, std::convert::Infallible>(resp),
                Err(err) => {
                    error!(error = %err, "router request failed");
                    Ok(Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from("Internal server error"))
                        .unwrap_or_else(|_| Response::new(Body::empty())))
                }
            }
        }
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

pub async fn run_worker(core_config: &Config) -> Result<()> {
    let state = Arc::new(state::AppState::from_config(core_config).await?);
    worker::run(state).await
}

fn build_router(api: &api::BackendApi, config: &Config) -> Router {
    let router = Router::new();
    let router = nest_surface_router(router, &config.kc.base_path, {
        let api = api.clone();
        let cfg = config.kc.clone();
        move |normalized| build_kc_router(api.clone(), cfg.clone(), normalized)
    });
    let router = nest_surface_router(router, &config.bff.base_path, {
        let api = api.clone();
        let cfg = config.bff.clone();
        move |normalized| build_bff_router(api.clone(), cfg.clone(), normalized)
    });
    nest_surface_router(router, &config.staff.base_path, {
        let api = api.clone();
        let cfg = config.staff.clone();
        move |normalized| build_staff_router(api.clone(), cfg.clone(), normalized)
    })
}

fn build_kc_router(api: api::BackendApi, cfg: backend_core::KcAuth, base_path: String) -> Router {
    let base_path = Arc::new(base_path);
    Router::new()
        .fallback(service_fn(move |req| {
            let api = api.clone();
            let base_path = base_path.clone();
            async move {
                let req = add_base_path(req, base_path.as_str());
                state::call_kc(api, req).await
            }
        }))
        .layer(kc_signature_layer(cfg))
}

fn build_bff_router(api: api::BackendApi, cfg: backend_core::BffAuth, base_path: String) -> Router {
    let base_path = Arc::new(base_path);
    Router::new()
        .fallback(service_fn(move |req| {
            let api = api.clone();
            let base_path = base_path.clone();
            async move {
                let req = add_base_path(req, base_path.as_str());
                state::call_bff(api, req).await
            }
        }))
        .layer(bff_bearer_layer(cfg))
}

fn build_staff_router(
    api: api::BackendApi,
    cfg: backend_core::StaffAuth,
    base_path: String,
) -> Router {
    let base_path = Arc::new(base_path);
    Router::new()
        .fallback(service_fn(move |req| {
            let api = api.clone();
            let base_path = base_path.clone();
            async move {
                let req = add_base_path(req, base_path.as_str());
                state::call_staff(api, req).await
            }
        }))
        .layer(staff_bearer_layer(cfg))
}

fn nest_surface_router<F>(router: Router, base_path: &str, build_child: F) -> Router
where
    F: FnOnce(String) -> Router,
{
    if let Some(normalized) = normalize_base_path(base_path) {
        router.nest(&normalized, build_child(normalized))
    } else {
        router
    }
}

fn normalize_base_path(base_path: &str) -> Option<String> {
    let trimmed = base_path.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut normalized = trimmed.to_string();
    if !normalized.starts_with('/') {
        normalized.insert(0, '/');
    }
    while normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }
    if normalized == "/" {
        None
    } else {
        Some(normalized)
    }
}

fn add_base_path(mut req: Request<Body>, base_path: &str) -> Request<Body> {
    if base_path.is_empty() {
        return req;
    }

    let path = req.uri().path();
    let query = req.uri().query();
    let base = base_path;
    let normalized_path = if path == "/" {
        base.to_string()
    } else {
        format!("{base}{path}")
    };
    let path_with_query = if let Some(query) = query {
        format!("{normalized_path}?{query}")
    } else {
        normalized_path
    };

    let mut parts = req.uri().clone().into_parts();
    parts.path_and_query =
        Some(PathAndQuery::from_maybe_shared(path_with_query).expect("valid path and query"));
    *req.uri_mut() = Uri::from_parts(parts).expect("valid uri");
    req
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::StatusCode;
    use axum::routing::get;
    use tower::ServiceExt;

    #[tokio::test]
    async fn non_blank_base_path_mounts_service() {
        let router = nest_surface_router(Router::new(), "/api/test", |_normalized| {
            Router::new().route("/", get(|| async { "mounted" }))
        });

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn blank_base_path_does_not_mount_service() {
        let router = Router::new().route("/ping", get(|| async { "pong" }));
        let router = nest_surface_router(router, "   ", |_normalized| {
            panic!("should not nest when base path is blank");
        });

        let response = router
            .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
