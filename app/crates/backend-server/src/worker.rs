use crate::state::AppState;
use apalis::prelude::{BoxDynError, TaskSink, WorkerBuilder};
use apalis_redis::{RedisConfig, RedisStorage};
use backend_repository::{KycRepo, UserRepo};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tracing::{info, warn};

const FINERACT_PROVISIONING_QUEUE_NAMESPACE: &str = "backend:fineract_provisioning";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FineractProvisioningJob {
    pub user_id: String,
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

pub async fn run(state: Arc<AppState>) -> backend_core::Result<()> {
    let redis_url = state.config.redis.url.clone();
    let conn = apalis_redis::connect(redis_url)
        .await
        .map_err(|error| backend_core::Error::Server(error.to_string()))?;
    let fineract_storage = RedisStorage::new_with_config(
        conn,
        RedisConfig::new(FINERACT_PROVISIONING_QUEUE_NAMESPACE),
    );

    let worker_state = state.clone();
    let fineract_worker = WorkerBuilder::new("fineract-provisioning-worker")
        .backend(fineract_storage)
        .build(move |job: FineractProvisioningJob| {
            let state = worker_state.clone();
            async move { process_fineract_provisioning_job(state, job).await }
        });

    info!("starting workers");

    tokio::select! {
        run_result = fineract_worker.run() => {
            run_result.map_err(|error| backend_core::Error::Server(error.to_string()))?;
        }
        _ = tokio::signal::ctrl_c() => {
            info!("ctrl+c received, stopping workers");
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

    let client = reqwest::Client::new();
    let url = format!("{}/api/registration/register", state.config.cuss.api_url);

    let response = client
        .post(&url)
        .header("User-Agent", "user-storage/1.0.0")
        .json(&req)
        .send()
        .await
        .map_err(|e| Box::new(e) as BoxDynError)?;

    if !response.status().is_success() {
        let status = response.status();
        let error_body = response.text().await.unwrap_or_default();
        warn!(
            user_id = %job.user_id,
            status = %status,
            body = %error_body,
            "failed to provision user in fineract"
        );
        return Err(
            format!("Fineract provisioning failed with status {status}: {error_body}").into(),
        );
    }

    let resp: RegistrationResponse = response
        .json()
        .await
        .map_err(|e| Box::new(e) as BoxDynError)?;

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
