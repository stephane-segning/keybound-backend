use async_trait::async_trait;
use backend_flow_sdk::flow::StepRef;
use backend_flow_sdk::{Actor, FlowError, Step, StepContext, StepOutcome};
use serde_json::Value;
use std::sync::Arc;

pub fn steps() -> Vec<StepRef> {
    vec![
        Arc::new(SubmitAccountUpdateStep),
        Arc::new(ApplyAccountUpdateStep),
    ]
}

pub struct SubmitAccountUpdateStep;

#[async_trait]
impl Step for SubmitAccountUpdateStep {
    fn step_type(&self) -> &'static str {
        "SUBMIT_ACCOUNT_UPDATE"
    }

    fn actor(&self) -> Actor {
        Actor::EndUser
    }

    fn human_id(&self) -> &'static str {
        "submit"
    }

    fn feature(&self) -> Option<&'static str> {
        Some("flow-account-update")
    }

    async fn execute(&self, _ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        Ok(StepOutcome::Waiting {
            actor: Actor::EndUser,
        })
    }

    async fn validate_input(&self, input: &Value) -> Result<(), FlowError> {
        let object = input.as_object().ok_or_else(|| {
            FlowError::InvalidDefinition("SUBMIT_ACCOUNT_UPDATE expects object input".to_owned())
        })?;

        if object.is_empty() {
            return Err(FlowError::InvalidDefinition(
                "SUBMIT_ACCOUNT_UPDATE requires at least one field".to_owned(),
            ));
        }

        Ok(())
    }
}

pub struct ApplyAccountUpdateStep;

#[async_trait]
impl Step for ApplyAccountUpdateStep {
    fn step_type(&self) -> &'static str {
        "APPLY_ACCOUNT_UPDATE"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "apply"
    }

    fn feature(&self) -> Option<&'static str> {
        Some("flow-account-update")
    }

    async fn execute(&self, _ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        Ok(StepOutcome::Done)
    }
}
