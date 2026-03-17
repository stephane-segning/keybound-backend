use async_trait::async_trait;
use backend_core::{Error, Result};

/// Trait for nonce replay protection.
///
/// Implementations must ensure that a nonce is unique within the allowed time window
/// for a given device. This prevents replay attacks where an attacker reuses a valid
/// signature.
#[async_trait]
pub trait ReplayGuard: Send + Sync {
    /// Checks and records a nonce for the given device.
    ///
    /// Returns `Ok(())` if the nonce is unique and has been recorded.
    /// Returns `Err` if the nonce has already been used within the time window.
    ///
    /// # Arguments
    /// * `device_id` - The device identifier
    /// * `nonce` - The nonce value from the request
    /// * `timestamp` - The request timestamp (Unix seconds)
    /// * `skew_seconds` - The allowed clock skew window in seconds
    async fn check_and_record(
        &self,
        device_id: &str,
        nonce: &str,
        timestamp: i64,
        skew_seconds: i64,
    ) -> Result<()>;
}

/// Redis-backed replay guard implementation.
///
/// This is the production-ready implementation that provides multi-instance correctness.
pub struct RedisReplayGuard {
    redis_url: String,
}

impl RedisReplayGuard {
    pub fn new(redis_url: String) -> Self {
        Self { redis_url }
    }
}

#[async_trait]
impl ReplayGuard for RedisReplayGuard {
    async fn check_and_record(
        &self,
        device_id: &str,
        nonce: &str,
        _timestamp: i64,
        skew_seconds: i64,
    ) -> Result<()> {
        let client = redis::Client::open(self.redis_url.clone()).map_err(|error| {
            Error::internal(
                "REDIS_CLIENT_FAILED",
                format!("failed to open redis client: {error}"),
            )
        })?;

        let mut conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|error| {
                Error::internal(
                    "REDIS_CONNECT_FAILED",
                    format!("failed to connect to redis: {error}"),
                )
            })?;

        let key = format!("replay:{}:{}", device_id, nonce);
        let ttl = (skew_seconds * 2).max(1); // Set TTL to double the skew to be safe, min 1 sec

        // SET key "" EX ttl NX
        let set_result: Option<String> = redis::cmd("SET")
            .arg(&key)
            .arg("")
            .arg("EX")
            .arg(ttl)
            .arg("NX")
            .query_async(&mut conn)
            .await
            .map_err(|error| {
                Error::internal(
                    "REDIS_CMD_FAILED",
                    format!("redis SET command failed: {error}"),
                )
            })?;

        match set_result {
            Some(_) => Ok(()), // Key was set successfully (was not present)
            None => Err(Error::unauthorized("Nonce already used")), // Key was already present
        }
    }
}

/// In-memory replay guard for testing and development.
///
/// This implementation uses a simple in-memory map and is NOT suitable for
/// production deployments with multiple server instances.
#[cfg(any(test, feature = "test-utils"))]
pub mod in_memory {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    /// In-memory replay guard implementation.
    pub struct InMemoryReplayGuard {
        cache: Mutex<HashMap<String, i64>>,
    }

    impl InMemoryReplayGuard {
        pub fn new() -> Self {
            Self {
                cache: Mutex::new(HashMap::new()),
            }
        }
    }

    impl Default for InMemoryReplayGuard {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait]
    impl ReplayGuard for InMemoryReplayGuard {
        async fn check_and_record(
            &self,
            device_id: &str,
            nonce: &str,
            timestamp: i64,
            skew_seconds: i64,
        ) -> Result<()> {
            let now = chrono::Utc::now().timestamp();
            let cutoff = now - skew_seconds.max(1);
            let nonce_key = format!("{device_id}:{nonce}");

            let mut entries = self.cache.lock().map_err(|_| {
                Error::internal("NONCE_CACHE_LOCK_FAILED", "failed to lock nonce cache")
            })?;

            entries.retain(|_, seen_at| *seen_at >= cutoff);

            if entries.contains_key(&nonce_key) {
                return Err(Error::unauthorized("Nonce already used"));
            }

            entries.insert(nonce_key, timestamp);
            Ok(())
        }
    }

    /// Thread-safe wrapper for the in-memory replay guard.
    #[allow(dead_code)]
    pub type SharedInMemoryReplayGuard = Arc<InMemoryReplayGuard>;
}

#[cfg(test)]
mod tests {
    use super::ReplayGuard;
    use super::in_memory::InMemoryReplayGuard;

    #[tokio::test]
    async fn test_in_memory_replay_guard_allows_unique_nonce() {
        let guard = InMemoryReplayGuard::new();
        let timestamp = chrono::Utc::now().timestamp();

        let result = guard
            .check_and_record("dev_123", "nonce_abc", timestamp, 300)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_in_memory_replay_guard_rejects_reused_nonce() {
        let guard = InMemoryReplayGuard::new();
        let timestamp = chrono::Utc::now().timestamp();

        let result1 = guard
            .check_and_record("dev_123", "nonce_abc", timestamp, 300)
            .await;
        assert!(result1.is_ok());

        let result2 = guard
            .check_and_record("dev_123", "nonce_abc", timestamp, 300)
            .await;
        assert!(result2.is_err());
        assert_eq!(result2.unwrap_err().to_string(), "Nonce already used");
    }
}
