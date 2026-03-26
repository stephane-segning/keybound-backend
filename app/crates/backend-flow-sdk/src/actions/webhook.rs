use super::mapping_utils::{
    JsonPointerMapping, apply_json_pointer_patch, resolve_json_pointer_mapping_value,
};
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
    #[serde(default)]
    pub payload_mappings: Vec<WebhookPayloadMapping>,
    /// Raw form body string (application/x-www-form-urlencoded).
    /// Supports `${ENV}` and `{{template}}` variable substitution.
    /// When set, takes precedence over `payload` and `payload_mappings`.
    pub form_body: Option<String>,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebhookPayloadMapping {
    #[serde(default)]
    pub source: Option<WebhookMappingSource>,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub json_pointer: Option<String>,
    pub target_path: String,
    #[serde(default)]
    pub value: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookMappingSource {
    Session,
    Flow,
    Input,
    Response,
    Literal,
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
            payload_mappings: Vec::new(),
            form_body: None,
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

        let mut req_builder = self
            .client
            .request(method, &url)
            .headers(headers)
            .timeout(Duration::from_millis(config.timeout_ms));

        if let Some(form) = &config.form_body {
            let rendered = render_template_str(form, ctx);
            req_builder = req_builder
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(rendered);
        } else {
            let payload = build_payload(&config, ctx)?;
            if let Some(p) = payload {
                req_builder = req_builder.json(&p);
            }
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

fn build_payload(
    config: &WebhookHttpConfig,
    ctx: &StepContext,
) -> Result<Option<Value>, FlowError> {
    if !config.payload_mappings.is_empty() {
        let mut payload = Value::Object(Map::new());
        for mapping in &config.payload_mappings {
            let normalized = JsonPointerMapping {
                source: mapping.source.clone().map(|source| match source {
                    WebhookMappingSource::Session => super::mapping_utils::MappingSource::Session,
                    WebhookMappingSource::Flow => super::mapping_utils::MappingSource::Flow,
                    WebhookMappingSource::Input => super::mapping_utils::MappingSource::Input,
                    WebhookMappingSource::Response => super::mapping_utils::MappingSource::Response,
                    WebhookMappingSource::Literal => super::mapping_utils::MappingSource::Literal,
                }),
                source_path: mapping.source_path.clone(),
                json_pointer: mapping.json_pointer.clone(),
                target_path: mapping.target_path.clone(),
                value: mapping.value.clone(),
            };
            if let Some(value) = resolve_json_pointer_mapping_value(ctx, None, &normalized)? {
                apply_json_pointer_patch(&mut payload, &mapping.target_path, value);
            }
        }
        return Ok(Some(payload));
    }

    Ok(config.payload.as_ref().map(|p| render_template_val(p, ctx)))
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
            apply_json_pointer_patch(&mut patch, &rule.target_path, value);
            updates.session_context_patch = Some(patch);
        }
        ExtractionTarget::FlowContext => {
            let mut patch = updates
                .flow_context_patch
                .take()
                .unwrap_or_else(|| Value::Object(Map::new()));
            apply_json_pointer_patch(&mut patch, &rule.target_path, value);
            updates.flow_context_patch = Some(patch);
        }
        ExtractionTarget::UserMetadata => {
            let mut patch = updates
                .user_metadata_patch
                .take()
                .unwrap_or_else(|| Value::Object(Map::new()));
            apply_json_pointer_patch(&mut patch, &rule.target_path, value);
            updates.user_metadata_patch = Some(patch);
        }
        ExtractionTarget::StepOutput => {
            let mut patch = Value::Object(step_output.clone());
            apply_json_pointer_patch(&mut patch, &rule.target_path, value);
            if let Value::Object(m) = patch {
                *step_output = m;
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StepServices;
    use serde_json::json;

    fn make_ctx() -> StepContext {
        StepContext {
            session_id: "sess-1".to_owned(),
            session_user_id: Some("usr-1".to_owned()),
            flow_id: "flow-1".to_owned(),
            step_id: "step-1".to_owned(),
            input: json!({"amount": 1500}),
            session_context: json!({"phone_number": "+237690000001"}),
            flow_context: json!({
                "full_name": "Mbarga Benn",
                "step_output": {
                    "register": {
                        "savingsAccountId": "sav-1"
                    }
                }
            }),
            services: StepServices::default(),
        }
    }

    #[test]
    fn build_payload_from_pointer_mappings() {
        let ctx = make_ctx();
        let config = WebhookHttpConfig {
            url: "http://localhost/hook".to_owned(),
            payload: None,
            payload_mappings: vec![
                WebhookPayloadMapping {
                    source: Some(WebhookMappingSource::Flow),
                    source_path: Some("/full_name".to_owned()),
                    json_pointer: None,
                    target_path: "/fullName".to_owned(),
                    value: None,
                },
                WebhookPayloadMapping {
                    source: None,
                    source_path: Some("/session_user_id".to_owned()),
                    json_pointer: None,
                    target_path: "/externalId".to_owned(),
                    value: None,
                },
                WebhookPayloadMapping {
                    source: Some(WebhookMappingSource::Input),
                    source_path: Some("/amount".to_owned()),
                    json_pointer: None,
                    target_path: "/depositAmount".to_owned(),
                    value: None,
                },
            ],
            ..Default::default()
        };

        let payload = build_payload(&config, &ctx).unwrap().unwrap();
        assert_eq!(payload["fullName"], "Mbarga Benn");
        assert_eq!(payload["externalId"], "usr-1");
        assert_eq!(payload["depositAmount"], 1500);
    }

    #[test]
    fn build_payload_falls_back_to_template_payload() {
        let ctx = make_ctx();
        let config = WebhookHttpConfig {
            url: "http://localhost/hook".to_owned(),
            payload: Some(json!({
                "phone": "{{session.phone_number}}",
                "client": "{{flow.context.full_name}}"
            })),
            ..Default::default()
        };

        let payload = build_payload(&config, &ctx).unwrap().unwrap();
        assert_eq!(payload["phone"], "+237690000001");
        assert_eq!(payload["client"], "Mbarga Benn");
    }
}
