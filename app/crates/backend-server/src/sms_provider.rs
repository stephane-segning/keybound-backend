//! SMS provider abstraction for sending OTP messages.
//!
//! This module provides a pluggable SMS provider system supporting multiple backends:
//! - Console: Logs to stdout (development)
//! - SNS: AWS SNS for production SMS delivery
//! - API: Generic HTTP API integration

use async_trait::async_trait;
use backend_core::Result;

/// Trait for SMS providers that can send OTP messages.
///
/// All SMS providers must implement this trait to be used by the notification system.
/// The implementation should handle retries, error mapping, and provider-specific logic.
#[async_trait]
pub trait SmsProvider: Send + Sync {
    /// Sends an OTP message to the specified phone number.
    ///
    /// # Arguments
    /// * `phone` - Phone number in E.164 format
    /// * `otp` - The OTP code to send
    ///
    /// # Returns
    /// `Result<()>` indicating success or error
    async fn send_otp(&self, phone: &str, otp: &str) -> Result<()>;
}

/// Development SMS provider that logs to console instead of sending real messages.
///
/// This provider is useful for local development and testing where actual SMS
/// delivery is not needed. It logs the phone number and OTP to stdout.
pub struct ConsoleSmsProvider;

#[async_trait]
impl SmsProvider for ConsoleSmsProvider {
    async fn send_otp(&self, phone: &str, otp: &str) -> Result<()> {
        tracing::info!("(Console SMS) To: {}, OTP: {}", phone, otp);
        Ok(())
    }
}

/// AWS SNS SMS provider for production SMS delivery.
///
/// Uses AWS SNS to send real SMS messages to users. Requires valid AWS credentials
/// and SNS configuration in the application settings.
pub struct SnsSmsProvider {
    client: aws_sdk_sns::Client,
}

impl SnsSmsProvider {
    /// Creates a new SNS SMS provider with the given AWS client.
    ///
    /// # Arguments
    /// * `client` - AWS SNS client
    pub fn new(client: aws_sdk_sns::Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl SmsProvider for SnsSmsProvider {
    async fn send_otp(&self, phone: &str, otp: &str) -> Result<()> {
        let message = format!("Your OTP is: {}", otp);
        self.client
            .publish()
            .phone_number(phone)
            .message(message)
            .send()
            .await
            .map_err(|e| backend_core::Error::Server(e.to_string()))?;
        Ok(())
    }
}

/// Generic HTTP API SMS provider for third-party SMS services.
///
/// Integrates with external SMS APIs via HTTP. Supports authentication via bearer token.
pub struct ApiSmsProvider {
    client: reqwest::Client,
    base_url: String,
    auth_token: Option<String>,
}

impl ApiSmsProvider {
    /// Creates a new API SMS provider.
    ///
    /// # Arguments
    /// * `client` - HTTP client
    /// * `base_url` - Base URL of the SMS API
    /// * `auth_token` - Optional bearer token for authentication
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
    async fn send_otp(&self, phone: &str, otp: &str) -> Result<()> {
        let url = format!("{}/otp", self.base_url.trim_end_matches('/'));
        let mut req = self.client.post(url).json(&serde_json::json!({
            "phone": phone,
            "otp": otp,
        }));

        if let Some(token) = &self.auth_token {
            req = req.bearer_auth(token);
        }

        let resp = req.send().await.map_err(|e| {
            backend_core::Error::internal("SMS_SEND_TRANSIENT", format!("sms transport error: {e}"))
        })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if status.is_server_error() || status.as_u16() == 429 {
                return Err(backend_core::Error::internal(
                    "SMS_SEND_TRANSIENT",
                    format!("SMS API returned {status}: {body}"),
                ));
            }
            return Err(backend_core::Error::bad_request(
                "SMS_SEND_PERMANENT",
                format!("SMS API returned {status}: {body}"),
            ));
        }

        Ok(())
    }
}
