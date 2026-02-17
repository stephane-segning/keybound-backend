use crate::sms_provider::{ConsoleSmsProvider, SmsProvider, SnsSmsProvider};
use backend_core::{Config, SmsProviderType};
use backend_repository::{
    ApprovalRepository, DeviceRepository, KycRepository, SmsRepository, UserRepository,
};
use diesel_async::pooled_connection::deadpool::Pool;
use diesel_async::AsyncPgConnection;
use std::sync::Arc;
use tracing::info;

#[derive(Clone)]
pub struct AppState {
    pub kyc: KycRepository,
    pub user: UserRepository,
    pub device: DeviceRepository,
    pub approval: ApprovalRepository,
    pub sms: SmsRepository,
    pub s3: aws_sdk_s3::Client,
    pub sns: aws_sdk_sns::Client,
    pub sms_provider: Arc<dyn SmsProvider>,
    pub config: Config,
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
            .field("sms_provider", &"<SmsProvider>")
            .field("config", &self.config)
            .field("http_cache", &"<HttpCache>")
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

        let sms_provider: Arc<dyn SmsProvider> = if let Some(sms_cfg) = &cfg.sms {
            match sms_cfg.provider {
                SmsProviderType::Console => Arc::new(ConsoleSmsProvider) as Arc<dyn SmsProvider>,
                SmsProviderType::Sns => Arc::new(SnsSmsProvider::new(sns.clone())) as Arc<dyn SmsProvider>,
            }
        } else {
            Arc::new(ConsoleSmsProvider) as Arc<dyn SmsProvider>
        };

        let kyc = KycRepository::new(pool.clone());
        let user = UserRepository::new(pool.clone());
        let device = DeviceRepository::new(pool.clone());
        let approval = ApprovalRepository::new(pool.clone());
        let sms = SmsRepository::new(pool.clone());

        Ok(Self {
            kyc,
            user,
            device,
            approval,
            sms,
            s3,
            sns,
            sms_provider,
            config: cfg.clone(),
        })
    }
}
