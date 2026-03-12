//! Background worker for async tasks and state machine processing.
//!
//! This module implements a distributed worker system using Redis for coordination.
//! It processes two types of jobs:
//! - State machine steps (KYC flows, etc.)
//! - Notifications (SMS OTP, email magic links)
//!
//! The worker uses a distributed lock to ensure only one instance runs at a time.

use crate::sms_provider::{ApiSmsProvider, ConsoleSmsProvider, SmsProvider, SnsSmsProvider};
use crate::state::AppState;
use crate::state_machine::engine::Engine;
use crate::state_machine::jobs::StateMachineStepJob;
use crate::state_machine::queue::queue_namespace as sm_queue_namespace;
use apalis::prelude::{BoxDynError, TaskSink, WorkerBuilder};
use apalis_redis::{RedisConfig, RedisStorage};
use async_trait::async_trait;
use backend_core::SmsProviderType;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::time::{Duration, interval};
use tracing::{info, warn};

/// Redis namespace for notification queue
const NOTIFICATION_QUEUE_NAMESPACE: &str = "backend:notifications";
/// Redis key for worker distributed lock
const WORKER_CONSUMER_LOCK_KEY: &str = "backend:worker:consumer-lock";
/// TTL for worker lock in seconds
const WORKER_CONSUMER_LOCK_TTL_SECONDS: i64 = 30;
/// Interval for renewing worker lock in seconds
const WORKER_CONSUMER_LOCK_RENEW_SECONDS: u64 = 10;

/// Verifies Redis connectivity before starting the worker.
///
/// # Arguments
/// * `redis_url` - Redis connection URL
///
/// # Returns
/// `Result<()>` indicating Redis is ready or error
///
/// # Errors
/// Returns error if Redis is unreachable or not responding
pub async fn ensure_redis_ready(redis_url: &str) -> backend_core::Result<()> {
    let client = redis::Client::open(redis_url)
        .map_err(|error| backend_core::Error::Server(format!("invalid redis url: {error}")))?;

    let mut connection = client
        .get_multiplexed_async_connection()
        .await
        .map_err(|error| {
            backend_core::Error::Server(format!("failed to connect to redis at startup: {error}"))
        })?;

    let response: String = connection.ping().await.map_err(|error| {
        backend_core::Error::Server(format!("failed to ping redis at startup: {error}"))
    })?;
    if response != "PONG" {
        return Err(backend_core::Error::Server(format!(
            "unexpected redis PING response at startup: {response}"
        )));
    }

    Ok(())
}

/// Distributed lock for ensuring only one worker runs at a time.
///
/// Uses Redis SET NX EX for acquiring the lock and maintains it with periodic renewal.
/// The lock is automatically released when dropped or when the worker stops.
pub struct WorkerConsumerLock {
    redis_url: String,
    key: String,
    owner: String,
    stop_renew: Option<oneshot::Sender<()>>,
    renew_handle: tokio::task::JoinHandle<()>,
}

impl WorkerConsumerLock {
    /// Releases the worker lock and stops the renewal task.
    ///
    /// This should be called when the worker is shutting down to ensure
    /// the lock is properly released for other instances.
    ///
    /// # Returns
    /// `Result<()>` indicating successful release or error
    pub async fn release(mut self) -> backend_core::Result<()> {
        if let Some(stop) = self.stop_renew.take() {
            let _ = stop.send(());
        }
        let _ = self.renew_handle.await;

        let client = redis::Client::open(self.redis_url.clone())
            .map_err(|error| backend_core::Error::Server(error.to_string()))?;
        let mut connection = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|error| backend_core::Error::Server(error.to_string()))?;

        let script = redis::Script::new(
            r#"
            if redis.call('GET', KEYS[1]) == ARGV[1] then
                return redis.call('DEL', KEYS[1])
            end
            return 0
            "#,
        );
        let _: i32 = script
            .key(self.key)
            .arg(self.owner)
            .invoke_async(&mut connection)
            .await
            .map_err(|error| backend_core::Error::Server(error.to_string()))?;
        Ok(())
    }
}

