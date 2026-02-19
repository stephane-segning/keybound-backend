use crate::file_storage::{FileStorage, S3FileStorage};
use crate::sms_provider::SmsProvider;
use crate::worker::{
    NotificationQueue, ProvisioningQueue, RedisNotificationQueue, RedisProvisioningQueue,
    WorkerHttpClient,
};
use backend_auth::{HttpClient, OidcState, SignatureState};
use backend_core::Config;
use backend_repository::{
    DeviceRepo, DeviceRepository, KycRepo, KycRepository, UserRepo, UserRepository,
};
use diesel_async::pooled_connection::deadpool::Pool;
use diesel_async::AsyncPgConnection;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

#[derive(Clone)]
pub struct AppState {
    pub kyc: Arc<dyn KycRepo>,
    pub user: Arc<dyn UserRepo>,
    pub device: Arc<dyn DeviceRepo>,
    pub sms: Arc<dyn SmsProvider>,
    pub notification_queue: Arc<dyn NotificationQueue>,
    pub provisioning_queue: Arc<dyn ProvisioningQueue>,
    pub s3: Arc<dyn FileStorage>,
    pub config: Config,
    pub oidc_state: Arc<OidcState>,
    pub signature_state: Arc<SignatureState>,
    pub worker_http_client: Arc<dyn WorkerHttpClient>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("kyc", &"<KycRepository>")
            .field("user", &"<UserRepository>")
            .field("device", &"<DeviceRepository>")
            .field("s3", &"<FileStorage>")
            .field("config", &self.config)
            .field("oidc_state", &"<OidcState>")
            .field("signature_state", &"<SignatureState>")
            .field("worker_http_client", &self.worker_http_client)
            .finish()
    }
}

impl AppState {
    pub async fn from_config(
        cfg: &Config,
        pool: Pool<AsyncPgConnection>,
    ) -> backend_core::Result<Self> {
        info!("initializing application state and repositories");

        let shared_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .load()
            .await;

        let s3_client = {
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
        let s3: Arc<dyn FileStorage> = Arc::new(S3FileStorage::new(s3_client));

        let kyc: Arc<dyn KycRepo> = Arc::new(KycRepository::new(pool.clone()));
        let user: Arc<dyn UserRepo> = Arc::new(UserRepository::new(pool.clone()));
        let device: Arc<dyn DeviceRepo> = Arc::new(DeviceRepository::new(pool.clone()));

        let sms: Arc<dyn SmsProvider> = match cfg.sms.as_ref().map(|s| &s.provider) {
            Some(backend_core::SmsProviderType::Sns) => {
                let sns_client = aws_sdk_sns::Client::new(&shared_config);
                Arc::new(crate::sms_provider::SnsSmsProvider::new(sns_client))
            }
            _ => Arc::new(crate::sms_provider::ConsoleSmsProvider),
        };

        let http_client = HttpClient::new_with_defaults()?;

        let oidc_state = Arc::new(OidcState::new(
            cfg.oauth2.issuer.clone(),
            None, // TODO: add audiences to config if needed
            Duration::from_secs(3600),
            Duration::from_secs(3600),
            http_client,
        ));

        let signature_state = Arc::new(SignatureState {
            signature_secret: cfg.kc.signature_secret.clone(),
            max_clock_skew_seconds: cfg.kc.max_clock_skew_seconds,
            max_body_bytes: cfg.kc.max_body_bytes,
        });

        let worker_http_client: Arc<dyn WorkerHttpClient> =
            Arc::new(reqwest::Client::builder().build()?);

        let notification_queue = Arc::new(RedisNotificationQueue::new(cfg.redis.url.clone()));
        let provisioning_queue = Arc::new(RedisProvisioningQueue::new(cfg.redis.url.clone()));

        Ok(Self {
            kyc,
            user,
            device,
            sms,
            notification_queue,
            provisioning_queue,
            s3,
            config: cfg.clone(),
            oidc_state,
            signature_state,
            worker_http_client,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::health_router;
    use axum::body::Body;
    use axum::http::Request;
    use backend_core::Config;
    use diesel_async::pooled_connection::AsyncDieselConnectionManager;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_app_state_from_config_minimal() {
        let config_yaml = r#"
server:
  address: "127.0.0.1"
  port: 8080
  tls:
    cert_path: "cert.pem"
    key_path: "key.pem"
logging:
  level: "info"
database:
  url: "postgres://localhost/test"
oauth2:
  issuer: "http://localhost:8081/realms/test"
kc:
  enabled: true
  base_path: "/kc"
  signature_secret: "test-secret"
  max_clock_skew_seconds: 30
  max_body_bytes: 1048576
bff:
  enabled: true
  base_path: "/bff"
staff:
  enabled: true
  base_path: "/staff"
cuss:
  api_url: "http://localhost:8082"
"#;
        let cfg: Config = serde_yaml::from_str(config_yaml).unwrap();

        // Create a dummy pool. It won't be used for actual DB ops in this test.
        let manager = AsyncDieselConnectionManager::<diesel_async::AsyncPgConnection>::new("postgres://localhost/test");
        let pool = Pool::builder(manager).build().unwrap();

        let state = AppState::from_config(&cfg, pool).await.unwrap();

        assert_eq!(state.config.server.port, 8080);
    }

    #[tokio::test]
    async fn test_health_router_regression() {
        let app = health_router();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), hyper::StatusCode::OK);
    }
}
