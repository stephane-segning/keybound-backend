use super::BackendApi;
use crate::file_storage::EncryptionMode;
use crate::state_machine::engine::Engine;
use crate::state_machine::secrets::{hash_secret, verify_secret};
use crate::state_machine::types::*;
use crate::worker::NotificationJob;
use axum_extra::extract::CookieJar;
use backend_auth::JwtToken;
use backend_core::Error;
use backend_repository::{SmStepAttemptCreateInput, SmStepAttemptPatch};
use chrono::{Duration, Utc};
use gen_oas_server_bff::apis::deposits::{
    Deposits, InternalCreatePhoneDepositResponse, InternalGetPhoneDepositResponse,
};
use gen_oas_server_bff::apis::notifications::{
    InternalIssueMagicEmailResponse, InternalIssueOtpResponse, InternalVerifyMagicEmailResponse,
    InternalVerifyOtpResponse, Notifications,
};
use gen_oas_server_bff::apis::steps::{
    InternalCreateStepResponse, InternalGetStepResponse, InternalStartSessionResponse, Steps,
};
use gen_oas_server_bff::apis::uploads::{
    InternalCompleteUploadResponse, InternalPresignUploadResponse, Uploads,
};
use gen_oas_server_bff::models;
use headers::Host;
use http::Method;
use serde_json::{json, Value};

const OTP_RATE_LIMIT_WINDOW_MINUTES: i64 = 10;
const OTP_RATE_LIMIT_MAX_ISSUES: i64 = 5;

const MAGIC_RATE_LIMIT_WINDOW_MINUTES: i64 = 10;
const MAGIC_RATE_LIMIT_MAX_ISSUES: i64 = 5;

const OTP_STEP_TYPE: &str = "PHONE";
const MAGIC_STEP_TYPE: &str = "EMAIL";

fn step_id(session_id: &str, step_type: &str) -> String {
    format!("{session_id}__{step_type}")
}

fn split_step_id(id: &str) -> Option<(String, String)> {
    let (session_id, step_type) = id.rsplit_once("__")?;
    Some((session_id.to_owned(), step_type.to_owned()))
}

fn parse_session_status(instance_status: &str) -> models::KycSessionInternalStatus {
    match instance_status {
        INSTANCE_STATUS_COMPLETED => models::KycSessionInternalStatus::Completed,
        INSTANCE_STATUS_FAILED | INSTANCE_STATUS_CANCELLED => {
            models::KycSessionInternalStatus::Locked
        }
        _ => models::KycSessionInternalStatus::Open,
    }
}

fn parse_step_status(
    kind: &str,
    instance_status: &str,
    step_type: &str,
    attempts: &[backend_model::db::SmStepAttemptRow],
    context: &Value,
) -> models::KycStatus {
    if kind == KIND_KYC_PHONE_OTP && step_type == OTP_STEP_TYPE {
        if instance_status == INSTANCE_STATUS_COMPLETED {
            return models::KycStatus::Verified;
        }

        // If locked by try exhaustion.
        if context
            .get("phone_locked")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return models::KycStatus::Failed;
        }

        let latest_issue = attempts
            .iter()
            .filter(|a| a.step_name == STEP_PHONE_ISSUE_OTP)
            .max_by_key(|a| a.attempt_no);
        if latest_issue.is_some() {
            return models::KycStatus::InProgress;
        }
        return models::KycStatus::NotStarted;
    }

    models::KycStatus::NotStarted
}

fn parse_step_type(raw: &str) -> Result<models::StepType, Error> {
    raw.parse::<models::StepType>().map_err(|_| {
        Error::internal(
            "INVALID_STEP_TYPE",
            format!("Unsupported step type stored: {raw}"),
        )
    })
}

fn rate_limited_error(key: &'static str, message: &str) -> Error {
    Error::Http {
        error_key: key,
        status_code: 429,
        message: message.to_owned(),
        context: None,
    }
}

fn ensure_user_match(claims: &JwtToken, expected_user_id: &str) -> Result<(), Error> {
    let authed = BackendApi::require_user_id(claims)?;
    if authed != expected_user_id {
        return Err(Error::unauthorized(
            "Authenticated user does not match request userId",
        ));
    }
    Ok(())
}

#[backend_core::async_trait]
impl Steps<Error> for BackendApi {
    type Claims = JwtToken;

