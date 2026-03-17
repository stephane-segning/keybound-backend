use crate::{Actor, FlowError, Step, StepContext, StepOutcome};
use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
pub struct RetryConfig {
    pub after_ms: u64,
}

pub struct RetryAction;

#[async_trait]
impl Step for RetryAction {
    fn step_type(&self) -> &'static str {
        "RETRY"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "retry"
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        let config: RetryConfig = super::parse_config(ctx, "retry_config")?;

        Ok(StepOutcome::Retry {
            after: Duration::from_millis(config.after_ms),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_ctx(flow_context: serde_json::Value) -> StepContext {
        StepContext {
            session_id: "test".to_string(),
            flow_id: "test-flow".to_string(),
            step_id: "retry-step".to_string(),
            input: json!({}),
            session_context: json!({}),
            flow_context,
            services: Default::default(),
        }
    }

    #[tokio::test]
    async fn retry_returns_retry_with_duration() {
        let action = RetryAction;
        let ctx = make_ctx(json!({
            "retry_config": { "after_ms": 5000 }
        }));

        let result = action.execute(&ctx).await.unwrap();

        match result {
            StepOutcome::Retry { after } => {
                assert_eq!(after, Duration::from_millis(5000));
            }
            _ => panic!("Expected Retry outcome"),
        }
    }
}