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
    pub value: Option<Value>,
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
        let config: UpdateUserMetadataConfig = super::parse_step_config(ctx)?;
        let mut patch = Value::Object(Map::new());

        for mapping in &config.mappings {
            if let Some(value) = resolve_mapping_value(ctx, mapping)? {
                apply_patch(&mut patch, &mapping.target_path, value);
            }
        }

        Ok(StepOutcome::Done {
            output: Some(json!({ "updated": true })),
            updates: Some(Box::new(ContextUpdates {
                user_metadata_patch: Some(patch),
                ..Default::default()
            })),
        })
    }
}

fn resolve_mapping_value(
    ctx: &StepContext,
    mapping: &MetadataMapping,
) -> Result<Option<Value>, FlowError> {
    if let Some(value) = mapping.value.clone() {
        return Ok(Some(value));
    }

    let source = mapping.source.clone().unwrap_or(MetadataSource::Literal);
    let pointer = mapping
        .source_path
        .as_deref()
        .ok_or_else(|| FlowError::InvalidDefinition("source_path is required".to_owned()))?;

    let value = match source {
        MetadataSource::Session => ctx.session_pointer(pointer),
        MetadataSource::Flow => ctx.flow_pointer(pointer),
        MetadataSource::Literal => None,
    };

    Ok(value.cloned())
}

fn apply_patch(target: &mut Value, path: &str, value: Value) {
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        *target = value;
        return;
    }

    let mut current = target;
    for (index, part) in parts.iter().enumerate() {
        if index == parts.len() - 1 {
            if let Value::Object(map) = current {
                map.insert((*part).to_owned(), value.clone());
            }
            return;
        }

        if !current.is_object() {
            *current = Value::Object(Map::new());
        }

        let Value::Object(map) = current else {
            return;
        };

        current = map
            .entry((*part).to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
    }
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
                let patch = updates.unwrap().user_metadata_patch.unwrap();
                assert_eq!(patch["phone"]["verified"], true);
                assert_eq!(patch["phone"]["number"], "+237690000001");
            }
            _ => panic!("expected done outcome"),
        }
    }
}
