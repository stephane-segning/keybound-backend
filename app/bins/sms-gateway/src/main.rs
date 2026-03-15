use apalis::prelude::WorkerBuilder;
use apalis_redis::{RedisConfig, RedisStorage};
use backend_core::config::{self, SmsProviderType};
use backend_core::NotificationJob;
use clap::Parser;
use mimalloc::MiMalloc;
use sms_provider::{process_notification_job, ApiSmsProvider, ConsoleSmsProvider, SnsSmsProvider};
use std::sync::Arc;
use tracing::{info, warn};

mod sms_provider;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

const NOTIFICATION_QUEUE_NAMESPACE: &str = "backend:notifications";

#[derive(Parser, Debug)]
#[command(author, version, about = "SMS Gateway Service", long_about = None)]
struct Args {
    /// Path to configuration file
    #[arg(short, long, default_value = "config/local.yaml")]
    config: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    // Load configuration
    info!("Loading configuration from: {}", args.config);
    let config = config::load_from_path(&args.config)?;

    // Ensure SMS configuration is present
    let sms_config = config.sms.as_ref().ok_or_else(|| {
        anyhow::anyhow!("SMS configuration is required but not found in config file")
    })?;

    // Create SMS provider based on configuration
    let provider = create_sms_provider(sms_config).await?;
    let provider = Arc::new(provider);

    // Connect to Redis
    info!("Connecting to Redis at: {}", config.redis.url);
    let redis_conn = apalis_redis::connect(config.redis.url.clone()).await?;
    let storage =
        RedisStorage::new_with_config(redis_conn, RedisConfig::new(NOTIFICATION_QUEUE_NAMESPACE));

    // Verify Redis connectivity
    verify_redis_connection(&config.redis.url).await?;

    info!("Starting SMS gateway worker");
    info!("Provider: {:?}", sms_config.provider);
    info!("Queue namespace: {}", NOTIFICATION_QUEUE_NAMESPACE);

    // Create and run the worker
    let provider_for_worker = provider.clone();
    let worker = WorkerBuilder::new("sms-gateway-worker")
        .backend(storage)
        .build(move |job: NotificationJob| {
            let provider = provider_for_worker.clone();
            async move {
                if let Err(e) = process_notification_job(provider, job).await {
                    warn!("Failed to process notification job: {}", e);
                    Err(apalis::prelude::BoxDynError::from(e))
                } else {
                    Ok(())
                }
            }
        });

    // Run until Ctrl+C
    tokio::select! {
        result = worker.run() => {
            result?;
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received Ctrl+C, shutting down");
        }
    }

    Ok(())
}

/// Create the appropriate SMS provider based on configuration
async fn create_sms_provider(
    config: &backend_core::config::SmsConfig,
) -> anyhow::Result<Arc<dyn sms_provider::SmsProvider>> {
    match config.provider {
        SmsProviderType::Console => {
            info!("Using Console SMS provider (development mode)");
            Ok(Arc::new(ConsoleSmsProvider))
        }
        SmsProviderType::Sns => {
            info!("Using AWS SNS SMS provider");
            let aws_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
                .load()
                .await;
            Ok(Arc::new(SnsSmsProvider::from_config(&aws_config).await))
        }
        SmsProviderType::Api => {
            let api_config = config.api.as_ref().ok_or_else(|| {
                anyhow::anyhow!("SMS API configuration is required when provider is 'api'")
            })?;
            info!("Using API SMS provider: {}", api_config.base_url);
            let client = reqwest::Client::new();
            Ok(Arc::new(ApiSmsProvider::new(
                client,
                api_config.base_url.clone(),
                api_config.auth_token.clone(),
            )))
        }
    }
}

/// Verify Redis connectivity before starting
async fn verify_redis_connection(redis_url: &str) -> anyhow::Result<()> {
    use redis::AsyncCommands;

    let client = redis::Client::open(redis_url)?;
    let mut conn = client.get_multiplexed_async_connection().await?;
    let _: String = conn.ping().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use apalis::prelude::{TaskSink, WorkerBuilder};
    use apalis_redis::RedisStorage;

    async fn setup_redis(redis_url: &str) -> redis::Client {
        let client = redis::Client::open(redis_url).unwrap();
        let mut conn = client.get_multiplexed_async_connection().await.unwrap();
        let _: () = redis::cmd("FLUSHDB").query_async(&mut conn).await.unwrap();
        client
    }

    #[tokio::test]
    async fn test_process_notification_job() {
        let provider = Arc::new(ConsoleSmsProvider);
        let job = NotificationJob::Otp {
            step_id: "test_step".to_string(),
            msisdn: "1234567890".to_string(),
            otp: "123456".to_string(),
        };
        let result = process_notification_job(provider, job).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_worker_consumes_job() {
        let redis_url = "redis://127.0.0.1:6379";
        let client = setup_redis(redis_url).await;
        let conn = client.get_multiplexed_async_connection().await.unwrap();

        let mut storage = RedisStorage::new(conn);
        let job = NotificationJob::Otp {
            step_id: "test_step".to_string(),
            msisdn: "1234567890".to_string(),
            otp: "123456".to_string(),
        };
        storage.push(job).await.unwrap();

        let provider = Arc::new(ConsoleSmsProvider);
        let provider_for_worker = provider.clone();
        let worker = WorkerBuilder::new("test-worker").backend(storage).build(
            move |job: NotificationJob| {
                let provider = provider_for_worker.clone();
                async move { process_notification_job(provider, job).await }
            },
        );

        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let _ = worker.run().await;
            let _ = tx.send(());
        });

        // This is a bit of a hack to ensure the worker has time to process the job
        // In a real-world scenario, you would use a more robust mechanism
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        let mut conn_check = client.get_multiplexed_async_connection().await.unwrap();
        let len: isize = redis::cmd("LLEN")
            .arg(format!("apalis:{}", NOTIFICATION_QUEUE_NAMESPACE))
            .query_async(&mut conn_check)
            .await
            .unwrap();
        assert_eq!(len, 0);

        let _ = rx.await;
    }
}
