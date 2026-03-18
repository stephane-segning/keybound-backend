use backend_flow_sdk::{WebhookStep, flow::StepRef};
use std::sync::Arc;

pub fn steps() -> Vec<StepRef> {
    vec![Arc::new(WebhookStep::new())]
}
