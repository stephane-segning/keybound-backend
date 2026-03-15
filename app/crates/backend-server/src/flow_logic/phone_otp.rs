use async_trait::async_trait;
use backend_flow_sdk::flow::StepRef;
use backend_flow_sdk::{Actor, FlowError, Step, StepContext, StepOutcome};
use serde_json::Value;
use std::sync::Arc;

pub fn steps() -> Vec<StepRef> {
    vec![Arc::new(IssuePhoneOtpStep), Arc::new(VerifyPhoneOtpStep)]
}

pub struct IssuePhoneOtpStep;

#[async_trait]
impl Step for IssuePhoneOtpStep {
    fn step_type(&self) -> &'static str {
        "ISSUE_PHONE_OTP"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "issue"
    }

    fn feature(&self) -> Option<&'static str> {
        Some("flow-phone-otp")
    }

    async fn execute(&self, _ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        Ok(StepOutcome::Done)
    }
}

pub struct VerifyPhoneOtpStep;

#[async_trait]
impl Step for VerifyPhoneOtpStep {
    fn step_type(&self) -> &'static str {
        "VERIFY_PHONE_OTP"
    }

    fn actor(&self) -> Actor {
        Actor::EndUser
    }

    fn human_id(&self) -> &'static str {
        "verify"
    }

    fn feature(&self) -> Option<&'static str> {
        Some("flow-phone-otp")
    }

    async fn execute(&self, _ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        Ok(StepOutcome::Waiting {
            actor: Actor::EndUser,
        })
    }

    async fn validate_input(&self, input: &Value) -> Result<(), FlowError> {
        if input.is_object() {
            return Ok(());
        }
        Err(FlowError::InvalidDefinition(
            "VERIFY_PHONE_OTP expects object input".to_owned(),
        ))
    }
}
