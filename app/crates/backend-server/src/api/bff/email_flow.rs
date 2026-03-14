use super::shared::{
    MAGIC_ISSUE_STEP, MAGIC_RATE_LIMIT_MAX_ISSUES, MAGIC_RATE_LIMIT_WINDOW_MINUTES,
    MAGIC_STEP_TYPE, ensure_step_registered, ensure_user_match, normalized_user_id,
    parse_step_status, parse_step_type, rate_limited_error, split_step_id, step_id,
    upsert_step_id_in_context, user_id_matches,
};
use crate::state_machine::secrets::{hash_secret, verify_secret};
use crate::state_machine::types::{ActorType, INSTANCE_STATUS_COMPLETED, KIND_KYC_EMAIL_MAGIC};
use crate::worker::NotificationJob;
use backend_auth::JwtToken;
use backend_core::Error;
use backend_repository::SmStepAttemptCreateInput;
use chrono::{Duration, Utc};
use gen_oas_server_bff::apis::email_magic::{
    InternalCreateEmailMagicStepResponse, InternalIssueMagicEmailChallengeResponse,
    InternalVerifyMagicEmailChallengeResponse,
};
use gen_oas_server_bff::models;
use serde_json::{Value, json};
use tracing::instrument;

use super::super::BackendApi;

impl BackendApi {
    #[instrument(skip(self))]
    pub async fn create_email_magic_step_flow(
        &self,
        claims: &JwtToken,
        body: &models::CreateCaseStepRequest,
    ) -> Result<InternalCreateEmailMagicStepResponse, Error> {
        ensure_user_match(claims, &body.user_id)?;
        let user_id = normalized_user_id(&body.user_id);

        let session = self
            .state
            .sm
            .get_instance(&body.session_id)
            .await?
            .ok_or_else(|| Error::not_found("SESSION_NOT_FOUND", "Session not found"))?;

        if session.kind != KIND_KYC_EMAIL_MAGIC {
            return Err(Error::bad_request(
                "INVALID_SESSION_KIND",
                "Unsupported session kind for email magic",
            ));
        }
        if !user_id_matches(session.user_id.as_deref(), &user_id) {
            return Err(Error::unauthorized(
                "Session does not belong to authenticated user",
            ));
        }

        let id = step_id(&session.id, MAGIC_STEP_TYPE);
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
            MAGIC_STEP_TYPE,
            &attempts,
            &context,
        );

