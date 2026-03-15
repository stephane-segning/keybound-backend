use crate::{FlowDefinition, FlowError, SessionDefinition};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Json,
    Yaml,
}

pub fn export_registry<T: serde::Serialize>(
    value: &T,
    format: ExportFormat,
) -> Result<String, FlowError> {
    match format {
        ExportFormat::Json => serde_json::to_string_pretty(value).map_err(Into::into),
        ExportFormat::Yaml => serde_yaml::to_string(value).map_err(Into::into),
    }
}

pub fn export_flow_definition(
    definition: &FlowDefinition,
    format: ExportFormat,
) -> Result<String, FlowError> {
    export_registry(definition, format)
}

pub fn export_session_definition(
    definition: &SessionDefinition,
    format: ExportFormat,
) -> Result<String, FlowError> {
    export_registry(definition, format)
}
