use async_trait::async_trait;
use backend_flow_sdk::flow::StepRef;
use backend_flow_sdk::{Actor, FlowError, Step, StepContext, StepOutcome};
use std::sync::Arc;

pub fn steps() -> Vec<StepRef> {
    vec![Arc::new(WebhookHttpStep)]
}

pub struct WebhookHttpStep;

#[async_trait]
impl Step for WebhookHttpStep {
    fn step_type(&self) -> &'static str {
        "WEBHOOK_HTTP"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "call_external"
    }

    fn feature(&self) -> Option<&'static str> {
        Some("flow-external-kyc")
    }

    async fn execute(&self, _ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        Ok(StepOutcome::Done)
    }
}
