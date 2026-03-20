use async_trait::async_trait;
use backend_flow_sdk::step::ContextUpdates;
use backend_flow_sdk::{Actor, FlowError, Step, StepContext, StepOutcome};
use serde::Deserialize;
use serde_json::json;
use tracing::{debug, instrument};

/// Internal contact representation for deposit recipients
#[derive(Debug, Clone)]
pub struct DepositRecipientContact {
    pub staff_id: String,
    pub full_name: String,
    pub phone_number: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RecipientRegexRule {
    pub provider: String,
    #[serde(rename = "fullname", alias = "full-name")]
    pub full_name: String,
    #[serde(alias = "phone-number")]
    pub phone_number: String,
    pub regex: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ResolveRecipientConfig {
    #[serde(default)]
    recipients: Vec<RecipientRegexRule>,
}

/// Resolves recipient contact for deposit based on configured phone regex rules.
pub struct ResolveRecipientStep;

#[async_trait]
impl Step for ResolveRecipientStep {
    fn step_type(&self) -> &'static str {
        "RESOLVE_RECIPIENT"
    }
    fn actor(&self) -> Actor {
        Actor::System
    }
    fn human_id(&self) -> &'static str {
        "resolve_recipient"
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        tracing::debug!(step = "RESOLVE_RECIPIENT", "Executing step");
        let config = parse_config(ctx)?;

        let phone = resolve_user_phone(ctx)?;

        tracing::info!(
            "[RESOLVE_RECIPIENT] Resolving recipient for deposit, phone: {}",
            phone
        );

        let fullname = resolve_full_name(ctx);

        // Resolve recipient contact from flow step config.
        let recipient_contact = resolve_recipient_from_config(phone, &config.recipients)?;

        let updates = ContextUpdates {
            session_context_patch: Some(json!({
                "recipient_name": recipient_contact.as_ref().map(|r| r.full_name.clone()).unwrap_or_else(|| fullname.unwrap_or("Unknown").to_string()),
                "recipient_phone": recipient_contact.as_ref().map(|r| r.phone_number.clone()).unwrap_or_else(|| phone.to_string()),
                "recipient_staff_id": recipient_contact.as_ref().map(|r| r.staff_id.clone()).unwrap_or_default(),
                "recipient_full_name": recipient_contact.as_ref().map(|r| r.full_name.clone()).unwrap_or_else(|| "Unknown".to_string()),
                "deposit_amount": ctx.input.get("amount"),
                "deposit_currency": ctx.input.get("currency"),
                "provider": recipient_contact.as_ref().map(|r| r.staff_id.clone())
            })),
            flow_context_patch: Some(json!({
                "provider_matched": recipient_contact.is_some()
            })),
            user_metadata_patch: None,
            user_metadata_eager_patch: None,
            notifications: None,
        };

        Ok(StepOutcome::Done {
            output: Some(json!({
                "recipientContact": recipient_contact.map(|r| json!({
                    "staffId": r.staff_id,
                    "fullName": r.full_name,
                    "phoneNumber": r.phone_number
                })).unwrap_or(serde_json::Value::Null)
            })),
            updates: Some(Box::new(updates)),
        })
    }
}

fn resolve_user_phone(ctx: &StepContext) -> Result<&str, FlowError> {
    ctx.flow_context
        .get("step_output")
        .and_then(|v| v.get("get_user"))
        .and_then(|v| v.get("phoneNumber"))
        .and_then(|v| v.as_str())
        .or_else(|| {
            ctx.session_context
                .get("phone_number")
                .and_then(|v| v.as_str())
        })
        .ok_or_else(|| FlowError::InvalidDefinition("Missing phone number".to_string()))
}

fn resolve_full_name(ctx: &StepContext) -> Option<&str> {
    ctx.session_context
        .get("full_name")
        .or_else(|| ctx.session_context.get("fullname"))
        .and_then(|v| v.as_str())
}

