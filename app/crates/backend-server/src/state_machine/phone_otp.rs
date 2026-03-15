use crate::state::AppState;
use crate::state_machine::jobs::StateMachineStepJob;
use crate::state_machine::secrets::hash_secret;
use crate::state_machine::types::*;
use backend_core::{Error, NotificationJob};
use backend_repository::{SmStepAttemptCreateInput, SmStepAttemptPatch};
use chrono::{Duration, Utc};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio::time::sleep;
use tracing::warn;

const OTP_MAX_TRIES: i32 = 5;
const OTP_ENQUEUE_MAX_RETRIES: i32 = 2;
const OTP_ENQUEUE_INITIAL_BACKOFF_SECONDS: i64 = 1;

pub enum PhoneIssueOtpOutcome {
    Succeeded,
    RetryScheduled,
    FailedTerminal,
}

pub struct PhoneOtpEngine {
    state: Arc<AppState>,
}

impl PhoneOtpEngine {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    pub async fn enqueue_phone_issue_otp(
        &self,
        instance_id: &str,
        msisdn: &str,
        channel: &str,
        ttl_seconds: i64,
    ) -> Result<(String, chrono::DateTime<Utc>, i32), Error> {
        let now = Utc::now();
        let expires_at = now + Duration::seconds(ttl_seconds.clamp(30, 900));
        let otp_ref = backend_id::kyc_otp_ref()?;

        let attempt_no = self
            .state
            .sm
            .next_attempt_no(instance_id, STEP_PHONE_ISSUE_OTP)
            .await?;

        let attempt = self
            .state
            .sm
            .create_step_attempt(SmStepAttemptCreateInput {
                id: backend_id::sm_attempt_id()?,
                instance_id: instance_id.to_owned(),
                step_name: STEP_PHONE_ISSUE_OTP.to_owned(),
                attempt_no,
                status: ATTEMPT_STATUS_QUEUED.to_owned(),
                external_ref: Some(otp_ref.clone()),
                input: json!({
                    "msisdn": msisdn,
                    "channel": channel,
                    "ttl_seconds": ttl_seconds,
                    "max_tries": OTP_MAX_TRIES,
                }),
                output: Some(json!({
                    "otp_ref": otp_ref,
                    "expires_at": expires_at,
                    "tries_left": OTP_MAX_TRIES,
                    "otp_hash": null,
                })),
                error: None,
                queued_at: Some(now),
                started_at: None,
                finished_at: None,
                next_retry_at: None,
            })
            .await?;

        self.state
            .sm
            .cancel_other_attempts_for_step(instance_id, STEP_PHONE_ISSUE_OTP, &attempt.id)
            .await?;

        self.state
            .sm
            .update_instance_status(instance_id, INSTANCE_STATUS_RUNNING, None)
            .await?;

        self.state
            .sm_queue
            .enqueue(StateMachineStepJob {
                instance_id: instance_id.to_owned(),
                step_name: STEP_PHONE_ISSUE_OTP.to_owned(),
                attempt_id: attempt.id,
            })
            .await
            .map_err(|err: backend_core::Error| {
                tracing::warn!(
                    instance_id = %instance_id,
                    step_name = %STEP_PHONE_ISSUE_OTP,
                    error = %err,
                    "SM enqueue failed for phone OTP step"
                );
                Error::internal("SM_ENQUEUE_FAILED", err.to_string())
            })?;

        Ok((otp_ref, expires_at, OTP_MAX_TRIES))
    }

    pub async fn run_phone_issue_otp(
        &self,
        job: StateMachineStepJob,
        attempt: backend_model::db::SmStepAttemptRow,
    ) -> Result<PhoneIssueOtpOutcome, Error> {
        let input = attempt.input.clone();
        let msisdn = input
            .get("msisdn")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::internal("SM_INPUT_INVALID", "Missing msisdn"))?;

        let otp = format!("{:06}", rand::random::<u32>() % 1_000_000);
        let otp_hash = hash_secret(&otp)?;

        let mut output = attempt.output.clone().unwrap_or_else(|| json!({}));
        if let Some(obj) = output.as_object_mut() {
            obj.insert("otp_hash".to_owned(), Value::String(otp_hash));
        }

