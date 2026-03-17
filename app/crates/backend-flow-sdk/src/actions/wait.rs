use crate::{Actor, FlowError, Step, StepContext, StepOutcome};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WaitActor {
    #[default]
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
    #[serde(default)]
    pub actor: WaitActor,
}

impl Default for WaitConfig {
    fn default() -> Self {
        Self {
            actor: WaitActor::User,
        }
    }
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
        let config: WaitConfig = super::parse_step_config(ctx)?;

        Ok(StepOutcome::Waiting {
            actor: config.actor.into(),
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
            step_id: "wait-step".to_string(),
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
    async fn wait_returns_waiting_for_user() {
        let action = WaitAction;
        let mut config = HashMap::new();
        config.insert("actor".to_string(), json!("USER"));
        let ctx = make_ctx(config);

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
        let mut config = HashMap::new();
        config.insert("actor".to_string(), json!("ADMIN"));
        let ctx = make_ctx(config);

        let result = action.execute(&ctx).await.unwrap();

        match result {
            StepOutcome::Waiting { actor } => {
                assert_eq!(actor, Actor::Admin);
            }
            _ => panic!("Expected Waiting outcome"),
        }
    }

    #[tokio::test]
    async fn wait_defaults_to_user() {
        let action = WaitAction;
        let ctx = make_ctx(HashMap::new());

        let result = action.execute(&ctx).await.unwrap();

        match result {
            StepOutcome::Waiting { actor } => {
                assert_eq!(actor, Actor::EndUser);
            }
            _ => panic!("Expected Waiting outcome"),
        }
    }
}
