use axum::body::Body;
use backend_auth::{KcContext, ServiceContext};
use backend_core::Config;
use backend_repository::{
    ApprovalRepository, DeviceRepository, KycRepository, SmsRepository,
    UserRepository,
};
use gen_oas_server_bff::models::{KycCaseResponse, LimitsResponse};
use http::{Request, Response, StatusCode};
use lru::LruCache;
use sqlx::postgres::PgPoolOptions;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use tower::util::ServiceExt;
use tracing::{debug, error, info};

#[derive(Clone)]
pub struct HttpCache {
    pub kyc_status: Arc<Mutex<LruCache<String, KycCaseResponse>>>,
    pub limits: Arc<Mutex<LruCache<String, LimitsResponse>>>,
}

#[derive(Clone)]
pub struct AppState {
    pub kyc: KycRepository,
    pub user: UserRepository,
    pub device: DeviceRepository,
    pub approval: ApprovalRepository,
    pub sms: SmsRepository,
    pub s3: aws_sdk_s3::Client,
    pub sns: aws_sdk_sns::Client,
    pub config: Config,
    pub http_cache: HttpCache,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("kyc", &"<KycRepository>")
            .field("user", &"<UserRepository>")
            .field("device", &"<DeviceRepository>")
            .field("approval", &"<ApprovalRepository>")
            .field("sms", &"<SmsRepository>")
            .field("s3", &"<S3Client>")
            .field("sns", &"<SnsClient>")
            .field("config", &self.config)
            .field("http_cache", &"<HttpCache>")
            .finish()
    }
}

impl AppState {
    pub async fn from_config(cfg: &Config) -> backend_core::Result<Self> {
        info!("initializing application state and repositories");
        let db = PgPoolOptions::new()
            .max_connections(cfg.database_pool_size())
            .connect(&cfg.database.url)
            .await?;

        let shared_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .load()
            .await;

        let s3 = {
            let mut builder = aws_sdk_s3::config::Builder::from(&shared_config);
            if let Some(s3_cfg) = &cfg.s3 {
                if let Some(region) = &s3_cfg.region {
                    builder = builder.region(aws_types::region::Region::new(region.clone()));
                }
                if let Some(endpoint) = &s3_cfg.endpoint {
                    builder = builder.endpoint_url(endpoint);
                }
                if s3_cfg.force_path_style.unwrap_or(false) {
                    builder = builder.force_path_style(true);
                }
            }
            aws_sdk_s3::Client::from_conf(builder.build())
        };

        let sns = {
            let mut builder = aws_sdk_sns::config::Builder::from(&shared_config);
            if let Some(sns_cfg) = &cfg.sns {
                if let Some(region) = &sns_cfg.region {
                    builder = builder.region(aws_types::region::Region::new(region.clone()));
                }
            }
            aws_sdk_sns::Client::from_conf(builder.build())
        };

        let capacity = NonZeroUsize::new(10_000).expect("non-zero LRU capacity");
        let http_cache = HttpCache {
            kyc_status: Arc::new(Mutex::new(LruCache::new(capacity))),
            limits: Arc::new(Mutex::new(LruCache::new(capacity))),
        };
        let kyc = KycRepository::new(db.clone());
        let user_phone_cache = Arc::new(Mutex::new(LruCache::new(capacity)));
        let user = UserRepository::new(db.clone(), user_phone_cache);
        let device = DeviceRepository::new(db.clone());
        let approval = ApprovalRepository::new(db.clone());
        let sms = SmsRepository::new(db);

        Ok(Self {
            kyc,
            user,
            device,
            approval,
            sms,
            s3,
            sns,
            config: cfg.clone(),
            http_cache,
        })
    }

    pub fn get_kyc_status_cache(&self, external_id: &str) -> Option<KycCaseResponse> {
        let mut cache = self
            .http_cache
            .kyc_status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.get(external_id).cloned()
    }

    pub fn put_kyc_status_cache(&self, external_id: String, value: KycCaseResponse) {
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
    let _ctx = KcContext::from_request(&req);
    debug!(path = %req.uri().path(), "dispatching request to KC generated router");
    let router = gen_oas_server_kc::server::new(api);
    match router.oneshot(req).await {
        Ok(resp) => resp,
        Err(e) => {
            error!(error = %e, "KC router request failed");
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Internal server error"))
                .unwrap_or_else(|_| Response::new(Body::empty()))
        }
    }
}

pub async fn call_bff(api: crate::api::BackendApi, req: Request<Body>) -> Response<Body> {
    let _ctx = ServiceContext::from_request(&req);
    debug!(path = %req.uri().path(), "dispatching request to BFF generated router");
    let router = gen_oas_server_bff::server::new(api);
    match router.oneshot(req).await {
        Ok(resp) => resp,
        Err(e) => {
            error!(error = %e, "BFF router request failed");
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Internal server error"))
                .unwrap_or_else(|_| Response::new(Body::empty()))
        }
    }
}

pub async fn call_staff(api: crate::api::BackendApi, req: Request<Body>) -> Response<Body> {
    let _ctx = ServiceContext::from_request(&req);
    debug!(path = %req.uri().path(), "dispatching request to Staff generated router");
    let router = gen_oas_server_staff::server::new(api);
    match router.oneshot(req).await {
        Ok(resp) => resp,
        Err(e) => {
            error!(error = %e, "Staff router request failed");
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Internal server error"))
                .unwrap_or_else(|_| Response::new(Body::empty()))
        }
    }
}