    async fn internal_start_session(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::InternalStartSessionRequest,
    ) -> Result<InternalStartSessionResponse, Error> {
        ensure_user_match(claims, &body.user_id)?;

        let engine = Engine::new(self.state.clone());
        let key = format!("{}:{}", KIND_KYC_PHONE_OTP, body.user_id);
        let mut ctx = json!({});
        // Ensure step list exists.
        if let Some(obj) = ctx.as_object_mut() {
            obj.insert("step_ids".to_owned(), Value::Array(vec![]));
        }

        let instance = engine
            .ensure_active_instance(KIND_KYC_PHONE_OTP, Some(body.user_id.clone()), key, ctx)
            .await?;

        let step_ids = instance
            .context
            .get("step_ids")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(InternalStartSessionResponse::Status201_Session(
            models::KycSessionInternal::new(
                instance.id,
                instance.user_id.unwrap_or_default(),
                parse_session_status(&instance.status),
                step_ids,
                instance.updated_at,
            ),
        ))
    }

    async fn internal_create_step(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::CreateStepRequest,
    ) -> Result<InternalCreateStepResponse, Error> {
        ensure_user_match(claims, &body.user_id)?;

        let session = self
            .state
            .sm
            .get_instance(&body.session_id)
            .await?
            .ok_or_else(|| Error::not_found("SESSION_NOT_FOUND", "Session not found"))?;

        if session.kind != KIND_KYC_PHONE_OTP {
            return Err(Error::bad_request(
                "INVALID_SESSION_KIND",
                "Unsupported session kind",
            ));
        }
        if session.user_id.as_deref() != Some(&body.user_id) {
            return Err(Error::unauthorized(
                "Session does not belong to authenticated user",
            ));
        }

        let step_type = body.r_type.to_string();
        if step_type != OTP_STEP_TYPE && step_type != MAGIC_STEP_TYPE {
            return Err(Error::bad_request(
                "STEP_TYPE_NOT_SUPPORTED",
                "Only PHONE and EMAIL steps are supported in this revamp",
            ));
        }

        let id = step_id(&session.id, &step_type);

        // Idempotent: ensure stepIds contains the deterministic id.
        let mut ctx = session.context;
        let step_ids_val = ctx.get_mut("step_ids");
        if let Some(Value::Array(ids)) = step_ids_val
            && !ids.iter().any(|v| v.as_str() == Some(&id))
        {
            ids.push(Value::String(id.clone()));
        }
        self.state
            .sm
            .update_instance_context(&session.id, ctx.clone())
            .await?;

        // Derive status from current machine state.
        let attempts = self.state.sm.list_step_attempts(&session.id).await?;
        let status = parse_step_status(&session.kind, &session.status, &step_type, &attempts, &ctx);

        Ok(InternalCreateStepResponse::Status201_StepCreated(
            models::KycStepInternal {
                id,
                session_id: session.id,
                user_id: body.user_id.clone(),
                r_type: parse_step_type(&step_type)?,
                status,
                data: None,
                policy: body.policy.clone(),
                created_at: session.created_at,
                updated_at: session.updated_at,
            },
        ))
    }

    async fn internal_get_step(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        path_params: &models::InternalGetStepPathParams,
    ) -> Result<InternalGetStepResponse, Error> {
        let (session_id, step_type) = split_step_id(&path_params.step_id)
            .ok_or_else(|| Error::bad_request("INVALID_STEP_ID", "Step id format is invalid"))?;

        let user_id = BackendApi::require_user_id(claims)?;
        let session = self
            .state
            .sm
            .get_instance(&session_id)
            .await?
            .ok_or_else(|| Error::not_found("SESSION_NOT_FOUND", "Session not found"))?;

        if session.user_id.as_deref() != Some(&user_id) {
            return Err(Error::unauthorized(
                "Step does not belong to authenticated user",
            ));
        }

        let attempts = self.state.sm.list_step_attempts(&session.id).await?;
        let status = parse_step_status(
            &session.kind,
            &session.status,
            &step_type,
            &attempts,
            &session.context,
        );

        Ok(InternalGetStepResponse::Status200_Step(
            models::KycStepInternal {
                id: path_params.step_id.clone(),
                session_id: session.id,
                user_id,
                r_type: parse_step_type(&step_type)?,
                status,
                data: None,
                policy: None,
                created_at: session.created_at,
                updated_at: session.updated_at,
            },
        ))
    }
}

#[backend_core::async_trait]
impl Notifications<Error> for BackendApi {
    type Claims = JwtToken;

