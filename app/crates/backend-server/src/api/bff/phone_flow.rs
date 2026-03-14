use super::shared::{
    OTP_RATE_LIMIT_MAX_ISSUES, OTP_RATE_LIMIT_WINDOW_MINUTES, OTP_STEP_TYPE,
    OTP_VERIFY_ATTEMPT_STEP, ensure_step_registered, ensure_user_match, normalized_user_id,
    parse_step_status, parse_step_type, rate_limited_error, split_step_id, step_id,
    upsert_step_id_in_context, user_id_matches,
};
use crate::state_machine::engine::Engine;
use crate::state_machine::secrets::verify_secret;
use crate::state_machine::types::{
    ATTEMPT_STATUS_FAILED, ActorType, INSTANCE_STATUS_COMPLETED, KIND_KYC_PHONE_OTP,
    STEP_PHONE_ISSUE_OTP, STEP_PHONE_VERIFY_OTP,
};
use backend_auth::JwtToken;
use backend_core::Error;
use backend_repository::{SmStepAttemptCreateInput, SmStepAttemptPatch};
use chrono::{Duration, Utc};
use gen_oas_server_bff::apis::phone_otp::{
    InternalCreatePhoneOtpStepResponse, InternalIssuePhoneOtpChallengeResponse,
    InternalVerifyPhoneOtpChallengeResponse,
};
use gen_oas_server_bff::models;
use serde_json::{Value, json};
use tracing::{info, instrument};

use super::super::BackendApi;

impl BackendApi {
    #[instrument(skip(self))]
    pub async fn create_phone_otp_step_flow(
        &self,
        claims: &JwtToken,
        body: &models::CreateCaseStepRequest,
    ) -> Result<InternalCreatePhoneOtpStepResponse, Error> {
        info!("Handling create_phone_otp_step_flow called");
        ensure_user_match(claims, &body.user_id)?;
        let user_id = normalized_user_id(&body.user_id);

        let session = self
            .state
            .sm
            .get_instance(&body.session_id)
            .await?
            .ok_or_else(|| Error::not_found("SESSION_NOT_FOUND", "Session not found"))?;

        // Validate session is correct type for phone OTP operations
        if session.kind != KIND_KYC_PHONE_OTP {
            return Err(Error::bad_request(
                "INVALID_SESSION_KIND",
                "Unsupported session kind for phone OTP",
            ));
        }
        if !user_id_matches(session.user_id.as_deref(), &user_id) {
            return Err(Error::unauthorized(
                "Session does not belong to authenticated user",
            ));
        }

        let id = step_id(&session.id, OTP_STEP_TYPE);
        let mut context = session.context;
        let changed = upsert_step_id_in_context(&mut context, &id);
        if changed {
            self.state
                .sm
                .update_instance_context(&session.id, context.clone())
                .await?;
        }

        let attempts = self.state.sm.list_step_attempts(&session.id).await?;
        let status = parse_step_status(
            &session.kind,
            &session.status,
            OTP_STEP_TYPE,
            &attempts,
            &context,
        );

        Ok(InternalCreatePhoneOtpStepResponse::Status201_StepCreated(
            models::KycStep {
                id,
                session_id: session.id,
                user_id,
                r_type: parse_step_type(OTP_STEP_TYPE)?,
                status,
                data: None,
                policy: body.policy.clone(),
                created_at: session.created_at,
                updated_at: session.updated_at,
            },
        ))
    }

