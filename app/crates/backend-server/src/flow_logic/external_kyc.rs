use async_trait::async_trait;
use backend_flow_sdk::flow::StepRef;
use backend_flow_sdk::{Actor, FlowError, Step, StepContext, StepOutcome};
use std::sync::Arc;

pub fn steps() -> Vec<StepRef> {
    vec![Arc::new(super::webhook_http::WebhookHttpStep::new())]
}