/// Acquires a distributed lock for exclusive worker execution.
///
/// Uses Redis SET NX EX to atomically acquire a lock. If successful, starts a
/// background task to periodically renew the lock. Only one worker can hold
/// the lock at a time, preventing multiple workers from processing the same jobs.
///
/// # Arguments
/// * `redis_url` - Redis connection URL
///
/// # Returns
/// `Result<WorkerConsumerLock>` containing the lock handle or error
///
/// # Errors
/// Returns error if Redis is unavailable or lock is already held by another instance
pub async fn acquire_worker_consumer_lock(
    redis_url: &str,
) -> backend_core::Result<WorkerConsumerLock> {
    let client = redis::Client::open(redis_url)
        .map_err(|error| backend_core::Error::Server(error.to_string()))?;
    let mut connection = client
        .get_multiplexed_async_connection()
        .await
        .map_err(|error| backend_core::Error::Server(error.to_string()))?;

    let owner = format!(
        "pid:{}:ts:{}",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    );

    let acquired: Option<String> = redis::cmd("SET")
        .arg(WORKER_CONSUMER_LOCK_KEY)
        .arg(&owner)
        .arg("NX")
        .arg("EX")
        .arg(WORKER_CONSUMER_LOCK_TTL_SECONDS)
        .query_async(&mut connection)
        .await
        .map_err(|error| backend_core::Error::Server(error.to_string()))?;

    if acquired.is_none() {
        return Err(backend_core::Error::Server(
            "worker consumer lock already held by another instance".to_owned(),
        ));
    }

    let redis_url_owned = redis_url.to_owned();
    let owner_for_renew = owner.clone();
    let key_for_renew = WORKER_CONSUMER_LOCK_KEY.to_owned();
    let (stop_tx, mut stop_rx) = oneshot::channel();
    let renew_handle = tokio::spawn(async move {
        let client = match redis::Client::open(redis_url_owned.clone()) {
            Ok(client) => client,
            Err(error) => {
                warn!("worker lock renew client init failed: {}", error);
                return;
            }
        };
        let mut connection = match client.get_multiplexed_async_connection().await {
            Ok(connection) => connection,
            Err(error) => {
                warn!("worker lock renew redis connect failed: {}", error);
                return;
            }
        };
        let renew_script = redis::Script::new(
            r#"
            if redis.call('GET', KEYS[1]) == ARGV[1] then
                return redis.call('EXPIRE', KEYS[1], ARGV[2])
            end
            return 0
            "#,
        );

        let mut tick = interval(Duration::from_secs(WORKER_CONSUMER_LOCK_RENEW_SECONDS));
        loop {
            tokio::select! {
                _ = &mut stop_rx => break,
                _ = tick.tick() => {
                    let renewed: Result<i32, redis::RedisError> = renew_script
                        .key(&key_for_renew)
                        .arg(&owner_for_renew)
                        .arg(WORKER_CONSUMER_LOCK_TTL_SECONDS)
                        .invoke_async(&mut connection)
                        .await;
                    match renewed {
                        Ok(1) => {}
                        Ok(_) => {
                            warn!("worker consumer lock renew lost ownership");
                            break;
                        }
                        Err(error) => {
                            warn!("worker consumer lock renew failed: {}", error);
                            break;
                        }
                    }
                }
            }
        }
    });

    Ok(WorkerConsumerLock {
        redis_url: redis_url.to_owned(),
        key: WORKER_CONSUMER_LOCK_KEY.to_owned(),
        owner,
        stop_renew: Some(stop_tx),
        renew_handle,
    })
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
        let conn = apalis_redis::connect(self.redis_url.clone())
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
}

pub async fn run(state: Arc<AppState>) -> backend_core::Result<()> {
    let redis_url = state.config.redis.url.clone();

    let conn = apalis_redis::connect(redis_url.clone())
        .await
        .map_err(|error| backend_core::Error::Server(error.to_string()))?;
    let sm_storage = RedisStorage::new_with_config(conn, RedisConfig::new(sm_queue_namespace()));

    let conn = apalis_redis::connect(redis_url.clone())
        .await
        .map_err(|error| backend_core::Error::Server(error.to_string()))?;
    let notification_storage =
        RedisStorage::new_with_config(conn, RedisConfig::new(NOTIFICATION_QUEUE_NAMESPACE));

    let sms_provider = build_sms_provider(&state.config).await?;

    let sm_state = state.clone();
    let sm_sms = sms_provider.clone();
    let sm_worker = WorkerBuilder::new("state-machine-worker")
        .backend(sm_storage)
        .build(move |job: StateMachineStepJob| {
            let state = sm_state.clone();
            let sms = sm_sms.clone();
            async move {
                let engine = Engine::new(state);
                engine
                    .process_step_job(job, sms)
                    .await
                    .map_err(|e| Box::new(e) as BoxDynError)
            }
        });

    let notification_worker = WorkerBuilder::new("notification-worker")
        .backend(notification_storage)
        .build(move |job: NotificationJob| {
            let provider = sms_provider.clone();
            async move { process_notification_job(provider, job).await }
        });

    info!("starting workers");

    tokio::select! {
        run_result = sm_worker.run() => {
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

async fn build_sms_provider(
    cfg: &backend_core::Config,
) -> backend_core::Result<Arc<dyn SmsProvider>> {
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
            SmsProviderType::Api => {
                let Some(api_cfg) = sms_cfg.api.clone() else {
                    return Err(backend_core::Error::Server(
                        "sms.provider=api requires sms.api config".to_owned(),
                    ));
                };
                let client = reqwest::Client::builder()
                    .build()
                    .map_err(|e| backend_core::Error::Server(e.to_string()))?;
                Arc::new(ApiSmsProvider::new(
                    client,
                    api_cfg.base_url,
                    api_cfg.auth_token,
                ))
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
            tracing::info!(step_id = %step_id, msisdn = %msisdn, "otp notification delivered");
        }
        NotificationJob::MagicEmail {
            step_id,
            email,
            token,
        } => {
            tracing::info!(
                step_id = %step_id,
                email = %email,
                token = %token,
                "magic email notification produced"
            );
        }
    }

    Ok(())
}
