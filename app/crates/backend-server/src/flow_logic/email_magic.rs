use async_trait::async_trait;
use backend_flow_sdk::flow::StepRef;
use backend_flow_sdk::{Actor, FlowError, Step, StepContext, StepOutcome};
use serde_json::Value;
use std::sync::Arc;

pub fn steps() -> Vec<StepRef> {
    vec![
        Arc::new(IssueMagicEmailStep),
        Arc::new(VerifyMagicEmailStep),
    ]
}

pub struct IssueMagicEmailStep;

#[async_trait]
impl Step for IssueMagicEmailStep {
    fn step_type(&self) -> &'static str {
        "ISSUE_MAGIC_EMAIL"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "issue"
    }

    fn feature(&self) -> Option<&'static str> {
        Some("flow-email-magic")
    }

    async fn execute(&self, _ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        Ok(StepOutcome::Done { output: None, updates: None })
    }
}

pub struct VerifyMagicEmailStep;

#[async_trait]
impl Step for VerifyMagicEmailStep {
    fn step_type(&self) -> &'static str {
        "VERIFY_MAGIC_EMAIL"
    }

    fn actor(&self) -> Actor {
        Actor::EndUser
    }

    fn human_id(&self) -> &'static str {
        "verify"
    }

    fn feature(&self) -> Option<&'static str> {
        Some("flow-email-magic")
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
            "VERIFY_MAGIC_EMAIL expects object input".to_owned(),
        ))
    }
}
