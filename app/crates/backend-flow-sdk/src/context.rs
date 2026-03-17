use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct StepContext {
    pub session_id: String,
    pub flow_id: String,
    pub step_id: String,
    pub input: Value,
    pub session_context: Value,
    pub flow_context: Value,
    pub services: StepServices,
}

#[derive(Debug, Clone, Default)]
pub struct StepServices {
    pub storage: Option<Arc<dyn StorageService>>,
    pub config: Option<HashMap<String, Value>>,
}

#[async_trait::async_trait]
pub trait StorageService: Send + Sync + std::fmt::Debug {
    async fn generate_upload_url(
        &self,
        document_type: &str,
        session_id: &str,
    ) -> Result<UploadUrlResult, String>;
}

#[derive(Debug, Clone)]
pub struct UploadUrlResult {
    pub url: String,
    pub key: String,
    pub headers: HashMap<String, String>,
}

impl StepContext {
    pub fn previous_step_output(&self, step_type: &str) -> Option<&Value> {
        self.flow_context
            .get("step_output")
            .and_then(|v| v.get(step_type))
    }

    pub fn session_config(&self, key: &str) -> Option<&Value> {
        self.session_context.get(key)
    }

    pub fn flow_config(&self, key: &str) -> Option<&Value> {
        self.flow_context.get(key)
    }

    pub fn step_config(&self, key: &str) -> Option<&Value> {
        self.services
            .config
            .as_ref()
            .and_then(|c| c.get(key))
            .or_else(|| self.input.get(key))
    }

    pub fn step_config_or<T: serde::de::DeserializeOwned + Default>(&self, key: &str) -> T {
        self.step_config(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    pub fn step_config_or_default<T: serde::de::DeserializeOwned>(
        &self,
        key: &str,
        default: T,
    ) -> T {
        self.step_config(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or(default)
    }
}