    #[instrument(skip(self))]
    pub async fn issue_phone_otp_challenge_flow(
        &self,
        claims: &JwtToken,
        body: &models::IssuePhoneOtpRequest,
    ) -> Result<InternalIssuePhoneOtpChallengeResponse, Error> {
        let (step_session_id, step_type) = split_step_id(&body.step_id)
            .ok_or_else(|| Error::bad_request("INVALID_STEP_ID", "Step id format is invalid"))?;
        // Verify the step is a phone OTP type
        if step_type != OTP_STEP_TYPE {
            return Err(Error::bad_request(
                "INVALID_STEP",
                "Expected PHONE_OTP step",
            ));
        }
        // Ensure step ownership matches the session
        if body.session_id != step_session_id {
            return Err(Error::bad_request(
                "INVALID_STEP",
                "stepId must belong to provided sessionId",
            ));
        }

        let user_id = BackendApi::require_user_id(claims)?;
        let session = self
            .state
            .sm
            .get_instance(&body.session_id)
            .await?
            .ok_or_else(|| Error::not_found("SESSION_NOT_FOUND", "Session not found"))?;
        if session.kind != KIND_KYC_PHONE_OTP {
            return Err(Error::bad_request(
                "INVALID_SESSION_KIND",
                "Unsupported session kind for phone OTP",
            ));
        }
        if !user_id_matches(session.user_id.as_deref(), &user_id) {
            return Err(Error::unauthorized(
                "Step does not belong to authenticated user",
            ));
        }
        ensure_step_registered(&session.context, &body.step_id)?;

        let since = Utc::now() - Duration::minutes(OTP_RATE_LIMIT_WINDOW_MINUTES);
        let attempts = self.state.sm.list_step_attempts(&session.id).await?;
        let recent = attempts
            .iter()
            .filter(|attempt| attempt.step_name == STEP_PHONE_ISSUE_OTP)
            .filter(|attempt| attempt.queued_at.map(|ts| ts >= since).unwrap_or(false))
            .count() as i64;
        if recent >= OTP_RATE_LIMIT_MAX_ISSUES {
            return Err(rate_limited_error(
                "OTP_RATE_LIMITED",
                "Too many OTP issuance requests",
            ));
        }

        let ttl_seconds = body.ttl_seconds.unwrap_or(300) as i64;
        let channel = body
            .channel
            .unwrap_or(models::IssueOtpRequestChannel::Sms)
            .to_string();

        let engine = Engine::new(self.state.clone());
        let (otp_ref, expires_at, tries_left) = engine
            .enqueue_phone_issue_otp(&session.id, &body.msisdn, &channel, ttl_seconds)
            .await?;

        Ok(
            InternalIssuePhoneOtpChallengeResponse::Status200_OTPChallenge(
                models::OtpChallengeInternal::new(
                    otp_ref,
                    expires_at,
                    u32::try_from(tries_left).unwrap_or(0),
                ),
            ),
        )
    }

