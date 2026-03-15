use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDefinition {
    pub session_type: String,
    pub human_id_prefix: String,
    #[serde(default)]
    pub feature: Option<String>,
    #[serde(default)]
    pub allowed_flows: Vec<String>,
}
