use async_trait::async_trait;
use backend_flow_sdk::step::ContextUpdates;
use backend_flow_sdk::{Actor, FlowError, Step, StepContext, StepOutcome};
use serde_json::json;

/// Internal contact representation for deposit recipients
#[derive(Debug, Clone)]
pub struct DepositRecipientContact {
    pub staff_id: String,
    pub full_name: String,
    pub phone_number: String,
}

/// Static recipient configuration loaded from YAML
/// Matches config/default.yaml deposit_flow.staff.recipients
fn get_static_recipients() -> Vec<(String, String, String, String)> {
    vec![
        (
            "MTN_CM".to_string(),
            "Mbarga Benn".to_string(),
            "+237690000111".to_string(),
            r"^\+23769[0-4][0-9]{6}$".to_string(),
        ),
        (
            "ORANGE_CM".to_string(),
            "Nkoumou Linda".to_string(),
            "+237699000222".to_string(),
            r"^\+23769[5-9][0-9]{6}$".to_string(),
        ),
    ]
}

/// Checks if user exists for deposit and resolves recipient contact
pub struct CheckUserExistsStep;

#[async_trait]
impl Step for CheckUserExistsStep {
    fn step_type(&self) -> &'static str {
        "check_user_exists"
    }
    fn actor(&self) -> Actor {
        Actor::System
    }
    fn human_id(&self) -> &'static str {
        "check_user_exists"
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        let phone = ctx
            .input
            .get("phone_number")
            .or_else(|| ctx.session_context.get("phone_number"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| FlowError::InvalidDefinition("Missing phone_number".to_string()))?;

        tracing::info!(
            "[CHECK_USER_EXISTS] Checking user for deposit, phone: {}",
            phone
        );

        // Resolve user from context (set by previous flow steps or session creation)
        let user_id = ctx.session_context.get("userId").and_then(|v| v.as_str());
        let fullname = ctx.session_context.get("fullname").and_then(|v| v.as_str());

        let (user_exists, user_info) = if let Some(uid) = user_id {
            (
                true,
                json!({
                    "userId": uid,
                    "fullname": fullname.unwrap_or("User"),
                    "phoneNumber": phone
                }),
            )
        } else {
            (
                false,
                json!({
                    "userId": serde_json::Value::Null,
                    "fullname": serde_json::Value::Null,
                    "phoneNumber": phone
                }),
            )
        };

        // Resolve recipient contact from static config
        let recipient_contact = resolve_recipient_from_config(phone)?;

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
                "user_exists": user_exists,
                "provider_matched": recipient_contact.is_some()
            })),
            user_metadata_patch: None,
            notifications: None,
        };

        Ok(StepOutcome::Done {
            output: Some(json!({
                "userExists": user_exists,
                "userInfo": user_info,
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

fn resolve_recipient_from_config(
    phone: &str,
) -> Result<Option<DepositRecipientContact>, FlowError> {
    for (provider, full_name, phone_number, regex_pattern) in get_static_recipients() {
        let re = regex_match(&regex_pattern)?;
        if re.is_match(phone) {
            return Ok(Some(DepositRecipientContact {
                staff_id: provider,
                full_name,
                phone_number,
            }));
        }
    }

    Ok(None)
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
