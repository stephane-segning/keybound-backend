use crate::step::ContextUpdates;
use crate::{Actor, FlowError, Step, StepContext, StepOutcome};
use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;

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
    #[serde(default)]
    pub retryable: Option<bool>,
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

impl Default for WebhookHttpConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            method: default_method(),
            headers: HashMap::new(),
            payload: None,
            timeout_ms: default_timeout(),
            behavior: default_behavior(),
            extraction_rules: Vec::new(),
            retryable: None,
            retry_policy: None,
            success_condition: None,
        }
    }
}

pub struct WebhookStep {
    client: reqwest::Client,
}

impl Default for WebhookStep {
    fn default() -> Self {
        Self::new()
    }
}

impl WebhookStep {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Step for WebhookStep {
    fn step_type(&self) -> &'static str {
        "WEBHOOK_HTTP"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "webhook"
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        tracing::debug!(step = self.step_type(), "Executing step");
        let config: WebhookHttpConfig = super::parse_step_config(ctx)?;

        let method = reqwest::Method::from_str(&config.method).map_err(|_| {
            FlowError::InvalidDefinition(format!("Invalid HTTP method: {}", config.method))
        })?;

        let url = render_template_str(&config.url, ctx);

        let mut headers = reqwest::header::HeaderMap::new();
        for (k, v) in &config.headers {
            let rendered_v = render_template_str(v, ctx);
            if let (Ok(name), Ok(val)) = (
                reqwest::header::HeaderName::from_str(k),
                reqwest::header::HeaderValue::from_str(&rendered_v),
            ) {
                headers.insert(name, val);
            }
        }

        let payload = config.payload.map(|p| render_template_val(&p, ctx));

        let mut req_builder = self
            .client
            .request(method, &url)
            .headers(headers)
            .timeout(Duration::from_millis(config.timeout_ms));

        if let Some(p) = payload {
            req_builder = req_builder.json(&p);
        }

        match config.behavior {
            WebhookBehavior::FireAndForget => {
                let _ = req_builder.send().await;
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

                        if let Some(cond) = &config.success_condition
                            && let Some(codes) = &cond.status_codes
                        {
                            is_success = codes.contains(&status.as_u16());
                        }

                        if is_success {
                            let mut updates = ContextUpdates::default();
                            let mut step_output = Map::new();

                            if (!config.extraction_rules.is_empty()
                                || config.success_condition.is_some())
                                && let Ok(resp_json) = response.json::<Value>().await
                            {
                                if let Some(cond) = &config.success_condition
                                    && let (Some(ptr), Some(exp)) =
                                        (&cond.json_pointer, &cond.expected_value)
                                {
                                    if let Some(val) = resp_json.pointer(ptr) {
                                        if val != exp {
                                            is_success = false;
                                        }
                                    } else {
                                        is_success = false;
                                    }
                                }

                                if is_success {
                                    for rule in &config.extraction_rules {
                                        if let Some(extracted) =
                                            resp_json.pointer(&rule.json_pointer)
                                        {
                                            apply_extraction(
                                                &mut updates,
                                                &mut step_output,
                                                rule,
                                                extracted.clone(),
                                            );
                                        }
                                    }
                                }
                            }

                            if is_success {
                                return Ok(StepOutcome::Done {
                                    output: Some(Value::Object(step_output)),
                                    updates: Some(Box::new(updates)),
                                });
                            }
                        }

                        let retryable = config.retryable.unwrap_or(
                            status.is_server_error()
                                || status == reqwest::StatusCode::TOO_MANY_REQUESTS,
                        );

                        if retryable && let Some(policy) = config.retry_policy {
                            return Ok(StepOutcome::Retry {
                                after: Duration::from_millis(policy.backoff_ms),
                            });
                        }

                        Ok(StepOutcome::Failed {
                            error: format!("http_error_{}", status.as_u16()),
                            retryable,
                        })
                    }
                    Err(e) => {
                        let retryable = config.retryable.unwrap_or(true);
                        if let Some(policy) = config.retry_policy {
                            return Ok(StepOutcome::Retry {
                                after: Duration::from_millis(policy.backoff_ms),
                            });
                        }
                        Ok(StepOutcome::Failed {
                            error: format!("network_error: {}", e),
                            retryable,
                        })
                    }
                }
            }
        }
    }
}

