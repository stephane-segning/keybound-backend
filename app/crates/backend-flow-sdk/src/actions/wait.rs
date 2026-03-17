use crate::{Actor, FlowError, Step, StepContext, StepOutcome};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WaitActor {
    User,
    Admin,
}

impl From<WaitActor> for Actor {
    fn from(value: WaitActor) -> Self {
        match value {
            WaitActor::User => Actor::EndUser,
            WaitActor::Admin => Actor::Admin,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct WaitConfig {
    pub actor: WaitActor,
}

pub struct WaitAction;

#[async_trait]
impl Step for WaitAction {
    fn step_type(&self) -> &'static str {
        "WAIT"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "wait"
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        let config: WaitConfig = super::parse_config(ctx, "wait_config")?;

        Ok(StepOutcome::Waiting {
            actor: config.actor.into(),
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
            step_id: "wait-step".to_string(),
            input: json!({}),
            session_context: json!({}),
            flow_context,
            services: Default::default(),
        }
    }

    #[tokio::test]
    async fn wait_returns_waiting_for_user() {
        let action = WaitAction;
        let ctx = make_ctx(json!({
            "wait_config": { "actor": "USER" }
        }));

        let result = action.execute(&ctx).await.unwrap();

        match result {
            StepOutcome::Waiting { actor } => {
                assert_eq!(actor, Actor::EndUser);
            }
            _ => panic!("Expected Waiting outcome"),
        }
    }

    #[tokio::test]
    async fn wait_returns_waiting_for_admin() {
        let action = WaitAction;
        let ctx = make_ctx(json!({
            "wait_config": { "actor": "ADMIN" }
        }));

        let result = action.execute(&ctx).await.unwrap();

        match result {
            StepOutcome::Waiting { actor } => {
                assert_eq!(actor, Actor::Admin);
            }
            _ => panic!("Expected Waiting outcome"),
        }
    }
}