#[instrument(skip(recipients))]
fn resolve_recipient_from_config(
    phone: &str,
    recipients: &[RecipientRegexRule],
) -> Result<Option<DepositRecipientContact>, FlowError> {
    if recipients.is_empty() {
        return Err(FlowError::InvalidDefinition(
            "RESOLVE_RECIPIENT requires config.recipients in flow step definition".to_string(),
        ));
    }

    for rule in recipients {
        let re = regex_match(&rule.regex)?;
        if re.is_match(phone) {
            debug!("Found recipient {:?} for {}", rule.full_name, phone);
            return Ok(Some(DepositRecipientContact {
                staff_id: rule.provider.clone(),
                full_name: rule.full_name.clone(),
                phone_number: rule.phone_number.clone(),
            }));
        }
    }

    debug!("Could not find recipient for {}", phone);

    Ok(None)
}

fn parse_config(ctx: &StepContext) -> Result<ResolveRecipientConfig, FlowError> {
    let config = ctx.services.config.as_ref().ok_or_else(|| {
        FlowError::InvalidDefinition(
            "RESOLVE_RECIPIENT requires step config with recipients".to_string(),
        )
    })?;

    if let Some(recipients) = config.get("recipients") {
        let recipients = serde_json::from_value::<Vec<RecipientRegexRule>>(recipients.clone())
            .map_err(|error| FlowError::InvalidDefinition(error.to_string()))?;
        return Ok(ResolveRecipientConfig { recipients });
    }

    Err(FlowError::InvalidDefinition(
        "RESOLVE_RECIPIENT requires config.recipients in flow step definition".to_string(),
    ))
}

fn regex_match(pattern: &str) -> Result<regex::Regex, FlowError> {
    regex::Regex::new(pattern)
        .map_err(|e| FlowError::InvalidDefinition(format!("Invalid regex: {}", e)))
}

/// Validates deposit amount
pub struct ValidateDepositStep;

#[async_trait]
impl Step for ValidateDepositStep {
    fn step_type(&self) -> &'static str {
        "validate_deposit"
    }
    fn actor(&self) -> Actor {
        Actor::System
    }
    fn human_id(&self) -> &'static str {
        "validate_deposit"
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        tracing::debug!(step = self.step_type(), "Executing step");
        let amount = ctx
            .input
            .get("amount")
            .or_else(|| ctx.session_context.get("deposit_amount"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| FlowError::InvalidDefinition("Missing amount".to_string()))?;

        let currency = ctx
            .input
            .get("currency")
            .or_else(|| ctx.session_context.get("deposit_currency"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| FlowError::InvalidDefinition("Missing currency".to_string()))?;

        tracing::info!(
            "[VALIDATE_DEPOSIT] Validating deposit: {} {}",
            amount,
            currency
        );

        let amount_num: f64 = amount
            .parse()
            .map_err(|_| FlowError::InvalidDefinition("Invalid amount format".to_string()))?;

        let is_valid =
            (1.0..=100000.0).contains(&amount_num) && ["USD", "EUR", "XAF"].contains(&currency);

        if is_valid {
            Ok(StepOutcome::Done {
                output: Some(json!({
                    "valid": true,
                    "amount": amount,
                    "currency": currency
                })),
                updates: None,
            })
        } else {
            Ok(StepOutcome::Failed {
                error: format!("Deposit validation failed: {} {}", amount, currency),
                retryable: false,
            })
        }
    }
}

/// Persists deposit result to user metadata via context updates
#[allow(dead_code)]
pub struct PersistDepositResultStep;

#[async_trait]
impl Step for PersistDepositResultStep {
    fn step_type(&self) -> &'static str {
        "persist_deposit_result"
    }
    fn actor(&self) -> Actor {
        Actor::System
    }
    fn human_id(&self) -> &'static str {
        "persist_deposit_result"
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        tracing::debug!(step = self.step_type(), "Executing step");
        let deposit_result = ctx
            .input
            .get("deposit_result")
            .cloned()
            .unwrap_or_else(|| json!({}));

        tracing::info!("[PERSIST_DEPOSIT_RESULT] Saving deposit result");

        let updates = ContextUpdates {
            user_metadata_patch: Some(json!({
                "deposit_status": "CONFIRMED",
                "deposit_result": deposit_result,
                "deposit_confirmed_at": chrono::Utc::now().to_rfc3339()
            })),
            user_metadata_eager_patch: None,
            session_context_patch: None,
            flow_context_patch: None,
            notifications: None,
        };

        Ok(StepOutcome::Done {
            output: Some(json!({ "persisted": true })),
            updates: Some(Box::new(updates)),
        })
    }
}
