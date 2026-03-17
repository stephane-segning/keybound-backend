use crate::step::ContextUpdates;
use crate::{Actor, FlowError, Step, StepContext, StepOutcome};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CloseSessionConfig {
    #[serde(default)]
    pub reason: Option<String>,
}

pub struct CloseSessionAction;

#[async_trait]
impl Step for CloseSessionAction {
    fn step_type(&self) -> &'static str {
        "CLOSE_SESSION"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "close_session"
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        let config: CloseSessionConfig = super::parse_step_config(ctx)?;
        let reason = config.reason.unwrap_or_else(|| "CLOSED".to_owned());

        Ok(StepOutcome::Done {
            output: Some(json!({ "closed": true, "reason": reason })),
            updates: Some(Box::new(ContextUpdates {
                session_context_patch: Some(json!({
                    "close_reason": reason,
                    "closed_by_step": ctx.step_id,
                })),
                ..Default::default()
            })),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn close_session_writes_reason() {
        let action = CloseSessionAction;
        let ctx = StepContext {
            session_id: "sess-1".to_owned(),
            session_user_id: None,
            flow_id: "flow-1".to_owned(),
            step_id: "step-1".to_owned(),
            input: json!({}),
            session_context: json!({}),
            flow_context: json!({}),
            services: Default::default(),
        };

        let outcome = action.execute(&ctx).await.unwrap();
        match outcome {
            StepOutcome::Done { updates, .. } => {
                let patch = updates.unwrap().session_context_patch.unwrap();
                assert_eq!(patch["close_reason"], "CLOSED");
            }
            _ => panic!("expected done outcome"),
        }
    }
}
