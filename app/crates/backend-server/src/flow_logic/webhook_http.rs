use async_trait::async_trait;
use backend_flow_sdk::{Actor, FlowError, Step, StepContext, StepOutcome};
use backend_flow_sdk::step::ContextUpdates;
use reqwest::{Client, Method, header::{HeaderMap, HeaderName, HeaderValue}};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;
use tracing::{error, info, warn};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookBehavior {
    FireAndForget,
    WaitForResponse,
    WaitAndSave,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebhookExtractionRule {
    pub json_pointer: String,
    pub target_path: String,
    pub target_context: ExtractionTarget,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionTarget {
    SessionContext,
    FlowContext,
    UserMetadata,
    StepOutput,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebhookRetryPolicy {
    pub max_attempts: u32,
    pub backoff_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebhookSuccessCondition {
    pub status_codes: Option<Vec<u16>>,
    pub json_pointer: Option<String>,
    pub expected_value: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebhookHttpConfig {
    pub url: String,
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub payload: Option<Value>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default = "default_behavior")]
    pub behavior: WebhookBehavior,
    #[serde(default)]
    pub extraction_rules: Vec<WebhookExtractionRule>,
    pub retry_policy: Option<WebhookRetryPolicy>,
    pub success_condition: Option<WebhookSuccessCondition>,
}

fn default_method() -> String {
    "POST".to_string()
}
fn default_timeout() -> u64 {
    5000
}
fn default_behavior() -> WebhookBehavior {
    WebhookBehavior::WaitForResponse
}

pub struct WebhookHttpStep {
    client: Client,
}

impl WebhookHttpStep {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
}

impl Default for WebhookHttpStep {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Step for WebhookHttpStep {
    fn step_type(&self) -> &'static str {
        "WEBHOOK_HTTP"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "call_webhook"
    }

    fn feature(&self) -> Option<&'static str> {
        None // Usually bound to the flow
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        let config_val = ctx.flow_config("webhook_config")
            .or_else(|| ctx.session_config("webhook_config"))
            .cloned()
            .unwrap_or_default();

        let config: WebhookHttpConfig = match serde_json::from_value(config_val) {
            Ok(c) => c,
            Err(e) => {
                error!("Invalid webhook config: {}", e);
                return Ok(StepOutcome::Failed {
                    error: "invalid_webhook_config".to_string(),
                    retryable: false,
                });
            }
        };

        let method = match Method::from_str(&config.method) {
            Ok(m) => m,
            Err(_) => {
                error!("Invalid HTTP method: {}", config.method);
                return Ok(StepOutcome::Failed {
                    error: "invalid_http_method".to_string(),
                    retryable: false,
                });
            }
        };

        // Render URL
        let url = render_template_str(&config.url, ctx);

        let mut headers = HeaderMap::new();
        for (k, v) in &config.headers {
            let rendered_v = render_template_str(v, ctx);
            if let (Ok(name), Ok(val)) = (
                HeaderName::from_str(k),
                HeaderValue::from_str(&rendered_v),
            ) {
                headers.insert(name, val);
            }
        }

        let payload = config.payload.map(|p| render_template_val(&p, ctx));

        let mut req_builder = self.client.request(method, &url)
            .headers(headers)
            .timeout(Duration::from_millis(config.timeout_ms));

        if let Some(p) = payload {
            req_builder = req_builder.json(&p);
        }

        match config.behavior {
            WebhookBehavior::FireAndForget => {
                // We send and immediately return Done, not waiting for success
                let _ = req_builder.send().await; // In a real implementation we might spawn this
                Ok(StepOutcome::Done {
                    output: None,
                    updates: None,
                })
            }
            WebhookBehavior::WaitForResponse | WebhookBehavior::WaitAndSave => {
                let res = req_builder.send().await;
                match res {
                    Ok(response) => {
                        let status = response.status();
                        let mut is_success = status.is_success();

                        if let Some(cond) = &config.success_condition {
                            if let Some(codes) = &cond.status_codes {
                                is_success = codes.contains(&status.as_u16());
                            }
                        }

                        if is_success {
                            let mut updates = ContextUpdates::default();
                            let mut step_output = serde_json::Map::new();

                            if !config.extraction_rules.is_empty() || config.success_condition.is_some() {
                                if let Ok(resp_json) = response.json::<Value>().await {
                                    // check specific json pointer condition if needed
                                    if let Some(cond) = &config.success_condition {
                                        if let (Some(ptr), Some(exp)) = (&cond.json_pointer, &cond.expected_value) {
                                            if let Some(val) = resp_json.pointer(ptr) {
                                                if val != exp {
                                                    is_success = false;
                                                }
                                            } else {
                                                is_success = false;
                                            }
                                        }
                                    }

                                    if is_success {
                                        for rule in &config.extraction_rules {
                                            if let Some(extracted) = resp_json.pointer(&rule.json_pointer) {
                                                match rule.target_context {
                                                    ExtractionTarget::SessionContext => {
                                                        let mut patch = updates.session_context_patch.unwrap_or_else(|| Value::Object(serde_json::Map::new()));
                                                        apply_patch(&mut patch, &rule.target_path, extracted.clone());
                                                        updates.session_context_patch = Some(patch);
                                                    }
                                                    ExtractionTarget::FlowContext => {
                                                        let mut patch = updates.flow_context_patch.unwrap_or_else(|| Value::Object(serde_json::Map::new()));
                                                        apply_patch(&mut patch, &rule.target_path, extracted.clone());
                                                        updates.flow_context_patch = Some(patch);
                                                    }
                                                    ExtractionTarget::UserMetadata => {
                                                        let mut patch = updates.user_metadata_patch.unwrap_or_else(|| Value::Object(serde_json::Map::new()));
                                                        apply_patch(&mut patch, &rule.target_path, extracted.clone());
                                                        updates.user_metadata_patch = Some(patch);
                                                    }
                                                    ExtractionTarget::StepOutput => {
                                                        let mut patch = Value::Object(step_output.clone());
                                                        apply_patch(&mut patch, &rule.target_path, extracted.clone());
                                                        if let Value::Object(m) = patch {
                                                            step_output = m;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            if is_success {
                                return Ok(StepOutcome::Done {
                                    output: Some(Value::Object(step_output)),
                                    updates: Some(updates),
                                });
                            }
                        }

                        warn!("Webhook non-success status: {}", status);
                        let retryable = status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS;
                        if retryable {
                            if let Some(policy) = config.retry_policy {
                                // Normally we would track attempts. For now, just retry
                                return Ok(StepOutcome::Retry {
                                    after: Duration::from_millis(policy.backoff_ms),
                                });
                            }
                        }
                        Ok(StepOutcome::Failed {
                            error: format!("http_error_{}", status.as_u16()),
                            retryable: false,
                        })
                    }
                    Err(e) => {
                        warn!("Webhook request failed: {}", e);
                        if let Some(policy) = config.retry_policy {
                            return Ok(StepOutcome::Retry {
                                after: Duration::from_millis(policy.backoff_ms),
                            });
                        }
                        Ok(StepOutcome::Failed {
                            error: "network_error".to_string(),
                            retryable: false,
                        })
                    }
                }
            }
        }
    }
}

// Simple templating utility: replaces {{var}} with actual values
fn render_template_str(template: &str, ctx: &StepContext) -> String {
    let mut result = template.to_string();
    // A proper implementation would use regex or a templating engine
    // For now, simple manual replace for common paths
    
    // session.*
    if result.contains("{{session.") {
        if let Value::Object(map) = &ctx.session_context {
            for (k, v) in map {
                let pattern = format!("{{{{session.{}}}}}", k);
                if result.contains(&pattern) {
                    let val_str = match v {
                        Value::String(s) => s.clone(),
                        _ => v.to_string(),
                    };
                    result = result.replace(&pattern, &val_str);
                }
            }
        }
    }

    // flow.context.*
    if result.contains("{{flow.context.") {
        if let Value::Object(map) = &ctx.flow_context {
            for (k, v) in map {
                let pattern = format!("{{{{flow.context.{}}}}}", k);
                if result.contains(&pattern) {
                    let val_str = match v {
                        Value::String(s) => s.clone(),
                        _ => v.to_string(),
                    };
                    result = result.replace(&pattern, &val_str);
                }
            }
        }
    }

    // Also support session_id, flow_id
    result = result.replace("{{session_id}}", &ctx.session_id);
    result = result.replace("{{flow_id}}", &ctx.flow_id);

    result
}

fn render_template_val(val: &Value, ctx: &StepContext) -> Value {
    match val {
        Value::String(s) => Value::String(render_template_str(s, ctx)),
        Value::Array(arr) => {
            Value::Array(arr.iter().map(|v| render_template_val(v, ctx)).collect())
        }
        Value::Object(map) => {
            let mut new_map = serde_json::Map::new();
            for (k, v) in map {
                new_map.insert(k.clone(), render_template_val(v, ctx));
            }
            Value::Object(new_map)
        }
        _ => val.clone(),
    }
}

// Applies a JSON pointer patch
fn apply_patch(target: &mut Value, path: &str, value: Value) {
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return;
    }
    
    let mut current = target;
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            if let Value::Object(map) = current {
                map.insert(part.to_string(), value.clone());
            }
        } else {
            if let Value::Object(map) = current {
                if !map.contains_key(*part) {
                    map.insert(part.to_string(), Value::Object(serde_json::Map::new()));
                }
                current = map.get_mut(*part).unwrap();
            } else {
                return;
            }
        }
    }
}
