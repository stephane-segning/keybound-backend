use async_trait::async_trait;
use backend_core::Result;

#[async_trait]
pub trait SmsProvider: Send + Sync {
    async fn send_otp(&self, phone: &str, otp: &str) -> Result<()>;
}

pub struct ConsoleSmsProvider;

#[async_trait]
impl SmsProvider for ConsoleSmsProvider {
    async fn send_otp(&self, phone: &str, otp: &str) -> Result<()> {
        tracing::info!("(Console SMS) To: {}, OTP: {}", phone, otp);
        Ok(())
    }
}

pub struct SnsSmsProvider {
    client: aws_sdk_sns::Client,
}

impl SnsSmsProvider {
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
