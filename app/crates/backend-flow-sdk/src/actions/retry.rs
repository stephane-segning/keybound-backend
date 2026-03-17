use crate::{Actor, FlowError, Step, StepContext, StepOutcome};
use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
pub struct RetryActionConfig {
    #[serde(default = "default_after_ms")]
    pub after_ms: u64,
}

fn default_after_ms() -> u64 {
    1000
}

impl Default for RetryActionConfig {
    fn default() -> Self {
        Self {
            after_ms: default_after_ms(),
        }
    }
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
        let config: RetryActionConfig = super::parse_step_config(ctx)?;

        Ok(StepOutcome::Retry {
            after: Duration::from_millis(config.after_ms),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StepServices;
    use serde_json::json;
    use std::collections::HashMap;

    fn make_ctx(config: HashMap<String, serde_json::Value>) -> StepContext {
        StepContext {
            session_id: "test".to_string(),
            session_user_id: None,
            flow_id: "test-flow".to_string(),
            step_id: "retry-step".to_string(),
            input: json!({}),
            session_context: json!({}),
            flow_context: json!({}),
            services: StepServices {
                config: Some(config),
                ..Default::default()
            },
        }
    }

    #[tokio::test]
    async fn retry_returns_retry_with_duration() {
        let action = RetryAction;
        let mut config = HashMap::new();
        config.insert("after_ms".to_string(), json!(5000));
        let ctx = make_ctx(config);

        let result = action.execute(&ctx).await.unwrap();

        match result {
            StepOutcome::Retry { after } => {
                assert_eq!(after, Duration::from_millis(5000));
            }
            _ => panic!("Expected Retry outcome"),
        }
    }

    #[tokio::test]
    async fn retry_defaults_after_ms() {
        let action = RetryAction;
        let ctx = make_ctx(HashMap::new());

        let result = action.execute(&ctx).await.unwrap();

        match result {
            StepOutcome::Retry { after } => {
                assert_eq!(after, Duration::from_millis(1000));
            }
            _ => panic!("Expected Retry outcome"),
        }
    }
}
