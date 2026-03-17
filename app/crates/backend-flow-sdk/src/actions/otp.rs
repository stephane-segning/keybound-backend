use crate::step::ContextUpdates;
use crate::{Actor, FlowError, Step, StepContext, StepOutcome};
use async_trait::async_trait;
use chrono::Utc;
use rand::RngExt;
use serde::Deserialize;
use serde_json::json;

#[cfg(test)]
use crate::StepServices;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OtpType {
    #[default]
    Numeric,
    Alphanumeric,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SaveTarget {
    #[default]
    Flow,
    Session,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GenerateOtpConfig {
    #[serde(default = "default_length")]
    pub length: u8,

    #[serde(default)]
    pub otp_type: OtpType,

    #[serde(default = "default_expiry")]
    pub expiry_seconds: u64,

    #[serde(default)]
    pub save_to: SaveTarget,
}

fn default_length() -> u8 {
    6
}

fn default_expiry() -> u64 {
    300
}

impl Default for GenerateOtpConfig {
    fn default() -> Self {
        Self {
            length: default_length(),
            otp_type: OtpType::Numeric,
            expiry_seconds: default_expiry(),
            save_to: SaveTarget::Flow,
        }
    }
}

pub struct GenerateOtpAction;

fn generate_otp(length: u8, otp_type: &OtpType) -> String {
    let mut rng = rand::rng();
    match otp_type {
        OtpType::Numeric => {
            let max = 10u64.pow(length as u32);
            let num = rng.random_range(0..max);
            format!("{:0width$}", num, width = length as usize)
        }
        OtpType::Alphanumeric => {
            const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
            (0..length)
                .map(|_| {
                    let idx = rng.random_range(0..CHARSET.len());
                    CHARSET[idx] as char
                })
                .collect()
        }
    }
}

#[async_trait]
impl Step for GenerateOtpAction {
    fn step_type(&self) -> &'static str {
        "GENERATE_OTP"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "generate_otp"
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        let config: GenerateOtpConfig = ctx
            .flow_config("otp_config")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| FlowError::InvalidDefinition(e.to_string()))?
            .unwrap_or_default();

        let otp = generate_otp(config.length, &config.otp_type);
        let expires_at = Utc::now().timestamp() + config.expiry_seconds as i64;

        tracing::info!(
            "[GENERATE_OTP] Generated OTP for session={}, length={}, type={:?}",
            ctx.session_id,
            config.length,
            config.otp_type
        );

        let patch = json!({
            "otp": &otp,
            "otp_expires_at": expires_at
        });

        let updates = match config.save_to {
            SaveTarget::Flow => ContextUpdates {
                flow_context_patch: Some(patch),
                session_context_patch: None,
                user_metadata_patch: None,
                notifications: None,
            },
            SaveTarget::Session => ContextUpdates {
                flow_context_patch: None,
                session_context_patch: Some(patch),
                user_metadata_patch: None,
                notifications: None,
            },
        };

        Ok(StepOutcome::Done {
            output: Some(json!({
                "generated": true,
                "expires_at": expires_at
            })),
            updates: Some(updates),
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct VerifyOtpConfig {
    #[serde(default = "default_input_field")]
    pub input_field: String,

    #[serde(default = "default_stored_field")]
    pub stored_field: String,

    #[serde(default = "default_max_attempts")]
    pub max_attempts: u8,

    #[serde(default = "default_attempts_field")]
    pub attempts_field: String,
}

fn default_input_field() -> String {
    "code".to_string()
}

fn default_stored_field() -> String {
    "otp".to_string()
}

fn default_max_attempts() -> u8 {
    3
}

fn default_attempts_field() -> String {
    "otp_attempts".to_string()
}

impl Default for VerifyOtpConfig {
    fn default() -> Self {
        Self {
            input_field: default_input_field(),
            stored_field: default_stored_field(),
            max_attempts: default_max_attempts(),
            attempts_field: default_attempts_field(),
        }
    }
}

pub struct VerifyOtpAction;

#[async_trait]
impl Step for VerifyOtpAction {
    fn step_type(&self) -> &'static str {
        "VERIFY_OTP"
    }

    fn actor(&self) -> Actor {
        Actor::EndUser
    }

    fn human_id(&self) -> &'static str {
        "verify_otp"
    }

    async fn execute(&self, _ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        Ok(StepOutcome::Waiting {
            actor: Actor::EndUser,
        })
    }

    async fn validate_input(&self, input: &serde_json::Value) -> Result<(), FlowError> {
        let config: VerifyOtpConfig = Default::default();

        if input.get(&config.input_field).is_none() {
            return Err(FlowError::InvalidDefinition(format!(
                "Missing required field: {}",
                config.input_field
            )));
        }

        Ok(())
    }

    async fn verify_input(
        &self,
        ctx: &StepContext,
        input: &serde_json::Value,
    ) -> Result<StepOutcome, FlowError> {
        let config: VerifyOtpConfig = ctx
            .flow_config("otp_config")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| FlowError::InvalidDefinition(e.to_string()))?
            .unwrap_or_default();

        let submitted = input
            .get(&config.input_field)
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                FlowError::InvalidDefinition(format!("Missing {}", config.input_field))
            })?;

        let stored = ctx
            .flow_config(&config.stored_field)
            .or_else(|| ctx.session_config(&config.stored_field))
            .and_then(|v| v.as_str());

        let expires_at = ctx
            .flow_config("otp_expires_at")
            .or_else(|| ctx.session_config("otp_expires_at"))
            .and_then(|v| v.as_i64());

        let attempts_key = &config.attempts_field;
        let current_attempts: u8 = ctx
            .flow_config(attempts_key)
            .or_else(|| ctx.session_config(attempts_key))
            .and_then(|v| v.as_u64())
            .map(|v| v as u8)
            .unwrap_or(0);

        match (stored, expires_at) {
            (Some(stored), Some(expires)) => {
                let now = Utc::now().timestamp();

                if now > expires {
                    tracing::warn!(
                        "[VERIFY_OTP] OTP expired for session={}",
                        ctx.session_id
                    );
                    return Ok(StepOutcome::Failed {
                        error: "OTP_EXPIRED".to_string(),
                        retryable: false,
                    });
                }

                if current_attempts >= config.max_attempts {
                    tracing::warn!(
                        "[VERIFY_OTP] Max attempts exceeded for session={}",
                        ctx.session_id
                    );
                    return Ok(StepOutcome::Failed {
                        error: "MAX_ATTEMPTS_EXCEEDED".to_string(),
                        retryable: false,
                    });
                }

                if stored != submitted {
                    let new_attempts = current_attempts + 1;
                    tracing::warn!(
                        "[VERIFY_OTP] Invalid OTP for session={}, attempts={}/{}",
                        ctx.session_id,
                        new_attempts,
                        config.max_attempts
                    );

                    return Ok(StepOutcome::Done {
                        output: Some(json!({
                            "verified": false,
                            "attempts_remaining": config.max_attempts - new_attempts
                        })),
                        updates: Some(ContextUpdates {
                            flow_context_patch: Some(json!({
                                attempts_key: new_attempts
                            })),
                            ..Default::default()
                        }),
                    });
                }

                tracing::info!(
                    "[VERIFY_OTP] OTP verified successfully for session={}",
                    ctx.session_id
                );

                Ok(StepOutcome::Done {
                    output: Some(json!({ "verified": true })),
                    updates: Some(ContextUpdates {
                        flow_context_patch: Some(json!({
                            "otp": null,
                            "otp_expires_at": null,
                            attempts_key: null
                        })),
                        ..Default::default()
                    }),
                })
            }
            _ => {
                tracing::warn!(
                    "[VERIFY_OTP] No OTP found in context for session={}",
                    ctx.session_id
                );
                Ok(StepOutcome::Failed {
                    error: "NO_OTP_FOUND".to_string(),
                    retryable: false,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_ctx(flow_context: serde_json::Value) -> StepContext {
        StepContext {
            session_id: "test".to_string(),
            flow_id: "test-flow".to_string(),
            step_id: "otp-step".to_string(),
            input: json!({}),
            session_context: json!({}),
            flow_context,
            services: StepServices::default(),
        }
    }

    #[tokio::test]
    async fn generate_otp_creates_numeric_code() {
        let action = GenerateOtpAction;
        let ctx = make_ctx(json!({
            "otp_config": {
                "length": 6,
                "otp_type": "numeric",
                "expiry_seconds": 300
            }
        }));

        let result = action.execute(&ctx).await.unwrap();

        match result {
            StepOutcome::Done { output, updates } => {
                let output = output.unwrap();
                assert_eq!(output["generated"], true);

                let updates = updates.unwrap();
                let patch = updates.flow_context_patch.unwrap();
                assert!(patch["otp"].is_string());
                assert_eq!(patch["otp"].as_str().unwrap().len(), 6);
                assert!(patch["otp_expires_at"].is_i64());
            }
            _ => panic!("Expected Done outcome"),
        }
    }

    #[tokio::test]
    async fn generate_otp_uses_defaults() {
        let action = GenerateOtpAction;
        let ctx = make_ctx(json!({}));

        let result = action.execute(&ctx).await.unwrap();

        match result {
            StepOutcome::Done { updates, .. } => {
                let updates = updates.unwrap();
                let patch = updates.flow_context_patch.unwrap();
                assert_eq!(patch["otp"].as_str().unwrap().len(), 6);
            }
            _ => panic!("Expected Done outcome"),
        }
    }

    #[tokio::test]
    async fn verify_otp_returns_waiting() {
        let action = VerifyOtpAction;
        let ctx = make_ctx(json!({}));

        let result = action.execute(&ctx).await.unwrap();

        match result {
            StepOutcome::Waiting { actor } => {
                assert_eq!(actor, Actor::EndUser);
            }
            _ => panic!("Expected Waiting outcome"),
        }
    }

    #[tokio::test]
    async fn verify_otp_validates_correct_code() {
        let action = VerifyOtpAction;
        let ctx = make_ctx(json!({
            "otp": "123456",
            "otp_expires_at": Utc::now().timestamp() + 300
        }));

        let input = json!({ "code": "123456" });

        let result = action.verify_input(&ctx, &input).await.unwrap();

        match result {
            StepOutcome::Done { output, updates } => {
                let output = output.unwrap();
                assert_eq!(output["verified"], true);

                let updates = updates.unwrap();
                let patch = updates.flow_context_patch.unwrap();
                assert!(patch["otp"].is_null());
            }
            _ => panic!("Expected Done outcome with verified=true"),
        }
    }

    #[tokio::test]
    async fn verify_otp_rejects_wrong_code() {
        let action = VerifyOtpAction;
        let ctx = make_ctx(json!({
            "otp": "123456",
            "otp_expires_at": Utc::now().timestamp() + 300
        }));

        let input = json!({ "code": "654321" });

        let result = action.verify_input(&ctx, &input).await.unwrap();

        match result {
            StepOutcome::Done { output, updates } => {
                let output = output.unwrap();
                assert_eq!(output["verified"], false);
                assert_eq!(output["attempts_remaining"], 2);

                let updates = updates.unwrap();
                let patch = updates.flow_context_patch.unwrap();
                assert_eq!(patch["otp_attempts"], 1);
            }
            _ => panic!("Expected Done outcome with verified=false"),
        }
    }

    #[tokio::test]
    async fn verify_otp_fails_expired() {
        let action = VerifyOtpAction;
        let ctx = make_ctx(json!({
            "otp": "123456",
            "otp_expires_at": Utc::now().timestamp() - 100
        }));

        let input = json!({ "code": "123456" });

        let result = action.verify_input(&ctx, &input).await.unwrap();

        match result {
            StepOutcome::Failed { error, retryable } => {
                assert_eq!(error, "OTP_EXPIRED");
                assert!(!retryable);
            }
            _ => panic!("Expected Failed outcome"),
        }
    }
}