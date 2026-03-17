use crate::{Actor, FlowError, Step, StepContext, StepOutcome};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ErrorConfig {
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub retryable: bool,
}

impl Default for ErrorConfig {
    fn default() -> Self {
        Self {
            message: "Unknown error".to_string(),
            code: String::new(),
            retryable: false,
        }
    }
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
        let config: ErrorConfig = super::parse_step_config(ctx)?;

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
    use crate::StepServices;
    use serde_json::json;
    use std::collections::HashMap;

    fn make_ctx(config: HashMap<String, serde_json::Value>) -> StepContext {
        StepContext {
            session_id: "test".to_string(),
            flow_id: "test-flow".to_string(),
            step_id: "error-step".to_string(),
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
    async fn error_returns_failed_with_message() {
        let action = ErrorAction;
        let mut config = HashMap::new();
        config.insert(
            "message".to_string(),
            json!("Something went wrong"),
        );
        config.insert("code".to_string(), json!("ERR_001"));
        config.insert("retryable".to_string(), json!(false));

        let ctx = make_ctx(config);

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
    async fn error_defaults_message_and_retryable() {
        let action = ErrorAction;
        let ctx = make_ctx(HashMap::new());

        let result = action.execute(&ctx).await.unwrap();

        match result {
            StepOutcome::Failed { error, retryable } => {
                assert_eq!(error, "Unknown error");
                assert!(!retryable);
            }
            _ => panic!("Expected Failed outcome"),
        }
    }
}