use crate::state::AppState;
use crate::sms_provider::{ConsoleSmsProvider, SmsProvider, SnsSmsProvider};
use apalis::prelude::{BoxDynError, TaskSink, WorkerBuilder};
use async_trait::async_trait;
use apalis_redis::{RedisConfig, RedisStorage};
use backend_core::SmsProviderType;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tracing::{info, warn};

const FINERACT_PROVISIONING_QUEUE_NAMESPACE: &str = "backend:fineract_provisioning";
const NOTIFICATION_QUEUE_NAMESPACE: &str = "backend:notifications";

#[async_trait]
pub trait WorkerHttpClient: Send + Sync + std::fmt::Debug {
    async fn post_json(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<(http::StatusCode, String), BoxDynError>;
}

#[async_trait]
impl WorkerHttpClient for reqwest::Client {
    async fn post_json(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<(http::StatusCode, String), BoxDynError> {
        let response = self
            .post(url)
            .header("User-Agent", "user-storage/1.0.0")
            .json(body)
            .send()
            .await
            .map_err(|e| Box::new(e) as BoxDynError)?;

        let status = response.status();
        let text = response.text().await.map_err(|e| Box::new(e) as BoxDynError)?;

        Ok((status, text))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FineractProvisioningJob {
    pub user_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NotificationJob {
    Otp {
        step_id: String,
        msisdn: String,
        otp: String,
    },
    MagicEmail {
        step_id: String,
        email: String,
        token: String,
    },
}

#[async_trait]
pub trait ProvisioningQueue: Send + Sync {
    async fn enqueue_fineract_provisioning(&self, user_id: &str) -> backend_core::Result<()>;
}

pub struct RedisProvisioningQueue {
    redis_url: String,
}

impl RedisProvisioningQueue {
    pub fn new(redis_url: String) -> Self {
        Self { redis_url }
    }
}

#[async_trait]
impl ProvisioningQueue for RedisProvisioningQueue {
    async fn enqueue_fineract_provisioning(&self, user_id: &str) -> backend_core::Result<()> {
        enqueue_fineract_provisioning(&self.redis_url, user_id).await
    }
}

#[async_trait]
pub trait NotificationQueue: Send + Sync {
    async fn enqueue(&self, job: NotificationJob) -> backend_core::Result<()>;
}

pub struct RedisNotificationQueue {
    redis_url: String,
}

impl RedisNotificationQueue {
    pub fn new(redis_url: String) -> Self {
        Self { redis_url }
    }
}

#[async_trait]
impl NotificationQueue for RedisNotificationQueue {
    async fn enqueue(&self, job: NotificationJob) -> backend_core::Result<()> {
        enqueue_notification(&self.redis_url, job).await
    }
}

pub async fn enqueue_fineract_provisioning(
    redis_url: &str,
    user_id: &str,
) -> backend_core::Result<()> {
    let conn = apalis_redis::connect(redis_url)
        .await
        .map_err(|error| backend_core::Error::Server(error.to_string()))?;
    let mut storage = RedisStorage::new_with_config(
        conn,
        RedisConfig::new(FINERACT_PROVISIONING_QUEUE_NAMESPACE),
    );
    storage
        .push(FineractProvisioningJob {
            user_id: user_id.to_owned(),
        })
        .await
        .map_err(|error| backend_core::Error::Server(error.to_string()))?;
    Ok(())
}

pub async fn enqueue_notification(
    redis_url: &str,
    job: NotificationJob,
) -> backend_core::Result<()> {
    let conn = apalis_redis::connect(redis_url)
        .await
        .map_err(|error| backend_core::Error::Server(error.to_string()))?;
    let mut storage =
        RedisStorage::new_with_config(conn, RedisConfig::new(NOTIFICATION_QUEUE_NAMESPACE));
    storage
        .push(job)
        .await
        .map_err(|error| backend_core::Error::Server(error.to_string()))?;
    Ok(())
}

pub async fn run(state: Arc<AppState>) -> backend_core::Result<()> {
    let redis_url = state.config.redis.url.clone();
    let conn = apalis_redis::connect(redis_url)
        .await
        .map_err(|error| backend_core::Error::Server(error.to_string()))?;
    let fineract_storage = RedisStorage::new_with_config(
        conn,
        RedisConfig::new(FINERACT_PROVISIONING_QUEUE_NAMESPACE),
    );
    let conn = apalis_redis::connect(state.config.redis.url.clone())
        .await
        .map_err(|error| backend_core::Error::Server(error.to_string()))?;
    let notification_storage =
        RedisStorage::new_with_config(conn, RedisConfig::new(NOTIFICATION_QUEUE_NAMESPACE));

    let sms_provider = build_sms_provider(&state.config).await?;

    let worker_state = state.clone();
    let fineract_worker = WorkerBuilder::new("fineract-provisioning-worker")
        .backend(fineract_storage)
        .build(move |job: FineractProvisioningJob| {
            let state = worker_state.clone();
            async move { process_fineract_provisioning_job(state, job).await }
        });

    let notification_worker = WorkerBuilder::new("notification-worker")
        .backend(notification_storage)
        .build(move |job: NotificationJob| {
            let provider = sms_provider.clone();
            async move { process_notification_job(provider, job).await }
        });

    info!("starting workers");

    tokio::select! {
        run_result = fineract_worker.run() => {
            run_result.map_err(|error| backend_core::Error::Server(error.to_string()))?;
        }
        run_result = notification_worker.run() => {
            run_result.map_err(|error| backend_core::Error::Server(error.to_string()))?;
        }
        _ = tokio::signal::ctrl_c() => {
            info!("ctrl+c received, stopping workers");
        }
    }

    Ok(())
}

async fn build_sms_provider(cfg: &backend_core::Config) -> backend_core::Result<Arc<dyn SmsProvider>> {
    let provider: Arc<dyn SmsProvider> = if let Some(sms_cfg) = &cfg.sms {
        match sms_cfg.provider {
            SmsProviderType::Console => Arc::new(ConsoleSmsProvider),
            SmsProviderType::Sns => {
                let shared_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
                    .load()
                    .await;

                let mut builder = aws_sdk_sns::config::Builder::from(&shared_config);
                if let Some(sns_cfg) = &cfg.sns
                    && let Some(region) = &sns_cfg.region
                {
                    builder = builder.region(aws_types::region::Region::new(region.clone()));
                }
                let sns = aws_sdk_sns::Client::from_conf(builder.build());
                Arc::new(SnsSmsProvider::new(sns))
            }
        }
    } else {
        Arc::new(ConsoleSmsProvider)
    };

    Ok(provider)
}

async fn process_notification_job(
    sms_provider: Arc<dyn SmsProvider>,
    job: NotificationJob,
) -> Result<(), BoxDynError> {
    match job {
        NotificationJob::Otp {
            step_id,
            msisdn,
            otp,
        } => {
            sms_provider.send_otp(&msisdn, &otp).await?;
            info!(step_id = %step_id, msisdn = %msisdn, "otp notification delivered");
        }
        NotificationJob::MagicEmail {
            step_id,
            email,
            token,
        } => {
            info!(
                step_id = %step_id,
                email = %email,
                token = %token,
                "magic email notification produced"
            );
        }
    }

    Ok(())
}

async fn process_fineract_provisioning_job(
    state: Arc<AppState>,
    job: FineractProvisioningJob,
) -> Result<(), BoxDynError> {
    use gen_oas_server_cuss::models::{RegistrationRequest, RegistrationResponse};

    info!(user_id = %job.user_id, "processing fineract provisioning job");

    let user = state.user.get_user(&job.user_id).await?;
    let Some(user) = user else {
        warn!(user_id = %job.user_id, "user not found for provisioning");
        return Ok(());
    };

    let identity_step = find_identity_step_data(state.as_ref(), &job.user_id).await?;
    let identity_data = identity_step.unwrap_or(Value::Null);

    let first_name = user
        .first_name
        .clone()
        .or_else(|| value_as_string(&identity_data, "first_name"))
        .or_else(|| value_as_string(&identity_data, "firstName"))
        .unwrap_or_default();
    let last_name = user
        .last_name
        .clone()
        .or_else(|| value_as_string(&identity_data, "last_name"))
        .or_else(|| value_as_string(&identity_data, "lastName"))
        .unwrap_or_default();
    let email = user
        .email
        .clone()
        .or_else(|| value_as_string(&identity_data, "email"))
        .unwrap_or_default();
    let phone = user
        .phone_number
        .clone()
        .or_else(|| value_as_string(&identity_data, "phone_number"))
        .or_else(|| value_as_string(&identity_data, "phoneNumber"))
        .unwrap_or_default();

    let date_of_birth = value_as_string(&identity_data, "date_of_birth")
        .or_else(|| value_as_string(&identity_data, "dateOfBirth"))
        .and_then(|d| d.parse::<chrono::NaiveDate>().ok());

    let req = RegistrationRequest {
        first_name,
        last_name,
        email,
        phone,
        national_id: value_as_string(&identity_data, "national_id")
            .or_else(|| value_as_string(&identity_data, "nationalId")),
        date_of_birth,
        gender: value_as_string(&identity_data, "gender"),
        address: None,
    };

    let url = format!("{}/api/registration/register", state.config.cuss.api_url);
    let body = serde_json::to_value(&req).map_err(|e| Box::new(e) as BoxDynError)?;

    let (status, text) = state
        .worker_http_client
        .post_json(&url, &body)
        .await?;

    if !status.is_success() {
        warn!(
            user_id = %job.user_id,
            status = %status,
            body = %text,
            "failed to provision user in fineract"
        );
        return Err(format!("Fineract provisioning failed with status {status}: {text}").into());
    }

    let resp: RegistrationResponse =
        serde_json::from_str(&text).map_err(|e| Box::new(e) as BoxDynError)?;
    info!(
        user_id = %job.user_id,
        external_id = ?resp.external_id,
        status = ?resp.status,
        "successfully provisioned user in fineract"
    );

    Ok(())
}

async fn find_identity_step_data(
    state: &AppState,
    user_id: &str,
) -> backend_core::Result<Option<Value>> {
    let (_session, step_ids) = state.kyc.start_or_resume_session(user_id).await?;
    let mut latest = None;

    for step_id in step_ids {
        if let Some(step) = state.kyc.get_step(&step_id).await? {
            if step.step_type == "IDENTITY" {
                latest = Some(step.data);
            }
        }
    }

    Ok(latest)
}

fn value_as_string(source: &Value, key: &str) -> Option<String> {
    source
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}
