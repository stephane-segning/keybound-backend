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

fn parse_step_config<T: serde::de::DeserializeOwned + Default>(
    ctx: &crate::StepContext,
) -> Result<T, FlowError> {
    let val = ctx
        .services
        .config
        .as_ref()
        .map(|c| serde_json::to_value(c).unwrap_or_default())
        .unwrap_or_default();

    if val.is_null() || val.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        Ok(T::default())
    } else {
        serde_json::from_value(val).map_err(|e| FlowError::InvalidDefinition(e.to_string()))
    }
}