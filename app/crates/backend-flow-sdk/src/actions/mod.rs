mod error;
mod kyc;
mod noop;
mod otp;
mod retry;
mod set;
mod wait;

#[cfg(feature = "webhook")]
mod webhook;

pub use error::ErrorAction;
pub use kyc::{DocumentType, ReviewDocumentAction, UploadDocumentAction, ValidateDepositAction};
pub use noop::NoopAction;
pub use otp::{GenerateOtpAction, VerifyOtpAction};
pub use retry::RetryAction;
pub use set::SetAction;
pub use wait::WaitAction;

#[cfg(feature = "webhook")]
pub use webhook::{
    ExtractionTarget, WebhookBehavior, WebhookExtractionRule, WebhookHttpConfig,
    WebhookRetryPolicy, WebhookStep, WebhookSuccessCondition,
};

use crate::FlowError;

fn parse_config<T: serde::de::DeserializeOwned>(
    ctx: &crate::StepContext,
    key: &str,
) -> Result<T, FlowError> {
    let val = ctx
        .flow_config(key)
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    serde_json::from_value(val).map_err(|e| FlowError::InvalidDefinition(e.to_string()))
}