    #[instrument(skip(self))]
    pub async fn verify_phone_otp_challenge_flow(
        &self,
        claims: &JwtToken,
        body: &models::VerifyPhoneOtpRequest,
    ) -> Result<InternalVerifyPhoneOtpChallengeResponse, Error> {
        let (step_session_id, step_type) = split_step_id(&body.step_id)
            .ok_or_else(|| Error::bad_request("INVALID_STEP_ID", "Step id format is invalid"))?;
        // Verify the step is a phone OTP type
        if step_type != OTP_STEP_TYPE {
            return Err(Error::bad_request(
                "INVALID_STEP",
                "Expected PHONE_OTP step",
            ));
        }
        // Ensure step ownership matches the session
        if body.session_id != step_session_id {
            return Err(Error::bad_request(
                "INVALID_STEP",
                "stepId must belong to provided sessionId",
            ));
        }

        let user_id = BackendApi::require_user_id(claims)?;
        let session = self
            .state
            .sm
            .get_instance(&body.session_id)
            .await?
            .ok_or_else(|| Error::not_found("SESSION_NOT_FOUND", "Session not found"))?;
        if session.kind != KIND_KYC_PHONE_OTP {
            return Err(Error::bad_request(
                "INVALID_SESSION_KIND",
                "Unsupported session kind for phone OTP",
            ));
        }
        if !user_id_matches(session.user_id.as_deref(), &user_id) {
            return Err(Error::unauthorized(
                "Step does not belong to authenticated user",
            ));
        }
        ensure_step_registered(&session.context, &body.step_id)?;

        let attempt = self
            .state
            .sm
            .get_step_attempt_by_external_ref(&session.id, STEP_PHONE_ISSUE_OTP, &body.otp_ref)
            .await?;

        let Some(attempt) = attempt else {
            return Ok(
                InternalVerifyPhoneOtpChallengeResponse::Status200_VerificationOutcome(
                    models::VerifyOutcome::new(
                        false,
                        models::VerifyOutcomeReason::Invalid,
                        models::KycStatus::Failed,
                    ),
                ),
            );
        };

        let output = attempt.output.unwrap_or(Value::Null);
        let expires_at = output
            .get("expires_at")
            .and_then(Value::as_str)
            .and_then(|raw| chrono::DateTime::parse_from_rfc3339(raw).ok())
            .map(|parsed| parsed.with_timezone(&Utc))
            .unwrap_or_else(|| Utc::now() - Duration::seconds(1));
        if expires_at < Utc::now() {
            return Ok(
                InternalVerifyPhoneOtpChallengeResponse::Status200_VerificationOutcome(
                    models::VerifyOutcome::new(
                        false,
                        models::VerifyOutcomeReason::Expired,
                        models::KycStatus::Failed,
                    ),
                ),
            );
        }

        let tries_left = output
            .get("tries_left")
            .and_then(Value::as_i64)
            .unwrap_or(0) as i32;
        if tries_left <= 0 {
            return Ok(
                InternalVerifyPhoneOtpChallengeResponse::Status200_VerificationOutcome(
                    models::VerifyOutcome::new(
                        false,
                        models::VerifyOutcomeReason::Locked,
                        models::KycStatus::Failed,
                    ),
                ),
            );
        }

        let otp_hash = output
            .get("otp_hash")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if otp_hash.is_empty() {
            return Ok(
                InternalVerifyPhoneOtpChallengeResponse::Status200_VerificationOutcome(
                    models::VerifyOutcome::new(
                        false,
                        models::VerifyOutcomeReason::Invalid,
                        models::KycStatus::InProgress,
                    ),
                ),
            );
        }

        if verify_secret(&body.code, otp_hash)? {
            let engine = Engine::new(self.state.clone());
            engine
                .emit_event(
                    &session.id,
                    "OTP_VERIFIED",
                    ActorType::User,
                    Some(user_id.clone()),
                    json!({"otp_ref": body.otp_ref, "step_id": body.step_id}),
                )
                .await?;
            let _ = engine
                .mark_manual_step_succeeded(&session.id, STEP_PHONE_VERIFY_OTP)
                .await;
            self.state
                .sm
                .update_instance_status(&session.id, INSTANCE_STATUS_COMPLETED, Some(Utc::now()))
                .await?;

            return Ok(
                InternalVerifyPhoneOtpChallengeResponse::Status200_VerificationOutcome(
                    models::VerifyOutcome::new(
                        true,
                        models::VerifyOutcomeReason::Verified,
                        models::KycStatus::Verified,
                    ),
                ),
            );
        }

        let verify_attempt_no = self
            .state
            .sm
            .next_attempt_no(&session.id, OTP_VERIFY_ATTEMPT_STEP)
            .await?;
        let finished_at = Utc::now();
        self.state
            .sm
            .create_step_attempt(SmStepAttemptCreateInput {
                id: backend_id::sm_attempt_id()?,
                instance_id: session.id.clone(),
                step_name: OTP_VERIFY_ATTEMPT_STEP.to_owned(),
                attempt_no: verify_attempt_no,
                status: ATTEMPT_STATUS_FAILED.to_owned(),
                external_ref: Some(body.otp_ref.clone()),
                input: json!({
                    "otp_ref": body.otp_ref,
                    "code_len": body.code.len()
                }),
                output: Some(json!({"reason": "INVALID"})),
                error: Some(json!({"error_key": "OTP_INVALID"})),
                queued_at: None,
                started_at: Some(finished_at),
                finished_at: Some(finished_at),
                next_retry_at: None,
            })
            .await?;

        let mut updated_output = output.clone();
        if let Some(output_obj) = updated_output.as_object_mut() {
            output_obj.insert(
                "tries_left".to_owned(),
                Value::Number(serde_json::Number::from((tries_left - 1).max(0))),
            );
        }
        self.state
            .sm
            .patch_step_attempt(
                &attempt.id,
                SmStepAttemptPatch {
                    status: None,
                    output: Some(Some(updated_output)),
                    error: None,
                    queued_at: None,
                    started_at: None,
                    finished_at: None,
                    next_retry_at: None,
                },
            )
            .await?;

        if tries_left - 1 <= 0 {
            let mut context = session.context;
            if let Some(obj) = context.as_object_mut() {
                obj.insert("phone_locked".to_owned(), Value::Bool(true));
            }
            self.state
                .sm
                .update_instance_context(&session.id, context)
                .await?;

            return Ok(
                InternalVerifyPhoneOtpChallengeResponse::Status200_VerificationOutcome(
                    models::VerifyOutcome::new(
                        false,
                        models::VerifyOutcomeReason::Locked,
                        models::KycStatus::Failed,
                    ),
                ),
            );
        }

        Ok(
            InternalVerifyPhoneOtpChallengeResponse::Status200_VerificationOutcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::Invalid,
                    models::KycStatus::InProgress,
                ),
            ),
        )
    }
}