        if let Err(error) = self
            .state
            .notification_queue
            .enqueue(NotificationJob::Otp {
                step_id: job.attempt_id.clone(),
                msisdn: msisdn.to_owned(),
                otp,
            })
            .await
        {
            let message = error.to_string();
            warn!(
                attempt_id = %job.attempt_id,
                step_name = %job.step_name,
                msisdn = %msisdn,
                error = %message,
                "failed to enqueue OTP notification"
            );

            let finished = Utc::now();
            let can_retry = attempt.attempt_no <= OTP_ENQUEUE_MAX_RETRIES;
            let mut retry_at = None;

            if can_retry {
                let backoff_seconds = otp_retry_backoff_seconds(attempt.attempt_no);
                retry_at = Some(finished + Duration::seconds(backoff_seconds));

                let retry_attempt = self
                    .create_sms_retry_attempt(&attempt, retry_at.unwrap())
                    .await?;

                self.schedule_step_retry(
                    retry_attempt.instance_id.clone(),
                    retry_attempt.step_name.clone(),
                    retry_attempt.id.clone(),
                    backoff_seconds,
                );
            }

            let _ = self
                .state
                .sm
                .patch_step_attempt(
                    &job.attempt_id,
                    SmStepAttemptPatch::new()
                        .status(ATTEMPT_STATUS_FAILED)
                        .error(json!({
                            "error": message,
                            "transient": true,
                            "will_retry": can_retry,
                        }))
                        .output(output)
                        .finished_at(finished)
                        .next_retry_at_opt(retry_at),
                )
                .await?;

            if can_retry {
                return Ok(PhoneIssueOtpOutcome::RetryScheduled);
            }
            return Ok(PhoneIssueOtpOutcome::FailedTerminal);
        }

        // OTP delivery succeeded.
        let finished = Utc::now();
        let _ = self
            .state
            .sm
            .patch_step_attempt(
                &job.attempt_id,
                SmStepAttemptPatch::new()
                    .status(ATTEMPT_STATUS_SUCCEEDED)
                    .output(output)
                    .clear_error()
                    .finished_at(finished)
                    .clear_next_retry_at(),
            )
            .await?;

        Ok(PhoneIssueOtpOutcome::Succeeded)
    }

    async fn create_sms_retry_attempt(
        &self,
        previous_attempt: &backend_model::db::SmStepAttemptRow,
        next_retry_at: chrono::DateTime<Utc>,
    ) -> Result<backend_model::db::SmStepAttemptRow, Error> {
        let next_attempt_no = self
            .state
            .sm
            .next_attempt_no(&previous_attempt.instance_id, &previous_attempt.step_name)
            .await?;

        self.state
            .sm
            .create_step_attempt(SmStepAttemptCreateInput {
                id: backend_id::sm_attempt_id()?,
                instance_id: previous_attempt.instance_id.clone(),
                step_name: previous_attempt.step_name.clone(),
                attempt_no: next_attempt_no,
                status: ATTEMPT_STATUS_QUEUED.to_owned(),
                external_ref: previous_attempt.external_ref.clone(),
                input: previous_attempt.input.clone(),
                output: previous_attempt.output.clone(),
                error: None,
                queued_at: Some(Utc::now()),
                started_at: None,
                finished_at: None,
                next_retry_at: Some(next_retry_at),
            })
            .await
    }

    fn schedule_step_retry(
        &self,
        instance_id: String,
        step_name: String,
        attempt_id: String,
        backoff_seconds: i64,
    ) {
        let queue = self.state.sm_queue.clone();
        tokio::spawn(async move {
            sleep(StdDuration::from_secs(backoff_seconds as u64)).await;
            if let Err(error) = queue
                .enqueue(StateMachineStepJob {
                    instance_id,
                    step_name,
                    attempt_id,
                })
                .await
            {
                tracing::warn!("failed to enqueue SMS retry job: {}", error);
            }
        });
    }
}

fn otp_retry_backoff_seconds(attempt_no: i32) -> i64 {
    let exponent = attempt_no.saturating_sub(1).max(0) as u32;
    let factor = 2_i64.saturating_pow(exponent);
    OTP_ENQUEUE_INITIAL_BACKOFF_SECONDS
        .saturating_mul(factor)
        .max(1)
}
