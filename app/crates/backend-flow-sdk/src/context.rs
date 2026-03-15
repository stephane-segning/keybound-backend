use serde_json::Value;

#[derive(Debug, Clone)]
pub struct StepContext {
    pub session_id: String,
    pub flow_id: String,
    pub step_id: String,
    pub input: Value,
    pub session_context: Value,
    pub flow_context: Value,
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
}
