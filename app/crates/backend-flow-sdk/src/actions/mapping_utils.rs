use crate::{FlowError, StepContext};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MappingSource {
    Session,
    Flow,
    Input,
    Response,
    Literal,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonPointerMapping {
    #[serde(default)]
    pub source: Option<MappingSource>,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub json_pointer: Option<String>,
    pub target_path: String,
    #[serde(default)]
    pub value: Option<Value>,
}

pub fn apply_json_pointer_patch(target: &mut Value, path: &str, value: Value) {
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

pub fn top_level_key(path: &str) -> Option<&str> {
    path.split('/').find(|part| !part.is_empty())
}

pub fn resolve_json_pointer_mapping_value(
    ctx: &StepContext,
    response: Option<&Value>,
    mapping: &JsonPointerMapping,
) -> Result<Option<Value>, FlowError> {
    if let Some(value) = mapping.value.clone() {
        return Ok(Some(value));
    }

    let pointer = mapping
        .source_path
        .as_deref()
        .or(mapping.json_pointer.as_deref())
        .ok_or_else(|| {
            FlowError::InvalidDefinition(
                "source_path or json_pointer is required when value is not set".to_owned(),
            )
        })?;

    let value = match mapping.source.clone() {
        Some(MappingSource::Session) => ctx.session_pointer(pointer).cloned(),
        Some(MappingSource::Flow) => ctx.flow_pointer(pointer).cloned(),
        Some(MappingSource::Input) => ctx.input.pointer(pointer).cloned(),
        Some(MappingSource::Response) => response.and_then(|resp| resp.pointer(pointer).cloned()),
        Some(MappingSource::Literal) => None,
        None => resolve_absolute_source_path(ctx, response, pointer),
    };

    Ok(value)
}

fn resolve_absolute_source_path(
    ctx: &StepContext,
    response: Option<&Value>,
    pointer: &str,
) -> Option<Value> {
    if pointer == "/session_id" {
        return Some(Value::String(ctx.session_id.clone()));
    }
    if pointer == "/flow_id" {
        return Some(Value::String(ctx.flow_id.clone()));
    }
    if pointer == "/session_user_id" {
        return ctx.session_user_id.clone().map(Value::String);
    }

    if let Some(relative) = trim_context_prefix(pointer, "/flow/context") {
        return ctx.flow_context.pointer(relative).cloned();
    }
    if let Some(relative) = trim_context_prefix(pointer, "/flow") {
        return ctx.flow_context.pointer(relative).cloned();
    }
    if let Some(relative) = trim_context_prefix(pointer, "/session/context") {
        return ctx.session_context.pointer(relative).cloned();
    }
    if let Some(relative) = trim_context_prefix(pointer, "/session") {
        return ctx.session_context.pointer(relative).cloned();
    }
    if let Some(relative) = trim_context_prefix(pointer, "/input") {
        return ctx.input.pointer(relative).cloned();
    }
    if let Some(relative) = trim_context_prefix(pointer, "/response") {
        return response.and_then(|resp| resp.pointer(relative).cloned());
    }

    None
}

fn trim_context_prefix<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if path == prefix {
        return Some("");
    }
    path.strip_prefix(prefix)
}
