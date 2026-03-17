use crate::step::ContextUpdates;
use crate::{Actor, FlowError, Step, StepContext, StepOutcome};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SetTarget {
    Session,
    #[default]
    Flow,
    User,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetConfig {
    #[serde(default)]
    pub to: SetTarget,
    #[serde(default)]
    pub values: Map<String, Value>,
}

impl Default for SetConfig {
    fn default() -> Self {
        Self {
            to: SetTarget::Flow,
            values: Map::new(),
        }
    }
}

pub struct SetAction;

#[async_trait]
impl Step for SetAction {
    fn step_type(&self) -> &'static str {
        "SET"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "set"
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        let config: SetConfig = super::parse_step_config(ctx)?;

        let values = Value::Object(config.values);

        let updates = match config.to {
            SetTarget::Session => ContextUpdates {
                session_context_patch: Some(values),
                flow_context_patch: None,
                user_metadata_patch: None,
                notifications: None,
            },
            SetTarget::Flow => ContextUpdates {
                session_context_patch: None,
                flow_context_patch: Some(values),
                user_metadata_patch: None,
                notifications: None,
            },
            SetTarget::User => ContextUpdates {
                session_context_patch: None,
                flow_context_patch: None,
                user_metadata_patch: Some(values),
                notifications: None,
            },
        };

        Ok(StepOutcome::Done {
            output: None,
            updates: Some(Box::new(updates)),
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
            step_id: "set-step".to_string(),
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
    async fn set_updates_session_context() {
        let action = SetAction;
        let mut config = HashMap::new();
        config.insert("to".to_string(), json!("session"));
        config.insert(
            "values".to_string(),
            json!({
                "key1": "value1",
                "key2": 42
            }),
        );
        let ctx = make_ctx(config);

        let result = action.execute(&ctx).await.unwrap();

        match result {
            StepOutcome::Done { output, updates } => {
                assert!(output.is_none());
                let updates = updates.unwrap();
                let patch = updates.session_context_patch.unwrap();
                assert_eq!(patch["key1"], "value1");
                assert_eq!(patch["key2"], 42);
            }
            _ => panic!("Expected Done outcome"),
        }
    }

    #[tokio::test]
    async fn set_updates_flow_context() {
        let action = SetAction;
        let mut config = HashMap::new();
        config.insert("to".to_string(), json!("flow"));
        config.insert("values".to_string(), json!({ "status": "processing" }));
        let ctx = make_ctx(config);

        let result = action.execute(&ctx).await.unwrap();

        match result {
            StepOutcome::Done { updates, .. } => {
                let updates = updates.unwrap();
                let patch = updates.flow_context_patch.unwrap();
                assert_eq!(patch["status"], "processing");
            }
            _ => panic!("Expected Done outcome"),
        }
    }

    #[tokio::test]
    async fn set_updates_user_metadata() {
        let action = SetAction;
        let mut config = HashMap::new();
        config.insert("to".to_string(), json!("user"));
        config.insert(
            "values".to_string(),
            json!({ "preferences": { "theme": "dark" } }),
        );
        let ctx = make_ctx(config);

        let result = action.execute(&ctx).await.unwrap();

        match result {
            StepOutcome::Done { updates, .. } => {
                let updates = updates.unwrap();
                let patch = updates.user_metadata_patch.unwrap();
                assert_eq!(patch["preferences"]["theme"], "dark");
            }
            _ => panic!("Expected Done outcome"),
        }
    }
}