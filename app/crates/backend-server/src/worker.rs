use crate::{sms_retry, state::AppState};
use apalis::prelude::{BoxDynError, TaskSink, WorkerBuilder};
use apalis_redis::{RedisConfig, RedisStorage};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

const SMS_RETRY_QUEUE_NAMESPACE: &str = "backend:sms_retry";
const SMS_RETRY_LOCK_KEY: &str = "backend:sms_retry:lock";
const SMS_RETRY_LOCK_TTL_SECONDS: u64 = 5;
const SMS_RETRY_BATCH_SIZE: i64 = 25;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmsRetrySweepJob {
    pub trigger: String,
}

pub async fn enqueue_sms_retry_sweep(redis_url: &str, trigger: &str) -> backend_core::Result<()> {
    let conn = apalis_redis::connect(redis_url)
        .await
        .map_err(|error| backend_core::Error::Server(error.to_string()))?;
    let mut storage = RedisStorage::new_with_config(conn, RedisConfig::new(SMS_RETRY_QUEUE_NAMESPACE));
    storage
        .push(SmsRetrySweepJob {
            trigger: trigger.to_owned(),
        })
        .await
        .map_err(|error| backend_core::Error::Server(error.to_string()))?;
    Ok(())
}

pub async fn run(state: Arc<AppState>) -> backend_core::Result<()> {
    let redis_url = state.config.redis.url.clone();
    let scheduler_redis_url = redis_url.clone();

    let scheduler = tokio::spawn(async move {
        loop {
            if let Err(error) = enqueue_sms_retry_sweep(&scheduler_redis_url, "interval").await {
                warn!("failed to enqueue periodic sms retry sweep: {error}");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    let conn = apalis_redis::connect(redis_url.clone())
        .await
        .map_err(|error| backend_core::Error::Server(error.to_string()))?;
    let storage = RedisStorage::new_with_config(conn, RedisConfig::new(SMS_RETRY_QUEUE_NAMESPACE));

    let worker_state = state.clone();
    let worker_redis_url = redis_url.clone();

    let worker = WorkerBuilder::new("sms-retry-worker")
        .backend(storage)
        .build(move |job: SmsRetrySweepJob| {
            let state = worker_state.clone();
            let redis_url = worker_redis_url.clone();
            async move { process_sms_retry_sweep_job(state, &redis_url, job).await }
        });

    info!("starting sms retry worker");

    tokio::select! {
        run_result = worker.run() => {
            scheduler.abort();
            run_result.map_err(|error| backend_core::Error::Server(error.to_string()))?;
        }
        _ = tokio::signal::ctrl_c() => {
            scheduler.abort();
            info!("ctrl+c received, stopping sms retry worker loop");
        }
    }

    Ok(())
}

async fn process_sms_retry_sweep_job(
    state: Arc<AppState>,
    redis_url: &str,
    _job: SmsRetrySweepJob,
) -> Result<(), BoxDynError> {
    if !try_acquire_sms_retry_lock(redis_url).await? {
        return Ok(());
    }

    sms_retry::process_retryable_sms_batch(state.as_ref(), SMS_RETRY_BATCH_SIZE).await?;
    Ok(())
}

async fn try_acquire_sms_retry_lock(redis_url: &str) -> Result<bool, BoxDynError> {
    use redis::AsyncCommands;

    let client = redis::Client::open(redis_url)?;
    let mut conn = client.get_multiplexed_async_connection().await?;
    let lock_token = backend_id::sms_hash()?;

    let set_result: Option<String> = conn
        .set_options(
            SMS_RETRY_LOCK_KEY,
            lock_token,
            redis::SetOptions::default()
                .conditional_set(redis::ExistenceCheck::NX)
                .with_expiration(redis::SetExpiry::EX(SMS_RETRY_LOCK_TTL_SECONDS as u64)),
        )
        .await?;

    Ok(set_result.is_some())
}