        Ok(InternalCreateEmailMagicStepResponse::Status201_StepCreated(
            models::KycStep {
                id,
                session_id: session.id,
                user_id,
                r_type: parse_step_type(MAGIC_STEP_TYPE)?,
                status,
                data: None,
                policy: body.policy.clone(),
                created_at: session.created_at,
                updated_at: session.updated_at,
            },
        ))
    }

    #[instrument(skip(self))]
    pub async fn issue_magic_email_challenge_flow(
        &self,
        claims: &JwtToken,
        body: &models::IssueMagicEmailRequest,
    ) -> Result<InternalIssueMagicEmailChallengeResponse, Error> {
        let (step_session_id, step_type) = split_step_id(&body.step_id)
            .ok_or_else(|| Error::bad_request("INVALID_STEP_ID", "Step id format is invalid"))?;
        // Verify the step is an email magic type
        if step_type != MAGIC_STEP_TYPE {
            return Err(Error::bad_request(
                "INVALID_STEP",
                "Expected EMAIL_MAGIC step",
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
        if session.kind != KIND_KYC_EMAIL_MAGIC {
            return Err(Error::bad_request(
                "INVALID_SESSION_KIND",
                "Unsupported session kind for email magic",
            ));
        }
        if !user_id_matches(session.user_id.as_deref(), &user_id) {
            return Err(Error::unauthorized(
                "Step does not belong to authenticated user",
            ));
        }
        ensure_step_registered(&session.context, &body.step_id)?;

        let since = Utc::now() - Duration::minutes(MAGIC_RATE_LIMIT_WINDOW_MINUTES);
        let attempts = self.state.sm.list_step_attempts(&session.id).await?;
        let recent = attempts
            .iter()
            .filter(|attempt| attempt.step_name == MAGIC_ISSUE_STEP)
            .filter(|attempt| attempt.queued_at.map(|ts| ts >= since).unwrap_or(false))
            .count() as i64;
        if recent >= MAGIC_RATE_LIMIT_MAX_ISSUES {
            return Err(rate_limited_error(
                "MAGIC_RATE_LIMITED",
                "Too many magic-link issuance requests",
            ));
        }

        let ttl_seconds = body.ttl_seconds.unwrap_or(300) as i64;
        let expires_at = Utc::now() + Duration::seconds(ttl_seconds.clamp(60, 86400));

        let token_ref = backend_id::kyc_magic_ref()?;
        let secret = format!("{:032x}", rand::random::<u128>());
        let token_hash = hash_secret(&secret)?;
        let token = format!("{token_ref}.{secret}");

        let attempt_no = self
            .state
            .sm
            .next_attempt_no(&session.id, MAGIC_ISSUE_STEP)
            .await?;
        let now = Utc::now();
        self.state
            .sm
            .create_step_attempt(SmStepAttemptCreateInput {
                id: backend_id::sm_attempt_id()?,
                instance_id: session.id.clone(),
                step_name: MAGIC_ISSUE_STEP.to_owned(),
                attempt_no,
                status: "SUCCEEDED".to_owned(),
                external_ref: Some(token_ref.clone()),
                input: json!({"email": body.email.clone(), "ttl_seconds": ttl_seconds}),
                output: Some(
                    json!({"token_ref": token_ref, "expires_at": expires_at, "token_hash": token_hash}),
                ),
                error: None,
                queued_at: Some(now),
                started_at: Some(now),
                finished_at: Some(now),
                next_retry_at: None,
            })
            .await?;

        let _ = self
            .state
            .notification_queue
            .enqueue(NotificationJob::MagicEmail {
                step_id: body.step_id.clone(),
                email: body.email.clone(),
                token,
            })
            .await;

        Ok(
            InternalIssueMagicEmailChallengeResponse::Status200_MagicEmailChallenge(
                models::MagicEmailChallengeInternal::new(token_ref, expires_at),
            ),
        )
    }

    #[instrument(skip(self))]
    pub async fn verify_magic_email_challenge_flow(
        &self,
        claims: &JwtToken,
        body: &models::VerifyMagicEmailRequest,
    ) -> Result<InternalVerifyMagicEmailChallengeResponse, Error> {
        let (step_session_id, step_type) = split_step_id(&body.step_id)
            .ok_or_else(|| Error::bad_request("INVALID_STEP_ID", "Step id format is invalid"))?;
        if step_type != MAGIC_STEP_TYPE {
            return Err(Error::bad_request(
                "INVALID_STEP",
                "Expected EMAIL_MAGIC step",
            ));
        }
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
        if session.kind != KIND_KYC_EMAIL_MAGIC {
            return Err(Error::bad_request(
                "INVALID_SESSION_KIND",
                "Unsupported session kind for email magic",
            ));
        }
        if !user_id_matches(session.user_id.as_deref(), &user_id) {
            return Err(Error::unauthorized(
                "Step does not belong to authenticated user",
            ));
        }
        ensure_step_registered(&session.context, &body.step_id)?;

        let Some((token_ref, secret)) = body.token.split_once('.') else {
            return Ok(
                InternalVerifyMagicEmailChallengeResponse::Status200_VerificationOutcome(
                    models::VerifyOutcome::new(
                        false,
                        models::VerifyOutcomeReason::Invalid,
                        models::KycStatus::Failed,
                    ),
                ),
            );
        };

        let attempt = self
            .state
            .sm
            .get_step_attempt_by_external_ref(&session.id, MAGIC_ISSUE_STEP, token_ref)
            .await?;
        let Some(attempt) = attempt else {
            return Ok(
                InternalVerifyMagicEmailChallengeResponse::Status200_VerificationOutcome(
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
                InternalVerifyMagicEmailChallengeResponse::Status200_VerificationOutcome(
                    models::VerifyOutcome::new(
                        false,
                        models::VerifyOutcomeReason::Expired,
                        models::KycStatus::Failed,
                    ),
                ),
            );
        }

        let token_hash = output
            .get("token_hash")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if token_hash.is_empty() {
            return Ok(
                InternalVerifyMagicEmailChallengeResponse::Status200_VerificationOutcome(
                    models::VerifyOutcome::new(
                        false,
                        models::VerifyOutcomeReason::Invalid,
                        models::KycStatus::Failed,
                    ),
                ),
            );
        }

        if verify_secret(secret, token_hash)? {
            let engine = crate::state_machine::engine::Engine::new(self.state.clone());
            engine
                .emit_event(
                    &session.id,
                    "MAGIC_EMAIL_VERIFIED",
                    ActorType::User,
                    Some(user_id),
                    json!({"token_ref": token_ref, "step_id": body.step_id}),
                )
                .await?;
            self.state
                .sm
                .update_instance_status(&session.id, INSTANCE_STATUS_COMPLETED, Some(Utc::now()))
                .await?;

            return Ok(
                InternalVerifyMagicEmailChallengeResponse::Status200_VerificationOutcome(
                    models::VerifyOutcome::new(
                        true,
                        models::VerifyOutcomeReason::Verified,
                        models::KycStatus::Verified,
                    ),
                ),
            );
        }

        Ok(
            InternalVerifyMagicEmailChallengeResponse::Status200_VerificationOutcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::Invalid,
                    models::KycStatus::Failed,
                ),
            ),
        )
    }
}
