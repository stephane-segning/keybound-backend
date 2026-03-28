//! Background worker for async tasks and flow step processing.
//!
//! This module implements a distributed worker system using Redis for coordination.
//! It processes two types of jobs:
//! - Flow steps (KYC flows, etc.)
//! - Notifications (SMS OTP, email magic links)
//!
//! The worker uses a distributed lock to ensure only one instance runs at a time.

use crate::flows::executor::FlowExecutor;
use crate::state::AppState;
use apalis::prelude::TaskSink;
use apalis_redis::{RedisConfig, RedisStorage};
use async_trait::async_trait;
use backend_core::NotificationJob;
use redis::AsyncCommands;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::time::{Duration, interval};
use tracing::{debug, info, instrument, warn};

/// Redis namespace for notification queue
const NOTIFICATION_QUEUE_NAMESPACE: &str = "backend:notifications";
/// Redis key for worker distributed lock
const WORKER_CONSUMER_LOCK_KEY: &str = "backend:worker:consumer-lock";

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
/// * `lock_ttl_seconds` - Lock TTL in seconds
/// * `lock_renew_seconds` - Lock renewal interval in seconds
///
/// # Returns
/// `Result<WorkerConsumerLock>` containing the lock handle or error
///
/// # Errors
/// Returns error if Redis is unavailable or lock is already held by another instance
pub async fn acquire_worker_consumer_lock(
    redis_url: &str,
    lock_ttl_seconds: i64,
    lock_renew_seconds: u64,
) -> backend_core::Result<WorkerConsumerLock> {
    let lock_ttl_seconds = lock_ttl_seconds.max(1);
    let lock_renew_seconds = if lock_renew_seconds == 0 {
        1
    } else {
        lock_renew_seconds.min((lock_ttl_seconds as u64).saturating_sub(1).max(1))
    };

    if lock_renew_seconds >= lock_ttl_seconds as u64 {
        warn!(
            lock_ttl_seconds,
            lock_renew_seconds,
            "worker lock renew interval should be smaller than ttl; using normalized values",
        );
    }

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
        .arg(lock_ttl_seconds)
        .query_async(&mut connection)
        .await
        .map_err(|error| backend_core::Error::Server(error.to_string()))?;

    if acquired.is_none() {
        let current_owner: Option<String> = redis::cmd("GET")
            .arg(WORKER_CONSUMER_LOCK_KEY)
            .query_async(&mut connection)
            .await
            .ok();
        let current_ttl_seconds: Option<i64> = redis::cmd("TTL")
            .arg(WORKER_CONSUMER_LOCK_KEY)
            .query_async(&mut connection)
            .await
            .ok();
        return Err(backend_core::Error::Server(format!(
            "worker consumer lock already held by another instance (key: {}, owner: {}, ttl_seconds: {})",
            WORKER_CONSUMER_LOCK_KEY,
            current_owner.unwrap_or_else(|| "unknown".to_owned()),
            current_ttl_seconds
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_owned())
        )));
    }

    info!(
        lock_key = WORKER_CONSUMER_LOCK_KEY,
        lock_ttl_seconds, lock_renew_seconds, owner, "worker consumer lock acquired"
    );

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

        let mut tick = interval(Duration::from_secs(lock_renew_seconds));
        loop {
            tokio::select! {
                _ = &mut stop_rx => break,
                _ = tick.tick() => {
                    let renewed: Result<i32, redis::RedisError> = renew_script
                        .key(&key_for_renew)
                        .arg(&owner_for_renew)
                        .arg(lock_ttl_seconds)
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

/// Acquires the worker consumer lock with retry and exponential backoff.
///
/// During rolling deployments, the old pod may still hold the lock. Instead of
/// crashing immediately, this retries up to 10 times with exponential backoff
/// (2s, 4s, 8s, ..., capped at 30s) to allow the old pod to terminate and
/// release the lock.
pub async fn acquire_worker_consumer_lock_with_retry(
    redis_url: &str,
    lock_ttl_seconds: i64,
    lock_renew_seconds: u64,
) -> backend_core::Result<WorkerConsumerLock> {
    const MAX_RETRIES: u32 = 10;
    const INITIAL_BACKOFF_SECS: u64 = 2;
    const MAX_BACKOFF_SECS: u64 = 30;

    for attempt in 0..MAX_RETRIES {
        match acquire_worker_consumer_lock(redis_url, lock_ttl_seconds, lock_renew_seconds).await {
            Ok(lock) => return Ok(lock),
            Err(e) if attempt < MAX_RETRIES - 1 => {
                let backoff = INITIAL_BACKOFF_SECS.pow(attempt + 1).min(MAX_BACKOFF_SECS);
                warn!(
                    attempt = attempt + 1,
                    max_retries = MAX_RETRIES,
                    backoff_secs = backoff,
                    "worker lock held by another instance, retrying: {}",
                    e
                );
                tokio::time::sleep(Duration::from_secs(backoff)).await;
            }
            Err(e) => return Err(e),
        }
    }

    unreachable!()
}

#[instrument(skip(state))]
pub async fn run(state: Arc<AppState>) -> backend_core::Result<()> {
    info!("starting flow sdk system worker");

    loop {
        debug!("Polling for next system step...");
        let state_clone = state.clone();

        // Wait for next eligible step
        let maybe_step = match state_clone.flow.claim_next_system_step().await {
            Ok(step) => step,
            Err(e) => {
                warn!("failed to claim next system step: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                continue;
            }
        };

        match maybe_step {
            Some(step) => {
                let executor = FlowExecutor::new(state_clone);
                if let Err(e) = executor.process_flow_step(step).await {
                    warn!("failed to process system flow step: {}", e);
                }
            }
            None => {
                // No eligible step found, sleep for a short duration
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
        }
    }
}
