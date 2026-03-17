use crate::{Actor, FlowError, Step, StepContext, StepOutcome};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConditionalSource {
    Session,
    #[default]
    Flow,
    Input,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConditionalConfig {
    #[serde(default)]
    pub source: ConditionalSource,
    pub pointer: String,
    #[serde(default)]
    pub cases: HashMap<String, String>,
    #[serde(default)]
    pub default_branch: Option<String>,
}

pub struct ConditionalAction;

#[async_trait]
impl Step for ConditionalAction {
    fn step_type(&self) -> &'static str {
        "CONDITIONAL"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "conditional"
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        let config: ConditionalConfig = super::parse_step_config(ctx)?;

        let selected = match config.source {
            ConditionalSource::Session => ctx.session_pointer(&config.pointer),
            ConditionalSource::Flow => ctx.flow_pointer(&config.pointer),
            ConditionalSource::Input => ctx.input.pointer(&config.pointer),
        };

        let branch = selected
            .map(stringify_value)
            .and_then(|key| config.cases.get(&key).cloned())
            .or(config.default_branch.clone())
            .ok_or_else(|| {
                FlowError::InvalidDefinition(format!(
                    "No conditional branch matched pointer {}",
                    config.pointer
                ))
            })?;

        Ok(StepOutcome::Branched {
            branch,
            output: selected.cloned(),
            updates: None,
        })
    }
}

fn stringify_value(value: &Value) -> String {
    match value {
        Value::String(inner) => inner.clone(),
        Value::Bool(inner) => inner.to_string(),
        Value::Number(inner) => inner.to_string(),
        Value::Null => "null".to_owned(),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StepServices;
    use serde_json::json;
    use std::collections::HashMap;

    #[tokio::test]
    async fn conditional_returns_named_branch() {
        let action = ConditionalAction;
        let mut config = HashMap::new();
        config.insert("source".to_owned(), json!("flow"));
        config.insert(
            "pointer".to_owned(),
            json!("/step_output/await_admin_decision/decision"),
        );
        config.insert(
            "cases".to_owned(),
            json!({
                "APPROVED": "approved",
                "REJECTED": "rejected"
            }),
        );

        let ctx = StepContext {
            session_id: "sess-1".to_owned(),
            session_user_id: None,
            flow_id: "flow-1".to_owned(),
            step_id: "step-1".to_owned(),
            input: json!({}),
            session_context: json!({}),
            flow_context: json!({
                "step_output": {
                    "await_admin_decision": {
                        "decision": "APPROVED"
                    }
                }
            }),
            services: StepServices {
                config: Some(config),
                ..Default::default()
            },
        };

        let outcome = action.execute(&ctx).await.unwrap();
        match outcome {
            StepOutcome::Branched { branch, .. } => assert_eq!(branch, "approved"),
            _ => panic!("expected branched outcome"),
        }
    }
}