    async fn internal_issue_otp(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::IssueOtpRequest,
    ) -> Result<InternalIssueOtpResponse, Error> {
        let (session_id, step_type) = split_step_id(&body.step_id)
            .ok_or_else(|| Error::bad_request("INVALID_STEP_ID", "Step id format is invalid"))?;
        if step_type != OTP_STEP_TYPE {
            return Err(Error::bad_request("INVALID_STEP", "Expected PHONE step"));
        }

        let user_id = BackendApi::require_user_id(claims)?;
        let session = self
            .state
            .sm
            .get_instance(&session_id)
            .await?
            .ok_or_else(|| Error::not_found("SESSION_NOT_FOUND", "Session not found"))?;

        if session.user_id.as_deref() != Some(&user_id) {
            return Err(Error::unauthorized(
                "Step does not belong to authenticated user",
            ));
        }

        // Rate limit by counting recent ISSUE_OTP attempts.
        let since = Utc::now() - Duration::minutes(OTP_RATE_LIMIT_WINDOW_MINUTES);
        let attempts = self.state.sm.list_step_attempts(&session.id).await?;
        let recent = attempts
            .iter()
            .filter(|a| a.step_name == STEP_PHONE_ISSUE_OTP)
            .filter(|a| a.queued_at.map(|t| t >= since).unwrap_or(false))
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

        Ok(InternalIssueOtpResponse::Status200_Challenge(
            models::OtpChallengeInternal::new(
                otp_ref,
                expires_at,
                u32::try_from(tries_left).unwrap_or(0),
            ),
        ))
    }

