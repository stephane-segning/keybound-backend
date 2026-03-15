use thiserror::Error;

#[derive(Debug, Error)]
pub enum FlowError {
    #[error("feature '{feature}' is not enabled for {item_kind} '{item}'")]
    FeatureNotEnabled {
        feature: String,
        item_kind: &'static str,
        item: String,
    },
    #[error("unknown step type: {0}")]
    UnknownStepType(String),
    #[error("unknown flow type: {0}")]
    UnknownFlowType(String),
    #[error("unknown session type: {0}")]
    UnknownSessionType(String),
    #[error("invalid human-readable id: {0}")]
    InvalidHumanReadableId(String),
    #[error("invalid definition: {0}")]
    InvalidDefinition(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<serde_json::Error> for FlowError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value.to_string())
    }
}

impl From<serde_yaml::Error> for FlowError {
    fn from(value: serde_yaml::Error) -> Self {
        Self::Serialization(value.to_string())
    }
}
