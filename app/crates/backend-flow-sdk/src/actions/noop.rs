use crate::{Actor, FlowError, Step, StepContext, StepOutcome};
use async_trait::async_trait;

pub struct NoopAction;

#[async_trait]
impl Step for NoopAction {
    fn step_type(&self) -> &'static str {
        "NOOP"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "noop"
    }

    async fn execute(&self, _ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        Ok(StepOutcome::Done {
            output: None,
            updates: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StepServices;
    use serde_json::json;

    fn make_ctx() -> StepContext {
        StepContext {
            session_id: "test".to_string(),
            flow_id: "test-flow".to_string(),
            step_id: "noop-step".to_string(),
            input: json!({}),
            session_context: json!({}),
            flow_context: json!({}),
            services: StepServices::default(),
        }
    }

    #[tokio::test]
    async fn noop_returns_done_with_none() {
        let action = NoopAction;
        let ctx = make_ctx();
        let result = action.execute(&ctx).await.unwrap();

        match result {
            StepOutcome::Done { output, updates } => {
                assert!(output.is_none());
                assert!(updates.is_none());
            }
            _ => panic!("Expected Done outcome"),
        }
    }
}