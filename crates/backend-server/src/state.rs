use axum::body::Body;
use backend_auth::{KcContext, ServiceContext};
use backend_core::{Aws, Config};
use backend_repository::PgRepository;
use bytes::Bytes;
use gen_oas_server_bff::models::{KycStatusResponse, LimitsResponse};
use http::{Request, Response, StatusCode};
use http_body_util::BodyExt as _;
use http_body_util::combinators::BoxBody;
use hyper::service::Service as HyperService;
use moka::future::Cache;
use sqlx::postgres::PgPoolOptions;
use std::convert::Infallible;
use std::time::Duration;
use tracing::error;

use crate::services::BackendService;

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub aws: Aws,
}

#[derive(Clone)]
pub struct HttpCache {
    pub kyc_status: Cache<String, KycStatusResponse>,
    pub limits: Cache<String, LimitsResponse>,
}

#[derive(Clone)]
pub struct AppState {
    pub repository: PgRepository,
    pub service: BackendService,
    pub s3: aws_sdk_s3::Client,
    pub sns: aws_sdk_sns::Client,
    pub config: RuntimeConfig,
    pub http_cache: HttpCache,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("repository", &"<PgRepository>")
            .field("service", &"<BackendService>")
            .field("s3", &"<S3Client>")
            .field("sns", &"<SnsClient>")
            .field("config", &self.config)
            .field("http_cache", &"<HttpCache>")
            .finish()
    }
}

impl AppState {
    pub async fn from_config(cfg: &Config) -> backend_core::Result<Self> {
        let db = PgPoolOptions::new()
            .max_connections(cfg.database_pool_size())
            .connect(&cfg.database.url)
            .await?;

        let shared_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_types::region::Region::new(cfg.aws.region.clone()))
            .load()
            .await;

        let s3 = {
            let mut builder = aws_sdk_s3::config::Builder::from(&shared_config);
            if let Some(endpoint) = &cfg.aws.s3.endpoint {
                builder = builder.endpoint_url(endpoint).force_path_style(true);
            }
            aws_sdk_s3::Client::from_conf(builder.build())
        };

        let sns = {
            let mut builder = aws_sdk_sns::config::Builder::from(&shared_config);
            if let Some(region) = &cfg.aws.sns.region {
                builder = builder.region(aws_types::region::Region::new(region.clone()));
            }
            aws_sdk_sns::Client::from_conf(builder.build())
        };

        let http_cache = HttpCache {
            kyc_status: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(Duration::from_secs(30))
                .build(),
            limits: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(Duration::from_secs(30))
                .build(),
        };

        let repository = PgRepository::new(db.clone());
        let service = BackendService::new(repository.clone());

        Ok(Self {
            repository,
            service,
            s3,
            sns,
            config: RuntimeConfig {
                aws: cfg.aws.clone(),
            },
            http_cache,
        })
    }

    pub async fn invalidate_bff_cache(&self, external_id: &str) {
        self.http_cache.kyc_status.invalidate(external_id).await;
        self.http_cache.limits.invalidate(external_id).await;
    }
}

pub async fn call_kc(api: crate::api::BackendApi, req: Request<Body>) -> Response<Body> {
    let ctx = KcContext::from_request(&req);
    let svc = gen_oas_server_kc::server::Service::new(api, false);
    to_axum_response(HyperService::call(&svc, (req, ctx)).await)
}

pub async fn call_bff(
    api: crate::api::BackendApi,
    ctx: ServiceContext,
    req: Request<Body>,
) -> Response<Body> {
    let svc = gen_oas_server_bff::server::Service::new(api, false);
    to_axum_response(HyperService::call(&svc, (req, ctx)).await)
}

pub async fn call_staff(
    api: crate::api::BackendApi,
    ctx: ServiceContext,
    req: Request<Body>,
) -> Response<Body> {
    let svc = gen_oas_server_staff::server::Service::new(api, false);
    to_axum_response(HyperService::call(&svc, (req, ctx)).await)
}

fn to_axum_response<E>(resp: Result<Response<BoxBody<Bytes, Infallible>>, E>) -> Response<Body>
where
    E: std::fmt::Display,
{
    match resp {
        Ok(resp) => {
            let (parts, body) = resp.into_parts();
            let body = Body::new(body.map_err(infallible_to_io));
            Response::from_parts(parts, body)
        }
        Err(e) => {
            error!("generated service error: {e}");
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Internal server error"))
                .unwrap_or_else(|_| Response::new(Body::empty()))
        }
    }
}

fn infallible_to_io(err: Infallible) -> std::io::Error {
    match err {}
}