    async fn internal_verify_otp(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::VerifyOtpInternalRequest,
    ) -> Result<InternalVerifyOtpResponse, Error> {
        let (session_id, step_type) = split_step_id(&body.step_id)
            .ok_or_else(|| Error::bad_request("INVALID_STEP_ID", "Step id format is invalid"))?;
        if step_type != OTP_STEP_TYPE {
            return Err(Error::bad_request("INVALID_STEP", "Expected PHONE step"));
        }

        let user_id = BackendApi::require_user_id(claims)?;
        let session = self
            .state
            .sm
            .get_instance(&session_id)
            .await?
            .ok_or_else(|| Error::not_found("SESSION_NOT_FOUND", "Session not found"))?;
        if session.user_id.as_deref() != Some(&user_id) {
            return Err(Error::unauthorized(
                "Step does not belong to authenticated user",
            ));
        }

        let attempt = self
            .state
            .sm
            .get_step_attempt_by_external_ref(&session.id, STEP_PHONE_ISSUE_OTP, &body.otp_ref)
            .await?;

        let Some(attempt) = attempt else {
            return Ok(InternalVerifyOtpResponse::Status200_VerificationOutcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::Invalid,
                    models::KycStatus::Failed,
                ),
            ));
        };

        let output = attempt.output.unwrap_or(Value::Null);
        let expires_at = output
            .get("expires_at")
            .and_then(Value::as_str)
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|| Utc::now() - Duration::seconds(1));
        if expires_at < Utc::now() {
            return Ok(InternalVerifyOtpResponse::Status200_VerificationOutcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::Expired,
                    models::KycStatus::Failed,
                ),
            ));
        }

        let tries_left = output
            .get("tries_left")
            .and_then(Value::as_i64)
            .unwrap_or(0) as i32;
        if tries_left <= 0 {
            return Ok(InternalVerifyOtpResponse::Status200_VerificationOutcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::Locked,
                    models::KycStatus::Failed,
                ),
            ));
        }

        let otp_hash = output
            .get("otp_hash")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if otp_hash.is_empty() {
            return Ok(InternalVerifyOtpResponse::Status200_VerificationOutcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::Invalid,
                    models::KycStatus::InProgress,
                ),
            ));
        }

        if verify_secret(&body.code, otp_hash)? {
            let engine = Engine::new(self.state.clone());
            engine
                .emit_event(
                    &session.id,
                    "OTP_VERIFIED",
                    ActorType::User,
                    Some(user_id.clone()),
                    json!({"otp_ref": body.otp_ref}),
                )
                .await?;
            // Mark verify step and complete the instance.
            engine
                .mark_manual_step_succeeded(&session.id, STEP_PHONE_VERIFY_OTP)
                .await
                .ok();
            // Mark instance completed.
            self.state
                .sm
                .update_instance_status(&session.id, INSTANCE_STATUS_COMPLETED, Some(Utc::now()))
                .await?;

            return Ok(InternalVerifyOtpResponse::Status200_VerificationOutcome(
                models::VerifyOutcome::new(
                    true,
                    models::VerifyOutcomeReason::Verified,
                    models::KycStatus::Verified,
                ),
            ));
        }

        // Decrement tries_left in attempt output.
        let mut new_out = output.clone();
        if let Some(obj) = new_out.as_object_mut() {
            obj.insert(
                "tries_left".to_owned(),
                Value::Number(serde_json::Number::from((tries_left - 1).max(0))),
            );
        }
        let _ = self
            .state
            .sm
            .patch_step_attempt(
                &attempt.id,
                SmStepAttemptPatch {
                    status: None,
                    output: Some(Some(new_out.clone())),
                    error: None,
                    queued_at: None,
                    started_at: None,
                    finished_at: None,
                    next_retry_at: None,
                },
            )
            .await?;

        if tries_left - 1 <= 0 {
            let mut ctx = session.context;
            if let Some(obj) = ctx.as_object_mut() {
                obj.insert("phone_locked".to_owned(), Value::Bool(true));
            }
            self.state
                .sm
                .update_instance_context(&session.id, ctx)
                .await?;
            return Ok(InternalVerifyOtpResponse::Status200_VerificationOutcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::Locked,
                    models::KycStatus::Failed,
                ),
            ));
        }

        Ok(InternalVerifyOtpResponse::Status200_VerificationOutcome(
            models::VerifyOutcome::new(
                false,
                models::VerifyOutcomeReason::Invalid,
                models::KycStatus::InProgress,
            ),
        ))
    }

    async fn internal_issue_magic_email(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::IssueMagicEmailRequest,
    ) -> Result<InternalIssueMagicEmailResponse, Error> {
        let (session_id, step_type) = split_step_id(&body.step_id)
            .ok_or_else(|| Error::bad_request("INVALID_STEP_ID", "Step id format is invalid"))?;
        if step_type != MAGIC_STEP_TYPE {
            return Err(Error::bad_request("INVALID_STEP", "Expected EMAIL step"));
        }

        let user_id = BackendApi::require_user_id(claims)?;
        let session = self
            .state
            .sm
            .get_instance(&session_id)
            .await?
            .ok_or_else(|| Error::not_found("SESSION_NOT_FOUND", "Session not found"))?;
        if session.user_id.as_deref() != Some(&user_id) {
            return Err(Error::unauthorized(
                "Step does not belong to authenticated user",
            ));
        }

        let since = Utc::now() - Duration::minutes(MAGIC_RATE_LIMIT_WINDOW_MINUTES);
        let attempts = self.state.sm.list_step_attempts(&session.id).await?;
        let recent = attempts
            .iter()
            .filter(|a| a.step_name == "ISSUE_MAGIC_EMAIL")
            .filter(|a| a.queued_at.map(|t| t >= since).unwrap_or(false))
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
            .next_attempt_no(&session.id, "ISSUE_MAGIC_EMAIL")
            .await?;
        let now = Utc::now();
        let attempt = self
            .state
            .sm
            .create_step_attempt(SmStepAttemptCreateInput {
                id: backend_id::sm_attempt_id()?,
                instance_id: session.id.clone(),
                step_name: "ISSUE_MAGIC_EMAIL".to_owned(),
                attempt_no,
                status: ATTEMPT_STATUS_SUCCEEDED.to_owned(),
                external_ref: Some(token_ref.clone()),
                input: json!({"email": body.email.clone(), "ttl_seconds": ttl_seconds}),
                output: Some(json!({"token_ref": token_ref, "expires_at": expires_at, "token_hash": token_hash})),
                error: None,
                queued_at: Some(now),
                started_at: Some(now),
                finished_at: Some(now),
                next_retry_at: None,
            })
            .await?;

        // Best-effort notification.
        let _ = self
            .state
            .notification_queue
            .enqueue(NotificationJob::MagicEmail {
                step_id: body.step_id.clone(),
                email: attempt
                    .input
                    .get("email")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                token,
            })
            .await;

        Ok(InternalIssueMagicEmailResponse::Status200_Challenge(
            models::MagicEmailChallengeInternal::new(token_ref, expires_at),
        ))
    }

    async fn internal_verify_magic_email(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::VerifyMagicEmailRequest,
    ) -> Result<InternalVerifyMagicEmailResponse, Error> {
        let user_id = BackendApi::require_user_id(claims)?;

        let Some((token_ref, secret)) = body.token.split_once('.') else {
            return Ok(InternalVerifyMagicEmailResponse::Status200_Outcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::Invalid,
                    models::KycStatus::Failed,
                ),
            ));
        };

        // We don't have stepId here, so search by token_ref across user's active session.
        let key = format!("{}:{user_id}", KIND_KYC_PHONE_OTP);
        let Some(session) = self.state.sm.get_instance_by_idempotency_key(&key).await? else {
            return Ok(InternalVerifyMagicEmailResponse::Status200_Outcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::Invalid,
                    models::KycStatus::Failed,
                ),
            ));
        };

        let attempt = self
            .state
            .sm
            .get_step_attempt_by_external_ref(&session.id, "ISSUE_MAGIC_EMAIL", token_ref)
            .await?;
        let Some(attempt) = attempt else {
            return Ok(InternalVerifyMagicEmailResponse::Status200_Outcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::Invalid,
                    models::KycStatus::Failed,
                ),
            ));
        };

        let output = attempt.output.unwrap_or(Value::Null);
        let expires_at = output
            .get("expires_at")
            .and_then(Value::as_str)
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|| Utc::now() - Duration::seconds(1));
        if expires_at < Utc::now() {
            return Ok(InternalVerifyMagicEmailResponse::Status200_Outcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::Expired,
                    models::KycStatus::Failed,
                ),
            ));
        }

        let token_hash = output
            .get("token_hash")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if token_hash.is_empty() {
            return Ok(InternalVerifyMagicEmailResponse::Status200_Outcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::Invalid,
                    models::KycStatus::Failed,
                ),
            ));
        }

        if verify_secret(secret, token_hash)? {
            return Ok(InternalVerifyMagicEmailResponse::Status200_Outcome(
                models::VerifyOutcome::new(
                    true,
                    models::VerifyOutcomeReason::Verified,
                    models::KycStatus::Verified,
                ),
            ));
        }

        Ok(InternalVerifyMagicEmailResponse::Status200_Outcome(
            models::VerifyOutcome::new(
                false,
                models::VerifyOutcomeReason::Invalid,
                models::KycStatus::Failed,
            ),
        ))
    }
}

