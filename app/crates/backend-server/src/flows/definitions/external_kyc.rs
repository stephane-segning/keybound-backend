use backend_flow_sdk::WebhookStep;
use backend_flow_sdk::flow::StepRef;
use std::sync::Arc;

pub fn steps() -> Vec<StepRef> {
    vec![Arc::new(WebhookStep::new())]
}
