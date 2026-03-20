use super::mapping_utils::{apply_json_pointer_patch, top_level_key};
use crate::step::ContextUpdates;
use crate::{Actor, FlowError, Step, StepContext, StepOutcome};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapRoot {
    #[serde(alias = "session_context")]
    Session,
    #[serde(alias = "flow_context")]
    Flow,
    Input,
    StepOutput,
    UserMetadata,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MapRef {
    pub root: MapRoot,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MapRule {
    pub from: MapRef,
    pub to: MapRef,
    #[serde(default)]
    pub eager: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MapConfig {
    #[serde(default)]
    pub mappings: Vec<MapRule>,
}

pub struct MapAction;

#[async_trait]
impl Step for MapAction {
    fn step_type(&self) -> &'static str {
        "MAP"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "map"
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        tracing::debug!(step = self.step_type(), "Executing step");
        let config: MapConfig = super::parse_step_config(ctx)?;

        let mut updates = ContextUpdates::default();
        let mut output = Value::Object(Map::new());
        let mut eager_patch = Map::new();

        for mapping in &config.mappings {
            let Some(value) = read_from_root(ctx, &mapping.from)? else {
                continue;
            };

            match mapping.to.root {
                MapRoot::Session => {
                    apply_to_updates(&mut updates.session_context_patch, &mapping.to.path, value);
                }
                MapRoot::Flow => {
                    apply_to_updates(&mut updates.flow_context_patch, &mapping.to.path, value);
                }
                MapRoot::UserMetadata => {
                    apply_to_updates(&mut updates.user_metadata_patch, &mapping.to.path, value);
                    if let Some(eager) = mapping.eager
                        && let Some(key) = top_level_key(&mapping.to.path)
                    {
                        eager_patch.insert(key.to_owned(), Value::Bool(eager));
                    }
                }
                MapRoot::StepOutput => {
                    apply_json_pointer_patch(&mut output, &mapping.to.path, value);
                }
                MapRoot::Input => {
                    return Err(FlowError::InvalidDefinition(
                        "input cannot be used as map target root".to_owned(),
                    ));
                }
            }
        }

        if !eager_patch.is_empty() {
            updates.user_metadata_eager_patch = Some(Value::Object(eager_patch));
        }

        let has_updates = updates.flow_context_patch.is_some()
            || updates.session_context_patch.is_some()
            || updates.user_metadata_patch.is_some()
            || updates.user_metadata_eager_patch.is_some()
            || updates.notifications.is_some();

        let output = match output {
            Value::Object(ref map) if map.is_empty() => None,
            other => Some(other),
        };

        Ok(StepOutcome::Done {
            output: output.or(Some(json!({"mapped": true}))),
            updates: has_updates.then_some(Box::new(updates)),
        })
    }
}

fn read_from_root(ctx: &StepContext, source: &MapRef) -> Result<Option<Value>, FlowError> {
    let pointer = normalize_pointer(&source.path);

    let value = match source.root {
        MapRoot::Session => ctx.session_pointer(&pointer),
        MapRoot::Flow => ctx.flow_pointer(&pointer),
        MapRoot::Input => ctx.input.pointer(&pointer),
        MapRoot::StepOutput => {
            let step_output_pointer = format!("/step_output{}", pointer);
            ctx.flow_pointer(&step_output_pointer)
        }
        MapRoot::UserMetadata => {
            return Err(FlowError::InvalidDefinition(
                "user_metadata cannot be used as map source root".to_owned(),
            ));
        }
    };

    Ok(value.cloned())
}

fn apply_to_updates(slot: &mut Option<Value>, path: &str, value: Value) {
    let mut patch = slot.take().unwrap_or_else(|| Value::Object(Map::new()));
    apply_json_pointer_patch(&mut patch, path, value);
    *slot = Some(patch);
}

fn normalize_pointer(path: &str) -> String {
    if path.is_empty() || path == "/" {
        String::new()
    } else if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{}", path)
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
            session_id: "sess-1".to_owned(),
            session_user_id: Some("usr-1".to_owned()),
            flow_id: "flow-1".to_owned(),
            step_id: "map-1".to_owned(),
            input: json!({"input_key": "input_value"}),
            session_context: json!({"phone": "+237690000001"}),
            flow_context: json!({
                "step_output": {
                    "register": {
                        "fineractClientId": "f-123"
                    }
                }
            }),
            services: StepServices {
                config: Some(config),
                ..Default::default()
            },
        }
    }

    #[tokio::test]
    async fn map_moves_values_across_contexts() {
        let action = MapAction;
        let mut config = HashMap::new();
        config.insert(
            "mappings".to_owned(),
            json!([
                {
                    "from": { "root": "step_output", "path": "/register/fineractClientId" },
                    "to": { "root": "user_metadata", "path": "/fineractId" },
                    "eager": true
                },
                {
                    "from": { "root": "session", "path": "/phone" },
                    "to": { "root": "flow", "path": "/normalized/phone" }
                },
                {
                    "from": { "root": "input", "path": "/input_key" },
                    "to": { "root": "step_output", "path": "/captured/input_key" }
                }
            ]),
        );

        let ctx = make_ctx(config);
        let outcome = action.execute(&ctx).await.unwrap();

        match outcome {
            StepOutcome::Done { output, updates } => {
                let output = output.unwrap();
                assert_eq!(output["captured"]["input_key"], "input_value");

                let updates = updates.unwrap();
                assert_eq!(
                    updates.user_metadata_patch.as_ref().unwrap()["fineractId"],
                    "f-123"
                );
                assert_eq!(
                    updates.flow_context_patch.as_ref().unwrap()["normalized"]["phone"],
                    "+237690000001"
                );
                assert_eq!(
                    updates.user_metadata_eager_patch.unwrap()["fineractId"],
                    true
                );
            }
            _ => panic!("expected done outcome"),
        }
    }
}
