use crate::FlowError;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HumanReadableId(String);

impl HumanReadableId {
    pub fn new(parts: &[&str]) -> Result<Self, FlowError> {
        if parts.is_empty() {
            return Err(FlowError::InvalidHumanReadableId(
                "id must contain at least one part".to_owned(),
            ));
        }

        let mut normalized = Vec::with_capacity(parts.len());
        for part in parts {
            let trimmed = part.trim();
            if trimmed.is_empty() || trimmed.contains('.') {
                return Err(FlowError::InvalidHumanReadableId(trimmed.to_owned()));
            }
            normalized.push(trimmed.to_owned());
        }

        Ok(Self(normalized.join(".")))
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, FlowError> {
        let raw = value.into();
        let parts = raw.split('.').collect::<Vec<_>>();
        Self::new(&parts)
    }

    pub fn parent(&self) -> Option<Self> {
        let mut parts = self.parts();
        if parts.len() <= 1 {
            return None;
        }
        parts.pop();
        Self::new(&parts).ok()
    }

    pub fn parts(&self) -> Vec<&str> {
        self.0.split('.').collect()
    }

    pub fn with_suffix(&self, suffix: &str) -> Result<Self, FlowError> {
        let mut parts = self.parts();
        parts.push(suffix);
        Self::new(&parts)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for HumanReadableId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
