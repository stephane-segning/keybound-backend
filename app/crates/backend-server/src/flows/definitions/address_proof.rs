use async_trait::async_trait;
use backend_flow_sdk::flow::StepRef;
use backend_flow_sdk::{Actor, FlowError, Step, StepContext, StepOutcome};
use serde_json::Value;
use std::sync::Arc;

pub fn steps() -> Vec<StepRef> {
    vec![
        Arc::new(SubmitAddressProofStep),
        Arc::new(ReviewAddressProofStep),
    ]
}

pub struct SubmitAddressProofStep;

#[async_trait]
impl Step for SubmitAddressProofStep {
    fn step_type(&self) -> &'static str {
        "SUBMIT_ADDRESS_PROOF"
    }

    fn actor(&self) -> Actor {
        Actor::EndUser
    }

    fn human_id(&self) -> &'static str {
        "submit"
    }

    fn feature(&self) -> Option<&'static str> {
        Some("flow-address-proof")
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
            "SUBMIT_ADDRESS_PROOF expects object input".to_owned(),
        ))
    }
}

pub struct ReviewAddressProofStep;

#[async_trait]
impl Step for ReviewAddressProofStep {
    fn step_type(&self) -> &'static str {
        "REVIEW_ADDRESS_PROOF"
    }

    fn actor(&self) -> Actor {
        Actor::Admin
    }

    fn human_id(&self) -> &'static str {
        "review"
    }

    fn feature(&self) -> Option<&'static str> {
        Some("flow-address-proof")
    }

    async fn execute(&self, _ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        Ok(StepOutcome::Waiting {
            actor: Actor::Admin,
        })
    }

    async fn validate_input(&self, input: &Value) -> Result<(), FlowError> {
        if input.is_object() {
            return Ok(());
        }
        Err(FlowError::InvalidDefinition(
            "REVIEW_ADDRESS_PROOF expects object input".to_owned(),
        ))
    }
}
