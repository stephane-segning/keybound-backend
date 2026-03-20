use super::mapping_utils::{
    JsonPointerMapping, MappingSource, apply_json_pointer_patch,
    resolve_json_pointer_mapping_value, top_level_key,
};
use crate::step::ContextUpdates;
use crate::{Actor, FlowError, Step, StepContext, StepOutcome};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataSource {
    Session,
    Flow,
    Literal,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MetadataMapping {
    pub target_path: String,
    #[serde(default)]
    pub source: Option<MetadataSource>,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub json_pointer: Option<String>,
    #[serde(default)]
    pub value: Option<Value>,
    #[serde(default)]
    pub eager: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateUserMetadataConfig {
    #[serde(default)]
    pub mappings: Vec<MetadataMapping>,
}

pub struct UpdateUserMetadataAction;

#[async_trait]
impl Step for UpdateUserMetadataAction {
    fn step_type(&self) -> &'static str {
        "UPDATE_USER_METADATA"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "update_user_metadata"
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        tracing::debug!(step = self.step_type(), "Executing step");
        let config: UpdateUserMetadataConfig = super::parse_step_config(ctx)?;
        let mut patch = Value::Object(Map::new());
        let mut eager_patch = Map::new();

        for mapping in &config.mappings {
            if let Some(value) = resolve_mapping_value(ctx, mapping)? {
                apply_json_pointer_patch(&mut patch, &mapping.target_path, value);
            }

            if let Some(eager) = mapping.eager
                && let Some(top_level_key) = top_level_key(&mapping.target_path)
            {
                eager_patch.insert(top_level_key.to_owned(), Value::Bool(eager));
            }
        }

        Ok(StepOutcome::Done {
            output: Some(json!({ "updated": true })),
            updates: Some(Box::new(ContextUpdates {
                user_metadata_patch: Some(patch),
                user_metadata_eager_patch: if eager_patch.is_empty() {
                    None
                } else {
                    Some(Value::Object(eager_patch))
                },
                ..Default::default()
            })),
        })
    }
}

fn resolve_mapping_value(
    ctx: &StepContext,
    mapping: &MetadataMapping,
) -> Result<Option<Value>, FlowError> {
    let normalized = JsonPointerMapping {
        source: mapping.source.clone().map(|source| match source {
            MetadataSource::Session => MappingSource::Session,
            MetadataSource::Flow => MappingSource::Flow,
            MetadataSource::Literal => MappingSource::Literal,
        }),
        source_path: mapping.source_path.clone(),
        json_pointer: mapping.json_pointer.clone(),
        target_path: mapping.target_path.clone(),
        value: mapping.value.clone(),
    };
    resolve_json_pointer_mapping_value(ctx, None, &normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StepServices;
    use serde_json::json;
    use std::collections::HashMap;

    #[tokio::test]
    async fn update_user_metadata_builds_nested_patch() {
        let action = UpdateUserMetadataAction;
        let mut config = HashMap::new();
        config.insert(
            "mappings".to_owned(),
            json!([
                {
                    "target_path": "/phone/verified",
                    "value": true
                },
                {
                    "target_path": "/phone/number",
                    "source": "flow",
                    "source_path": "/step_output/init_phone/phone_number"
                }
            ]),
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
                    "init_phone": {
                        "phone_number": "+237690000001"
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
            StepOutcome::Done { updates, .. } => {
                let updates = updates.unwrap();
                let patch = updates.user_metadata_patch.unwrap();
                assert_eq!(patch["phone"]["verified"], true);
                assert_eq!(patch["phone"]["number"], "+237690000001");
                assert!(updates.user_metadata_eager_patch.is_none());
            }
            _ => panic!("expected done outcome"),
        }
    }

    #[tokio::test]
    async fn update_user_metadata_collects_eager_flags_by_top_level_key() {
        let action = UpdateUserMetadataAction;
        let mut config = HashMap::new();
        config.insert(
            "mappings".to_owned(),
            json!([
                {
                    "target_path": "/fineractId",
                    "value": 1,
                    "eager": true
                },
                {
                    "target_path": "/firstDeposit/transactionId",
                    "value": 99,
                    "eager": false
                }
            ]),
        );

        let ctx = StepContext {
            session_id: "sess-1".to_owned(),
            session_user_id: None,
            flow_id: "flow-1".to_owned(),
            step_id: "step-1".to_owned(),
            input: json!({}),
            session_context: json!({}),
            flow_context: json!({}),
            services: StepServices {
                config: Some(config),
                ..Default::default()
            },
        };

        let outcome = action.execute(&ctx).await.unwrap();
        match outcome {
            StepOutcome::Done { updates, .. } => {
                let updates = updates.unwrap();
                let eager = updates.user_metadata_eager_patch.unwrap();
                assert_eq!(eager["fineractId"], true);
                assert_eq!(eager["firstDeposit"], false);
            }
            _ => panic!("expected done outcome"),
        }
    }
}
