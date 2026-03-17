use crate::Step;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

pub type StepRef = Arc<dyn Step>;

pub trait Flow: Send + Sync + 'static {
    fn flow_type(&self) -> &str;
    fn human_id(&self) -> &str;
    fn feature(&self) -> Option<&str>;
    fn steps(&self) -> &[StepRef];
    fn initial_step(&self) -> &str;
    fn transitions(&self) -> &HashMap<String, StepTransition>;

    fn find_next_step(&self, current_step: &str) -> Option<&str> {
        self.transitions()
            .get(current_step)
            .map(|t| t.on_success.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepTransition {
    pub on_success: String,
    #[serde(default)]
    pub on_failure: Option<String>,
    #[serde(default)]
    pub branches: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowDefinition {
    pub flow_type: String,
    pub human_id_prefix: String,
    #[serde(default)]
    pub feature: Option<String>,
    pub initial_step: String,
    pub steps: HashMap<String, FlowStepDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowStepDefinition {
    pub action: String,
    pub actor: crate::Actor,
    #[serde(default)]
    pub config: Option<serde_json::Value>,
    #[serde(default)]
    pub retry: Option<RetryConfig>,
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default)]
    pub ok: Option<String>,
    #[serde(default)]
    pub fail: Option<String>,
    #[serde(default)]
    pub branches: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    #[serde(default = "default_max_retries")]
    pub max: i32,
    #[serde(default = "default_delay_ms")]
    pub delay_ms: u64,
}

fn default_max_retries() -> i32 {
    3
}

fn default_delay_ms() -> u64 {
    1000
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max: default_max_retries(),
            delay_ms: default_delay_ms(),
        }
    }
}

impl FlowDefinition {
    pub fn get_step_retry_config(&self, step_name: &str) -> RetryConfig {
        self.steps
            .get(step_name)
            .and_then(|s| s.retry.clone())
            .unwrap_or_default()
    }
}