#[backend_core::async_trait]
impl Deposits<Error> for BackendApi {
    type Claims = JwtToken;

    async fn internal_create_phone_deposit(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::CreatePhoneDepositRequest,
    ) -> Result<InternalCreatePhoneDepositResponse, Error> {
        ensure_user_match(claims, &body.user_id)?;

        let engine = Engine::new(self.state.clone());
        let key = format!("{}:{}", KIND_KYC_FIRST_DEPOSIT, body.user_id);

        // Choose staff contact (same logic as legacy implementation).
        let (staff_id, staff_full_name, staff_phone_number) = self
            .state
            .sm
            .select_deposit_staff_contact(&body.user_id)
            .await?;

        let now = Utc::now();
        let expires_at = now + Duration::hours(2);

        let ctx = json!({
            "deposit": {
                "amount": body.amount,
                "currency": body.currency,
                "reason": body.reason,
                "reference": body.reference,
                "provider": body.provider.as_ref().map(|p| p.to_string()),
                "status": "CONTACT_PROVIDED",
                "expires_at": expires_at,
                "contact": {
                    "staff_id": staff_id,
                    "full_name": staff_full_name,
                    "phone_number": staff_phone_number,
                }
            }
        });

        let instance = engine
            .ensure_active_instance(KIND_KYC_FIRST_DEPOSIT, Some(body.user_id.clone()), key, ctx)
            .await?;

        // Ensure the first manual gate is open.
        engine
            .ensure_manual_step_running(&instance.id, STEP_DEPOSIT_AWAIT_PAYMENT)
            .await?;

        Ok(
            InternalCreatePhoneDepositResponse::Status201_DepositRequestCreated(
                phone_deposit_from_instance(instance)?,
            ),
        )
    }

