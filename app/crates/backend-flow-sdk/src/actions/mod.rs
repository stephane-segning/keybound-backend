mod close_session;
mod conditional;
mod debug;
mod error;
mod get_user;
mod kyc;
mod noop;
mod otp;
mod retry;
mod set;
mod update_user_metadata;
mod wait;
mod webhook;

pub use close_session::CloseSessionAction;
pub use conditional::ConditionalAction;
pub use debug::DebugLogAction;
pub use error::ErrorAction;
pub use get_user::GetUserAction;
pub use kyc::{DocumentType, ReviewDocumentAction, UploadDocumentAction, ValidateDepositAction};
pub use noop::NoopAction;
pub use otp::{GenerateOtpAction, VerifyOtpAction};
pub use retry::RetryAction;
pub use set::SetAction;
pub use update_user_metadata::UpdateUserMetadataAction;
pub use wait::WaitAction;

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
