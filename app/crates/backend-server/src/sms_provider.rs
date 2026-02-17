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
