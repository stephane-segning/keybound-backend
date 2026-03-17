use crate::{FlowDefinition, FlowError, SessionDefinition};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    Json,
    Yaml,
}

impl ImportFormat {
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => Self::Json,
            _ => Self::Yaml,
        }
    }
}

pub fn import_flow_definition(
    content: &str,
    format: ImportFormat,
) -> Result<FlowDefinition, FlowError> {
    let definition = match format {
        ImportFormat::Json => serde_json::from_str(content)?,
        ImportFormat::Yaml => serde_yaml::from_str(content)?,
    };

    validate_flow_definition(&definition)?;
    Ok(definition)
}

pub fn import_session_definition(
    content: &str,
    format: ImportFormat,
) -> Result<SessionDefinition, FlowError> {
    let definition: SessionDefinition = match format {
        ImportFormat::Json => serde_json::from_str(content)?,
        ImportFormat::Yaml => serde_yaml::from_str(content)?,
    };

    if definition.session_type.trim().is_empty() {
        return Err(FlowError::InvalidDefinition(
            "session_type cannot be empty".to_owned(),
        ));
    }

    Ok(definition)
}

fn validate_flow_definition(definition: &FlowDefinition) -> Result<(), FlowError> {
    if definition.flow_type.trim().is_empty() {
        return Err(FlowError::InvalidDefinition(
            "flow_type cannot be empty".to_owned(),
        ));
    }
    if definition.steps.is_empty() {
        return Err(FlowError::InvalidDefinition(
            "steps cannot be empty".to_owned(),
        ));
    }

    for (step_name, step) in &definition.steps {
        if step.action.trim().is_empty() {
            return Err(FlowError::InvalidDefinition(format!(
                "step '{}' action cannot be empty",
                step_name
            )));
        }
    }

    if !definition.steps.contains_key(&definition.initial_step) {
        return Err(FlowError::InvalidDefinition(format!(
            "initial_step '{}' not found in steps",
            definition.initial_step
        )));
    }

    Ok(())
}
