use crate::{Actor, FlowError, Step, StepContext, StepOutcome};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DebugLogConfig {
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub session_pointers: Vec<String>,
    #[serde(default)]
    pub flow_pointers: Vec<String>,
}

pub struct DebugLogAction;

#[async_trait]
impl Step for DebugLogAction {
    fn step_type(&self) -> &'static str {
        "DEBUG_LOG"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "debug"
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        let config: DebugLogConfig = super::parse_step_config(ctx)?;

        if let Some(message) = config.message.as_deref() {
            tracing::info!("[DEBUG_LOG] {}", message);
        }

        for pointer in &config.session_pointers {
            tracing::info!(
                "[DEBUG_LOG] session{}={}",
                pointer,
                ctx.session_pointer(pointer)
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "null".to_owned())
            );
        }

        for pointer in &config.flow_pointers {
            tracing::info!(
                "[DEBUG_LOG] flow{}={}",
                pointer,
                ctx.flow_pointer(pointer)
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "null".to_owned())
            );
        }

        Ok(StepOutcome::Done {
            output: None,
            updates: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn debug_log_returns_done_without_updates() {
        let action = DebugLogAction;
        let ctx = StepContext {
            session_id: "sess-1".to_owned(),
            session_user_id: None,
            flow_id: "flow-1".to_owned(),
            step_id: "step-1".to_owned(),
            input: json!({}),
            session_context: json!({"status": "ready"}),
            flow_context: json!({"otp": "123456"}),
            services: Default::default(),
        };

        let outcome = action.execute(&ctx).await.unwrap();
        match outcome {
            StepOutcome::Done { output, updates } => {
                assert!(output.is_none());
                assert!(updates.is_none());
            }
            _ => panic!("expected done outcome"),
        }
    }
}
