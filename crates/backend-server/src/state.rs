use backend_core::Config;
use backend_repository::PgRepository;
use backend_auth::{KcContext, ServiceContext};
use axum::body::Body;
use bytes::Bytes;
use gen_oas_server_bff::models::{KycStatusResponse, LimitsResponse};
use http::{Request, Response, StatusCode};
use http_body_util::BodyExt as _;
use http_body_util::combinators::BoxBody;
use hyper::service::Service as HyperService;
use lru::LruCache;
use sqlx::postgres::PgPoolOptions;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::convert::Infallible;
use tracing::error;

use crate::services::BackendService;

#[derive(Clone)]
pub struct HttpCache {
    pub kyc_status: Arc<Mutex<LruCache<String, KycStatusResponse>>>,
    pub limits: Arc<Mutex<LruCache<String, LimitsResponse>>>,
}

#[derive(Clone)]
pub struct AppState {
    pub repository: PgRepository,
    pub service: BackendService,
    pub s3: aws_sdk_s3::Client,
    pub sns: aws_sdk_sns::Client,
    pub config: Config,
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

        let capacity = NonZeroUsize::new(10_000).expect("non-zero LRU capacity");
        let http_cache = HttpCache {
            kyc_status: Arc::new(Mutex::new(LruCache::new(capacity))),
            limits: Arc::new(Mutex::new(LruCache::new(capacity))),
        };

        let repository = PgRepository::new(db.clone());
        let service = BackendService::new(repository.clone());

        Ok(Self {
            repository,
            service,
            s3,
            sns,
            config: cfg.clone(),
            http_cache,
        })
    }

    pub fn get_kyc_status_cache(&self, external_id: &str) -> Option<KycStatusResponse> {
        let mut cache = self
            .http_cache
            .kyc_status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.get(external_id).cloned()
    }

    pub fn put_kyc_status_cache(&self, external_id: String, value: KycStatusResponse) {
        let mut cache = self
            .http_cache
            .kyc_status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.put(external_id, value);
    }

    pub fn get_limits_cache(&self, external_id: &str) -> Option<LimitsResponse> {
        let mut cache = self
            .http_cache
            .limits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.get(external_id).cloned()
    }

    pub fn put_limits_cache(&self, external_id: String, value: LimitsResponse) {
        let mut cache = self
            .http_cache
            .limits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.put(external_id, value);
    }

    pub fn invalidate_bff_cache(&self, external_id: &str) {
        let mut kyc_status = self
            .http_cache
            .kyc_status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        kyc_status.pop(external_id);

        let mut limits = self
            .http_cache
            .limits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        limits.pop(external_id);
    }
}

pub async fn call_kc(api: crate::api::BackendApi, req: Request<Body>) -> Response<Body> {
    let ctx = KcContext::from_request(&req);
    let svc = gen_oas_server_kc::server::Service::new(api, false);
    to_axum_response(HyperService::call(&svc, (req, ctx)).await)
}

pub async fn call_bff(api: crate::api::BackendApi, req: Request<Body>) -> Response<Body> {
    let ctx = ServiceContext::from_request(&req);
    let svc = gen_oas_server_bff::server::Service::new(api, false);
    to_axum_response(HyperService::call(&svc, (req, ctx)).await)
}

pub async fn call_staff(api: crate::api::BackendApi, req: Request<Body>) -> Response<Body> {
    let ctx = ServiceContext::from_request(&req);
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
