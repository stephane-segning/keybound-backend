use crate::{Actor, FlowError, Step, StepContext, StepOutcome};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ErrorConfig {
    pub message: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub retryable: bool,
}

pub struct ErrorAction;

#[async_trait]
impl Step for ErrorAction {
    fn step_type(&self) -> &'static str {
        "ERROR"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "error"
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        let config: ErrorConfig = super::parse_config(ctx, "error_config")?;

        Ok(StepOutcome::Failed {
            error: if config.code.is_empty() {
                config.message
            } else {
                format!("{}: {}", config.code, config.message)
            },
            retryable: config.retryable,
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
            step_id: "error-step".to_string(),
            input: json!({}),
            session_context: json!({}),
            flow_context,
            services: Default::default(),
        }
    }

    #[tokio::test]
    async fn error_returns_failed_with_message() {
        let action = ErrorAction;
        let ctx = make_ctx(json!({
            "error_config": {
                "message": "Something went wrong",
                "code": "ERR_001",
                "retryable": false
            }
        }));

        let result = action.execute(&ctx).await.unwrap();

        match result {
            StepOutcome::Failed { error, retryable } => {
                assert_eq!(error, "ERR_001: Something went wrong");
                assert!(!retryable);
            }
            _ => panic!("Expected Failed outcome"),
        }
    }

    #[tokio::test]
    async fn error_defaults_code_and_retryable() {
        let action = ErrorAction;
        let ctx = make_ctx(json!({
            "error_config": {
                "message": "Simple error"
            }
        }));

        let result = action.execute(&ctx).await.unwrap();

        match result {
            StepOutcome::Failed { error, retryable } => {
                assert_eq!(error, "Simple error");
                assert!(!retryable);
            }
            _ => panic!("Expected Failed outcome"),
        }
    }
}