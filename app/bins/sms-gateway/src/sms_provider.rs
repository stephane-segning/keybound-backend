use async_trait::async_trait;
use backend_core::{Error, NotificationJob};
use serde_json::json;
use std::sync::Arc;
use tracing::info;

#[cfg(test)]
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

/// SMS provider trait for sending OTP messages
#[async_trait]
pub trait SmsProvider: Send + Sync {
    /// Send an OTP code to the specified phone number
    async fn send_otp(&self, msisdn: &str, otp: &str) -> Result<(), Error>;
}

// Implement SmsProvider for Arc<T> where T implements SmsProvider
#[async_trait]
impl<T: SmsProvider + ?Sized> SmsProvider for Arc<T> {
    async fn send_otp(&self, msisdn: &str, otp: &str) -> Result<(), Error> {
        (**self).send_otp(msisdn, otp).await
    }
}

/// Console SMS provider for development - logs to console instead of sending
pub struct ConsoleSmsProvider;

#[async_trait]
impl SmsProvider for ConsoleSmsProvider {
    async fn send_otp(&self, msisdn: &str, otp: &str) -> Result<(), Error> {
        info!("CONSOLE SMS: Sending OTP {} to {}", otp, msisdn);
        Ok(())
    }
}

/// AWS SNS SMS provider for production
pub struct SnsSmsProvider {
    client: aws_sdk_sns::Client,
}

impl SnsSmsProvider {
    pub fn new(client: aws_sdk_sns::Client) -> Self {
        Self { client }
    }

    pub async fn from_config(config: &aws_config::SdkConfig) -> Self {
        let client = aws_sdk_sns::Client::new(config);
        Self::new(client)
    }
}

#[async_trait]
impl SmsProvider for SnsSmsProvider {
    async fn send_otp(&self, msisdn: &str, otp: &str) -> Result<(), Error> {
        let message = format!("Your verification code is: {}", otp);

        self.client
            .publish()
            .phone_number(msisdn)
            .message(message)
            .send()
            .await
            .map_err(|e| {
                Error::internal(
                    "SMS_SEND_FAILED",
                    format!("Failed to send SMS via SNS: {}", e),
                )
            })?;

        Ok(())
    }
}

/// Third-party API SMS provider
pub struct ApiSmsProvider {
    client: reqwest::Client,
    base_url: String,
    auth_token: Option<String>,
}

impl ApiSmsProvider {
    pub fn new(client: reqwest::Client, base_url: String, auth_token: Option<String>) -> Self {
        Self {
            client,
            base_url,
            auth_token,
        }
    }
}

#[async_trait]
impl SmsProvider for ApiSmsProvider {
    async fn send_otp(&self, msisdn: &str, otp: &str) -> Result<(), Error> {
        let url = format!("{}/otp", self.base_url.trim_end_matches('/'));
        let mut request = self.client.post(&url).json(&json!({
            "msisdn": msisdn,
            "otp": otp
        }));

        if let Some(token) = &self.auth_token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await.map_err(|e| {
            Error::internal(
                "SMS_SEND_TRANSIENT",
                format!("Failed to contact SMS API: {}", e),
            )
        })?;

        let status = response.status();
        if status.is_success() {
            Ok(())
        } else if status.is_server_error() {
            let error_text = response.text().await.unwrap_or_default();
            Err(Error::internal(
                "SMS_SEND_TRANSIENT",
                format!("SMS API server error ({}): {}", status, error_text),
            ))
        } else {
            let error_text = response.text().await.unwrap_or_default();
            Err(Error::internal(
                "SMS_SEND_PERMANENT",
                format!("SMS API client error ({}): {}", status, error_text),
            ))
        }
    }
}

/// Process a notification job using the given SMS provider
pub async fn process_notification_job(
    provider: Arc<dyn SmsProvider>,
    job: NotificationJob,
) -> Result<(), Error> {
    match job {
        NotificationJob::Otp {
            step_id,
            msisdn,
            otp,
        } => {
            info!("Processing OTP job for step: {}", step_id);
            provider.send_otp(&msisdn, &otp).await?;
            Ok(())
        }
        NotificationJob::MagicEmail { .. } => {
            // Email notifications are not handled by SMS gateway
            info!("Skipping non-SMS notification job");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn console_sms_provider_logs_to_console() {
        let provider = ConsoleSmsProvider;
        let result = provider.send_otp("1234567890", "123456").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn api_sms_provider_sends_sms() {
        let server = MockServer::start().await;
        let client = reqwest::Client::new();
        let provider = ApiSmsProvider::new(client, server.uri(), Some("test_token".to_string()));

        Mock::given(method("POST"))
            .and(path("/otp"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let result = provider.send_otp("1234567890", "123456").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn api_sms_provider_handles_transient_error() {
        let server = MockServer::start().await;
        let client = reqwest::Client::new();
        let provider = ApiSmsProvider::new(client, server.uri(), Some("test_token".to_string()));

        Mock::given(method("POST"))
            .and(path("/otp"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let result = provider.send_otp("1234567890", "123456").await;
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(matches!(error, Error::Http { .. }));
        if let Error::Http { error_key, .. } = error {
            assert_eq!(error_key, "SMS_SEND_TRANSIENT");
        }
    }

    #[tokio::test]
    async fn api_sms_provider_handles_permanent_error() {
        let server = MockServer::start().await;
        let client = reqwest::Client::new();
        let provider = ApiSmsProvider::new(client, server.uri(), Some("test_token".to_string()));

        Mock::given(method("POST"))
            .and(path("/otp"))
            .respond_with(ResponseTemplate::new(400))
            .mount(&server)
            .await;

        let result = provider.send_otp("1234567890", "123456").await;
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(matches!(error, Error::Http { .. }));
        if let Error::Http { error_key, .. } = error {
            assert_eq!(error_key, "SMS_SEND_PERMANENT");
        }
    }

    #[tokio::test]
    async fn process_notification_job_sends_otp() {
        let provider = std::sync::Arc::new(ConsoleSmsProvider);
        let job = NotificationJob::Otp {
            step_id: "test_step".to_string(),
            msisdn: "1234567890".to_string(),
            otp: "123456".to_string(),
        };
        let result = process_notification_job(provider, job).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn process_notification_job_skips_email() {
        let provider = std::sync::Arc::new(ConsoleSmsProvider);
        let job = NotificationJob::MagicEmail {
            step_id: "test_step".to_string(),
            email: "test@example.com".to_string(),
            token: "token123".to_string(),
        };
        let result = process_notification_job(provider, job).await;
        assert!(result.is_ok());
    }
}