fn apply_extraction(
    updates: &mut ContextUpdates,
    step_output: &mut Map<String, Value>,
    rule: &WebhookExtractionRule,
    value: Value,
) {
    match rule.target_context {
        ExtractionTarget::SessionContext => {
            let mut patch = updates
                .session_context_patch
                .take()
                .unwrap_or_else(|| Value::Object(Map::new()));
            apply_patch(&mut patch, &rule.target_path, value);
            updates.session_context_patch = Some(patch);
        }
        ExtractionTarget::FlowContext => {
            let mut patch = updates
                .flow_context_patch
                .take()
                .unwrap_or_else(|| Value::Object(Map::new()));
            apply_patch(&mut patch, &rule.target_path, value);
            updates.flow_context_patch = Some(patch);
        }
        ExtractionTarget::UserMetadata => {
            let mut patch = updates
                .user_metadata_patch
                .take()
                .unwrap_or_else(|| Value::Object(Map::new()));
            apply_patch(&mut patch, &rule.target_path, value);
            updates.user_metadata_patch = Some(patch);
        }
        ExtractionTarget::StepOutput => {
            let mut patch = Value::Object(step_output.clone());
            apply_patch(&mut patch, &rule.target_path, value);
            if let Value::Object(m) = patch {
                *step_output = m;
            }
        }
    }
}

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
        } else if let Value::Object(map) = current {
            if !map.contains_key(*part) {
                map.insert(part.to_string(), Value::Object(Map::new()));
            }
            current = map.get_mut(*part).unwrap();
        } else {
            return;
        }
    }
}

pub fn render_template_str(template: &str, ctx: &StepContext) -> String {
    let re = Regex::new(r"\{\{\s*([^}]+?)\s*\}\}").expect("valid template regex");
    re.replace_all(template, |captures: &regex::Captures<'_>| {
        let token = captures
            .get(1)
            .map(|m: regex::Match<'_>| m.as_str())
            .unwrap_or_default();
        resolve_template_token(token, ctx).unwrap_or_else(|| captures[0].to_string())
    })
    .into_owned()
}

pub fn render_template_val(val: &Value, ctx: &StepContext) -> Value {
    match val {
        Value::String(s) => render_template_exact_value(s, ctx)
            .unwrap_or_else(|| Value::String(render_template_str(s, ctx))),
        Value::Array(arr) => {
            Value::Array(arr.iter().map(|v| render_template_val(v, ctx)).collect())
        }
        Value::Object(map) => {
            let mut new_map = Map::new();
            for (k, v) in map {
                new_map.insert(k.clone(), render_template_val(v, ctx));
            }
            Value::Object(new_map)
        }
        _ => val.clone(),
    }
}

fn resolve_template_token(token: &str, ctx: &StepContext) -> Option<String> {
    match token {
        "session_id" => return Some(ctx.session_id.clone()),
        "flow_id" => return Some(ctx.flow_id.clone()),
        _ => {}
    }

    if let Some(path) = token.strip_prefix("session.") {
        return read_dot_path(&ctx.session_context, path);
    }

    if let Some(path) = token.strip_prefix("flow.context.") {
        return read_dot_path(&ctx.flow_context, path);
    }

    None
}

fn resolve_template_raw(token: &str, ctx: &StepContext) -> Option<Value> {
    match token {
        "session_id" => return Some(Value::String(ctx.session_id.clone())),
        "flow_id" => return Some(Value::String(ctx.flow_id.clone())),
        _ => {}
    }

    if let Some(path) = token.strip_prefix("session.") {
        return read_dot_path_value(&ctx.session_context, path).cloned();
    }

    if let Some(path) = token.strip_prefix("flow.context.") {
        return read_dot_path_value(&ctx.flow_context, path).cloned();
    }

    None
}

fn read_dot_path(value: &Value, path: &str) -> Option<String> {
    read_dot_path_value(value, path).and_then(stringify_value)
}

fn read_dot_path_value<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;

    if path.is_empty() {
        return Some(current);
    }

    for part in path.split('.') {
        current = current.get(part)?;
    }

    Some(current)
}

fn stringify_value(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(inner) => Some(inner.clone()),
        _ => Some(value.to_string()),
    }
}

fn render_template_exact_value(template: &str, ctx: &StepContext) -> Option<Value> {
    let re = Regex::new(r"^\{\{\s*([^}]+?)\s*\}\}$").ok()?;
    let captures = re.captures(template)?;
    let token = captures
        .get(1)
        .map(|m: regex::Match<'_>| m.as_str())
        .unwrap_or_default();
    resolve_template_raw(token, ctx)
}