    async fn internal_get_phone_deposit(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        path_params: &models::InternalGetPhoneDepositPathParams,
    ) -> Result<InternalGetPhoneDepositResponse, Error> {
        let user_id = BackendApi::require_user_id(claims)?;

        let Some(instance) = self.state.sm.get_instance(&path_params.deposit_id).await? else {
            return Err(Error::not_found(
                "DEPOSIT_NOT_FOUND",
                "Deposit request not found",
            ));
        };

        if instance.user_id.as_deref() != Some(&user_id) {
            return Err(Error::unauthorized(
                "Deposit request does not belong to authenticated user",
            ));
        }

        Ok(InternalGetPhoneDepositResponse::Status200_DepositRequest(
            phone_deposit_from_instance(instance)?,
        ))
    }
}

#[backend_core::async_trait]
impl Uploads<Error> for BackendApi {
    type Claims = JwtToken;

    async fn internal_presign_upload(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::InternalPresignRequest,
    ) -> Result<InternalPresignUploadResponse, Error> {
        // This backend no longer persists uploads/evidence in SQL; we only provide presigned routines.
        // Ownership is still enforced via JWT user id.
        let user_id = BackendApi::require_user_id(claims)?;
        if user_id != body.user_id {
            return Err(Error::unauthorized(
                "Authenticated user does not match request userId",
            ));
        }

        let bucket = self
            .state
            .config
            .s3
            .as_ref()
            .map(|s3| s3.bucket.clone())
            .ok_or_else(|| Error::internal("S3_NOT_CONFIGURED", "S3 is not configured"))?;

        let upload_id = backend_id::kyc_upload_id()?;
        let object_key = format!("uploads/{}/{}", body.user_id, upload_id);

        let encryption = match body.encryption.as_ref().map(|e| e.mode) {
            Some(models::SseMode::SseS3) => EncryptionMode::S3,
            Some(models::SseMode::SseKms) => EncryptionMode::Kms,
            _ => EncryptionMode::S3,
        };

        let presigned = self
            .state
            .s3
            .upload_presigned(
                &bucket,
                &object_key,
                &body.mime,
                encryption,
                std::time::Duration::from_secs(300),
            )
            .await?;

        Ok(InternalPresignUploadResponse::Status200_PresignResponse(
            models::PresignUploadResponseInternal {
                upload_id,
                bucket,
                object_key,
                method: models::UploadMethod::Put,
                url: Some(presigned.url),
                headers: Some(presigned.headers),
                multipart: None,
                expires_at: Utc::now() + Duration::seconds(300),
            },
        ))
    }

    async fn internal_complete_upload(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::InternalCompleteUploadRequest,
    ) -> Result<InternalCompleteUploadResponse, Error> {
        let _user_id = BackendApi::require_user_id(claims)?;
        // Best-effort: validate that object exists for S3 setups.
        let _ = self
            .state
            .s3
            .head_object(&body.bucket, &body.object_key)
            .await;

        Ok(
            InternalCompleteUploadResponse::Status200_EvidenceRegistered(models::EvidenceRef::new(
                backend_id::kyc_evidence_id()?,
                body.upload_id.clone(),
                "EVIDENCE".to_owned(),
                Utc::now(),
            )),
        )
    }
}

fn parse_deposit_status(raw: &str) -> Result<models::DepositStatus, Error> {
    raw.parse::<models::DepositStatus>().map_err(|_| {
        Error::internal(
            "INVALID_DEPOSIT_STATUS",
            format!("Unsupported deposit status: {raw}"),
        )
    })
}

fn phone_deposit_from_instance(
    instance: backend_model::db::SmInstanceRow,
) -> Result<models::PhoneDepositResponse, Error> {
    let deposit = instance
        .context
        .get("deposit")
        .cloned()
        .unwrap_or(Value::Null);

    let status = deposit
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("CONTACT_PROVIDED");
    let amount = deposit.get("amount").and_then(Value::as_f64).unwrap_or(0.0);
    let currency = deposit
        .get("currency")
        .and_then(Value::as_str)
        .unwrap_or("XAF")
        .to_owned();

    let contact = deposit.get("contact").cloned().unwrap_or(Value::Null);
    let staff_id = contact
        .get("staff_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let full_name = contact
        .get("full_name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let phone_number = contact
        .get("phone_number")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();

    let expires_at = deposit
        .get("expires_at")
        .and_then(Value::as_str)
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    Ok(models::PhoneDepositResponse {
        deposit_id: instance.id,
        status: parse_deposit_status(status)?,
        amount,
        currency,
        contact: models::StaffContact {
            staff_id,
            full_name,
            phone_number,
        },
        expires_at,
        created_at: instance.created_at,
    })
}
