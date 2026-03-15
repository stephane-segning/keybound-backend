use crate::Step;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

pub type StepRef = Arc<dyn Step>;

pub trait Flow: Send + Sync + 'static {
    fn flow_type(&self) -> &'static str;
    fn human_id(&self) -> &'static str;
    fn feature(&self) -> Option<&'static str>;
    fn steps(&self) -> &[StepRef];
    fn initial_step(&self) -> &'static str;
    fn transitions(&self) -> &HashMap<String, StepTransition>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepTransition {
    pub on_success: String,
    pub on_failure: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowDefinition {
    pub api_version: String,
    pub kind: String,
    pub metadata: FlowMetadata,
    pub spec: FlowSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowMetadata {
    pub flow_type: String,
    pub human_id_prefix: String,
    #[serde(default)]
    pub feature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowSpec {
    pub steps: Vec<FlowStepDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowStepDefinition {
    pub step_type: String,
    pub actor: crate::Actor,
    pub human_id: String,
    #[serde(default)]
    pub feature: Option<String>,
    #[serde(default)]
    pub config: Option<serde_json::Value>,
    #[serde(default)]
    pub on_success: Option<String>,
    #[serde(default)]
    pub on_failure: Option<String>,
}
