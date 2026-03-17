use async_trait::async_trait;
use backend_flow_sdk::flow::StepRef;
use backend_flow_sdk::{Actor, FlowError, Step, StepContext, StepOutcome};
use serde_json::Value;
use std::sync::Arc;

pub fn steps() -> Vec<StepRef> {
    vec![
        Arc::new(super::shared_steps::CheckUserExistsStep),
        Arc::new(super::shared_steps::ValidateDepositStep),
        Arc::new(AwaitPaymentConfirmationStep),
        Arc::new(ApproveAndDepositStep),
    ]
}

pub struct AwaitPaymentConfirmationStep;

#[async_trait]
impl Step for AwaitPaymentConfirmationStep {
    fn step_type(&self) -> &'static str {
        "AWAIT_PAYMENT_CONFIRMATION"
    }

    fn actor(&self) -> Actor {
        Actor::Admin
    }

    fn human_id(&self) -> &'static str {
        "await_payment"
    }

    fn feature(&self) -> Option<&'static str> {
        Some("flow-first-deposit")
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
            "AWAIT_PAYMENT_CONFIRMATION expects object input".to_owned(),
        ))
    }
}

pub struct ApproveAndDepositStep;

#[async_trait]
impl Step for ApproveAndDepositStep {
    fn step_type(&self) -> &'static str {
        "APPROVE_AND_DEPOSIT"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "approve"
    }

    fn feature(&self) -> Option<&'static str> {
        Some("flow-first-deposit")
    }

    async fn execute(&self, _ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        Ok(StepOutcome::Done {
            output: None,
            updates: None,
        })
    }
}
