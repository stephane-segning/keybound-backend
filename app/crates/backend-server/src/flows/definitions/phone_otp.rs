use async_trait::async_trait;
use backend_flow_sdk::flow::StepRef;
use backend_flow_sdk::{Actor, ContextUpdates, FlowError, Step, StepContext, StepOutcome};
use serde_json::{Value, json};
use std::sync::Arc;
use tracing::{debug, info, instrument, warn};

pub fn steps() -> Vec<StepRef> {
    vec![Arc::new(IssuePhoneOtpStep), Arc::new(VerifyPhoneOtpStep)]
}

pub struct IssuePhoneOtpStep;

fn generate_otp() -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    format!("{:06}", rng.random_range(0..1000000))
}

#[async_trait]
impl Step for IssuePhoneOtpStep {
    fn step_type(&self) -> &'static str {
        "ISSUE_PHONE_OTP"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "issue"
    }

    fn feature(&self) -> Option<&'static str> {
        Some("flow-phone-otp")
    }

    #[instrument(skip(self, ctx))]
    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        debug!("Executing IssuePhoneOtpStep");

        let phone_number = ctx
            .flow_config("phoneNumber")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                FlowError::InvalidDefinition("phoneNumber not found in flow context".to_owned())
            })?;

        let otp = generate_otp();
        info!("Generated OTP for {}: {}", phone_number, otp);

        let notification = json!({
            "type": "OTP",
            "step_id": ctx.step_id,
            "msisdn": phone_number,
            "otp": otp
        });

        Ok(StepOutcome::Done {
            output: Some(json!({"otpSent": true, "phoneNumber": phone_number})),
            updates: Some(Box::new(ContextUpdates {
                flow_context_patch: Some(json!({
                    "otp": otp,
                    "otpExpiresAt": chrono::Utc::now().timestamp() + 300
                })),
                notifications: Some(vec![notification]),
                ..Default::default()
            })),
        })
    }
}

pub struct VerifyPhoneOtpStep;

#[async_trait]
impl Step for VerifyPhoneOtpStep {
    fn step_type(&self) -> &'static str {
        "VERIFY_PHONE_OTP"
    }

    fn actor(&self) -> Actor {
        Actor::EndUser
    }

    fn human_id(&self) -> &'static str {
        "verify"
    }

    fn feature(&self) -> Option<&'static str> {
        Some("flow-phone-otp")
    }

    #[instrument(skip(self, _ctx))]
    async fn execute(&self, _ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        debug!("Executing VerifyPhoneOtpStep (waiting for user)");
        Ok(StepOutcome::Waiting {
            actor: Actor::EndUser,
        })
    }

    async fn validate_input(&self, input: &Value) -> Result<(), FlowError> {
        let otp = input.get("otpCode").and_then(|v| v.as_str());
        if otp.is_none() {
            return Err(FlowError::InvalidDefinition(
                "otpCode is required".to_owned(),
            ));
        }
        Ok(())
    }

    async fn verify_input(
        &self,
        ctx: &StepContext,
        input: &Value,
    ) -> Result<StepOutcome, FlowError> {
        let submitted_otp = input
            .get("otpCode")
            .and_then(|v| v.as_str())
            .ok_or_else(|| FlowError::InvalidDefinition("otpCode is required".to_owned()))?;

        let stored_otp = ctx.flow_config("otp").and_then(|v| v.as_str());
        let expires_at = ctx.flow_config("otpExpiresAt").and_then(|v| v.as_i64());

        match (stored_otp, expires_at) {
            (Some(stored), Some(expires)) => {
                let now = chrono::Utc::now().timestamp();
                if now > expires {
                    warn!("OTP expired for step {}", ctx.step_id);
                    return Ok(StepOutcome::Failed {
                        error: "OTP_EXPIRED".to_owned(),
                        retryable: false,
                    });
                }

                if stored != submitted_otp {
                    warn!(
                        "Invalid OTP for step {}: submitted={}, stored={}",
                        ctx.step_id, submitted_otp, stored
                    );
                    return Ok(StepOutcome::Failed {
                        error: "INVALID_OTP".to_owned(),
                        retryable: true,
                    });
                }

                info!("OTP verified successfully for step {}", ctx.step_id);
                Ok(StepOutcome::Done {
                    output: Some(json!({"verified": true})),
                    updates: Some(Box::new(ContextUpdates {
                        flow_context_patch: Some(json!({
                            "otp": null,
                            "otpExpiresAt": null,
                            "phoneVerified": true
                        })),
                        ..Default::default()
                    })),
                })
            }
            _ => {
                warn!("No OTP found in flow context for step {}", ctx.step_id);
                Ok(StepOutcome::Failed {
                    error: "NO_OTP_FOUND".to_owned(),
                    retryable: false,
                })
            }
        }
    }
}
