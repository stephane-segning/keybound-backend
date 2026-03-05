use crate::sms_provider::SmsProvider;
use crate::state::AppState;
use crate::state_machine::jobs::StateMachineStepJob;
use crate::state_machine::secrets::hash_secret;
use crate::state_machine::types::*;
use backend_core::Error;
use backend_repository::{
    SmEventCreateInput, SmInstanceCreateInput, SmStepAttemptCreateInput, SmStepAttemptPatch,
};
use chrono::{Duration, Utc};
use serde_json::{Value, json};
use std::sync::Arc;

const OTP_MAX_TRIES: i32 = 5;

pub struct Engine {
    state: Arc<AppState>,
}

impl Engine {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    pub async fn ensure_active_instance(
        &self,
        kind: &str,
        user_id: Option<String>,
        idempotency_key: String,
        context: Value,
    ) -> Result<backend_model::db::SmInstanceRow, Error> {
        if let Some(found) = self
            .state
            .sm
            .get_instance_by_idempotency_key(&idempotency_key)
            .await?
        {
            return Ok(found);
        }

        let instance_id = backend_id::sm_instance_id()?;
        let created = self
            .state
            .sm
            .create_instance(SmInstanceCreateInput {
                id: instance_id,
                kind: kind.to_owned(),
                user_id,
                idempotency_key,
                status: INSTANCE_STATUS_ACTIVE.to_owned(),
                context,
            })
            .await?;

        Ok(created)
    }

    pub async fn emit_event(
        &self,
        instance_id: &str,
        kind: &str,
        actor_type: ActorType,
        actor_id: Option<String>,
        payload: Value,
    ) -> Result<(), Error> {
        let _ = self
            .state
            .sm
            .append_event(SmEventCreateInput {
                id: backend_id::sm_event_id()?,
                instance_id: instance_id.to_owned(),
                kind: kind.to_owned(),
                actor_type: actor_type.as_str().to_owned(),
                actor_id,
                payload,
            })
            .await?;
        Ok(())
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

        // Cancel older issuance attempts deterministically. Old refs become invalid.
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
            .map_err(|err| Error::internal("SM_ENQUEUE_FAILED", err.to_string()))?;

        Ok((otp_ref, expires_at, OTP_MAX_TRIES))
    }

    pub async fn staff_confirm_deposit_payment(
        &self,
        instance_id: &str,
        staff_user_id: Option<String>,
        payload: Value,
    ) -> Result<(), Error> {
        self.emit_event(
            instance_id,
            "PAYMENT_CONFIRMED",
            ActorType::Staff,
            staff_user_id,
            payload,
        )
        .await?;

        // Mark manual step succeeded and open next manual gate.
        self.mark_manual_step_succeeded(instance_id, STEP_DEPOSIT_AWAIT_PAYMENT)
            .await?;
        self.ensure_manual_step_running(instance_id, STEP_DEPOSIT_AWAIT_APPROVAL)
            .await?;

        Ok(())
    }

    pub async fn staff_approve_deposit(
        &self,
        instance_id: &str,
        staff_user_id: Option<String>,
        payload: Value,
    ) -> Result<(), Error> {
        if let Some(latest_register_attempt) = self
            .state
            .sm
            .get_latest_step_attempt(instance_id, STEP_DEPOSIT_REGISTER_CUSTOMER)
            .await?
        {
            if latest_register_attempt.status != ATTEMPT_STATUS_FAILED {
                return Err(Error::conflict(
                    "DEPOSIT_ALREADY_APPROVED",
                    "Deposit has already been approved for processing",
                ));
            }
        }

        self.emit_event(
            instance_id,
            "DEPOSIT_APPROVED",
            ActorType::Staff,
            staff_user_id,
            payload.clone(),
        )
        .await?;

        self.mark_manual_step_succeeded(instance_id, STEP_DEPOSIT_AWAIT_APPROVAL)
            .await?;

        // Start async workflow.
        let attempt_id = self
            .create_async_attempt(instance_id, STEP_DEPOSIT_REGISTER_CUSTOMER, json!({}), None)
            .await?;

        self.state
            .sm
            .update_instance_status(instance_id, INSTANCE_STATUS_RUNNING, None)
            .await?;

        self.state
            .sm_queue
            .enqueue(StateMachineStepJob {
                instance_id: instance_id.to_owned(),
                step_name: STEP_DEPOSIT_REGISTER_CUSTOMER.to_owned(),
                attempt_id,
            })
            .await
            .map_err(|err| Error::internal("SM_ENQUEUE_FAILED", err.to_string()))?;

        Ok(())
    }

    pub async fn ensure_manual_step_running(
        &self,
        instance_id: &str,
        step_name: &str,
    ) -> Result<(), Error> {
        if let Some(latest) = self
            .state
            .sm
            .get_latest_step_attempt(instance_id, step_name)
            .await?
        {
            if latest.status == ATTEMPT_STATUS_SUCCEEDED {
                return Ok(());
            }
            return Ok(());
        }

        let attempt_id = backend_id::sm_attempt_id()?;
        let attempt_no = 1;
        let now = Utc::now();
        let _ = self
            .state
            .sm
            .create_step_attempt(SmStepAttemptCreateInput {
                id: attempt_id,
                instance_id: instance_id.to_owned(),
                step_name: step_name.to_owned(),
                attempt_no,
                status: ATTEMPT_STATUS_RUNNING.to_owned(),
                external_ref: None,
                input: json!({}),
                output: None,
                error: None,
                queued_at: None,
                started_at: Some(now),
                finished_at: None,
                next_retry_at: None,
            })
            .await?;

        self.state
            .sm
            .update_instance_status(instance_id, INSTANCE_STATUS_WAITING_INPUT, None)
            .await?;

        Ok(())
    }

    pub async fn mark_manual_step_succeeded(
        &self,
        instance_id: &str,
        step_name: &str,
    ) -> Result<(), Error> {
        let Some(latest) = self
            .state
            .sm
            .get_latest_step_attempt(instance_id, step_name)
            .await?
        else {
            return Err(Error::bad_request(
                "SM_STEP_NOT_STARTED",
                format!("Manual step {step_name} has not been started"),
            ));
        };

        let now = Utc::now();
        let _ = self
            .state
            .sm
            .patch_step_attempt(
                &latest.id,
                SmStepAttemptPatch {
                    status: Some(ATTEMPT_STATUS_SUCCEEDED.to_owned()),
                    output: None,
                    error: Some(None),
                    queued_at: None,
                    started_at: None,
                    finished_at: Some(Some(now)),
                    next_retry_at: Some(None),
                },
            )
            .await?;

        Ok(())
    }

    async fn create_async_attempt(
        &self,
        instance_id: &str,
        step_name: &str,
        input: Value,
        external_ref: Option<String>,
    ) -> Result<String, Error> {
        let attempt_no = self
            .state
            .sm
            .next_attempt_no(instance_id, step_name)
            .await?;
        let now = Utc::now();
        let attempt = self
            .state
            .sm
            .create_step_attempt(SmStepAttemptCreateInput {
                id: backend_id::sm_attempt_id()?,
                instance_id: instance_id.to_owned(),
                step_name: step_name.to_owned(),
                attempt_no,
                status: ATTEMPT_STATUS_QUEUED.to_owned(),
                external_ref,
                input,
                output: None,
                error: None,
                queued_at: Some(now),
                started_at: None,
                finished_at: None,
                next_retry_at: None,
            })
            .await?;
        Ok(attempt.id)
    }

    pub async fn process_step_job(
        &self,
        job: StateMachineStepJob,
        sms_provider: Arc<dyn SmsProvider>,
    ) -> Result<(), Error> {
        // Atomically claim the attempt so duplicate jobs do not re-run side effects.
        // If it's not queued anymore (already running/finished/cancelled), treat the job as a no-op.
        let Some(claimed_attempt) = self.state.sm.claim_step_attempt(&job.attempt_id).await? else {
            return Ok(());
        };
        if claimed_attempt.instance_id != job.instance_id
            || claimed_attempt.step_name != job.step_name
        {
            return Err(Error::internal(
                "SM_JOB_MISMATCH",
                "Job does not match claimed attempt",
            ));
        }

        let instance = self
            .state
            .sm
            .get_instance(&job.instance_id)
            .await?
            .ok_or_else(|| Error::not_found("SM_INSTANCE_NOT_FOUND", "Instance not found"))?;

        let instance_id = job.instance_id.clone();

        match (instance.kind.as_str(), job.step_name.as_str()) {
            (KIND_KYC_PHONE_OTP, STEP_PHONE_ISSUE_OTP) => {
                self.run_phone_issue_otp(job, claimed_attempt, sms_provider)
                    .await?;
                // After SMS sent, create manual gate.
                self.ensure_manual_step_running(&instance.id, STEP_PHONE_VERIFY_OTP)
                    .await?;
            }
            (KIND_KYC_FIRST_DEPOSIT, STEP_DEPOSIT_REGISTER_CUSTOMER) => {
                self.run_deposit_register_customer(instance, job).await?;
            }
            (KIND_KYC_FIRST_DEPOSIT, STEP_DEPOSIT_APPROVE_AND_DEPOSIT) => {
                self.run_deposit_approve_and_deposit(instance, job).await?;
                self.finish_instance_success(&instance_id).await?;
            }
            _ => {
                return Err(Error::internal(
                    "SM_STEP_NOT_SUPPORTED",
                    format!(
                        "Unsupported step {} for kind {}",
                        job.step_name, instance.kind
                    ),
                ));
            }
        }

        Ok(())
    }

    async fn run_phone_issue_otp(
        &self,
        job: StateMachineStepJob,
        attempt: backend_model::db::SmStepAttemptRow,
        sms_provider: Arc<dyn SmsProvider>,
    ) -> Result<(), Error> {
        let input = attempt.input;
        let msisdn = input
            .get("msisdn")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::internal("SM_INPUT_INVALID", "Missing msisdn"))?;

        let otp = format!("{:06}", rand::random::<u32>() % 1_000_000);
        let otp_hash = hash_secret(&otp)?;

        // Merge into output.
        let mut output = attempt.output.unwrap_or_else(|| json!({}));
        if let Some(obj) = output.as_object_mut() {
            obj.insert("otp_hash".to_owned(), Value::String(otp_hash));
        }

        // Send SMS first; if this fails, mark FAILED and do not leak OTP hash.
        if let Err(err) = sms_provider.send_otp(msisdn, &otp).await {
            let finished = Utc::now();
            let _ = self
                .state
                .sm
                .patch_step_attempt(
                    &job.attempt_id,
                    SmStepAttemptPatch {
                        status: Some(ATTEMPT_STATUS_FAILED.to_owned()),
                        // Keep the prior output (otp_ref/expires/tries) but without a hash update.
                        output: None,
                        error: Some(Some(json!({"error": err.to_string()}))),
                        queued_at: None,
                        started_at: None,
                        finished_at: Some(Some(finished)),
                        next_retry_at: Some(None),
                    },
                )
                .await?;
            return Ok(());
        }

        let finished = Utc::now();
        let _ = self
            .state
            .sm
            .patch_step_attempt(
                &job.attempt_id,
                SmStepAttemptPatch {
                    status: Some(ATTEMPT_STATUS_SUCCEEDED.to_owned()),
                    output: Some(Some(output)),
                    error: Some(None),
                    queued_at: None,
                    started_at: None,
                    finished_at: Some(Some(finished)),
                    next_retry_at: Some(None),
                },
            )
            .await?;

        Ok(())
    }

    async fn run_deposit_register_customer(
        &self,
        instance: backend_model::db::SmInstanceRow,
        job: StateMachineStepJob,
    ) -> Result<(), Error> {
        use gen_oas_client_cuss::apis::Error as CussApiError;
        use gen_oas_client_cuss::apis::registration_api;
        use gen_oas_client_cuss::models::RegistrationRequest;

        let user_id = instance
            .user_id
            .clone()
            .ok_or_else(|| Error::internal("SM_INSTANCE_INVALID", "Missing user_id"))?;
        let user = self
            .state
            .user
            .get_user(&user_id)
            .await?
            .ok_or_else(|| Error::bad_request("USER_NOT_FOUND", "User not found"))?;
        let phone = user.phone_number.clone().ok_or_else(|| {
            Error::bad_request("USER_PHONE_REQUIRED", "User phone number is required")
        })?;

        let approval = instance
            .context
            .get("approval")
            .cloned()
            .unwrap_or(Value::Null);

        let first_name = approval
            .get("first_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let last_name = approval
            .get("last_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();

        let req = RegistrationRequest {
            first_name,
            last_name,
            email: None,
            phone,
            external_id: user_id.clone(),
            date_of_birth: None,
        };

        let config = self.cuss_client_config();
        let resp = match registration_api::register_customer(&config, req).await {
            Ok(response) => response,
            Err(CussApiError::ResponseError(response_error)) => {
                self.mark_attempt_failed(
                    &job.attempt_id,
                    json!({
                        "status": response_error.status.as_u16(),
                        "body": response_error.content,
                    }),
                )
                .await?;
                return Err(Error::internal(
                    "CUS_REGISTER_FAILED",
                    format!("CUSS register failed: {}", response_error.status),
                ));
            }
            Err(error) => {
                self.mark_attempt_failed(
                    &job.attempt_id,
                    json!({
                        "error": error.to_string(),
                    }),
                )
                .await?;
                return Err(Error::internal(
                    "CUS_REGISTER_CALL_FAILED",
                    error.to_string(),
                ));
            }
        };

        let resp = serde_json::to_value(resp)
            .map_err(|e| Error::internal("CUS_RESP_SERIALIZE_FAILED", e.to_string()))?;

        let finished = Utc::now();
        let _ = self
            .state
            .sm
            .patch_step_attempt(
                &job.attempt_id,
                SmStepAttemptPatch {
                    status: Some(ATTEMPT_STATUS_SUCCEEDED.to_owned()),
                    output: Some(Some(resp.clone())),
                    error: Some(None),
                    queued_at: None,
                    started_at: None,
                    finished_at: Some(Some(finished)),
                    next_retry_at: Some(None),
                },
            )
            .await?;

        // Queue next step.
        let next_attempt_id = self
            .create_async_attempt(
                &instance.id,
                STEP_DEPOSIT_APPROVE_AND_DEPOSIT,
                json!({}),
                None,
            )
            .await?;

        self.state
            .sm_queue
            .enqueue(StateMachineStepJob {
                instance_id: instance.id.clone(),
                step_name: STEP_DEPOSIT_APPROVE_AND_DEPOSIT.to_owned(),
                attempt_id: next_attempt_id,
            })
            .await
            .map_err(|err| Error::internal("SM_ENQUEUE_FAILED", err.to_string()))?;

        Ok(())
    }

    async fn run_deposit_approve_and_deposit(
        &self,
        instance: backend_model::db::SmInstanceRow,
        job: StateMachineStepJob,
    ) -> Result<(), Error> {
        use gen_oas_client_cuss::apis::Error as CussApiError;
        use gen_oas_client_cuss::apis::registration_api;
        use gen_oas_client_cuss::models::ApproveAndDepositRequest;

        // Get savingsAccountId from previous step output.
        let reg_attempt = self
            .state
            .sm
            .get_latest_step_attempt(&instance.id, STEP_DEPOSIT_REGISTER_CUSTOMER)
            .await?
            .ok_or_else(|| Error::internal("SM_DEP_MISSING", "Missing registerCustomer output"))?;

        let savings_account_id = reg_attempt
            .output
            .as_ref()
            .and_then(|v| {
                v.get("savingsAccountId")
                    .or_else(|| v.get("savings_account_id"))
            })
            .and_then(Value::as_i64)
            .ok_or_else(|| Error::internal("SM_DEP_INVALID", "Missing savingsAccountId"))?;

        let approval = instance
            .context
            .get("approval")
            .cloned()
            .unwrap_or(Value::Null);
        let deposit_amount = approval
            .get("deposit_amount")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);

        let req = ApproveAndDepositRequest {
            savings_account_id,
            deposit_amount: Some(deposit_amount),
        };

        let config = self.cuss_client_config();
        let resp = match registration_api::approve_and_deposit(&config, req).await {
            Ok(response) => response,
            Err(CussApiError::ResponseError(response_error)) => {
                self.mark_attempt_failed(
                    &job.attempt_id,
                    json!({
                        "status": response_error.status.as_u16(),
                        "body": response_error.content,
                    }),
                )
                .await?;
                return Err(Error::internal(
                    "CUS_APPROVE_DEPOSIT_FAILED",
                    format!("CUSS approve-and-deposit failed: {}", response_error.status),
                ));
            }
            Err(error) => {
                self.mark_attempt_failed(
                    &job.attempt_id,
                    json!({
                        "error": error.to_string(),
                    }),
                )
                .await?;
                return Err(Error::internal(
                    "CUS_APPROVE_DEPOSIT_CALL_FAILED",
                    error.to_string(),
                ));
            }
        };

        let resp = serde_json::to_value(resp)
            .map_err(|e| Error::internal("CUS_RESP_SERIALIZE_FAILED", e.to_string()))?;

        let finished = Utc::now();
        let _ = self
            .state
            .sm
            .patch_step_attempt(
                &job.attempt_id,
                SmStepAttemptPatch {
                    status: Some(ATTEMPT_STATUS_SUCCEEDED.to_owned()),
                    output: Some(Some(resp)),
                    error: Some(None),
                    queued_at: None,
                    started_at: None,
                    finished_at: Some(Some(finished)),
                    next_retry_at: Some(None),
                },
            )
            .await?;

        Ok(())
    }

    async fn finish_instance_success(&self, instance_id: &str) -> Result<(), Error> {
        let now = Utc::now();
        let mark_complete_attempt = self
            .state
            .sm
            .create_step_attempt(SmStepAttemptCreateInput {
                id: backend_id::sm_attempt_id()?,
                instance_id: instance_id.to_owned(),
                step_name: STEP_MARK_COMPLETE.to_owned(),
                attempt_no: 1,
                status: ATTEMPT_STATUS_SUCCEEDED.to_owned(),
                external_ref: None,
                input: json!({}),
                output: None,
                error: None,
                queued_at: None,
                started_at: Some(now),
                finished_at: Some(now),
                next_retry_at: None,
            })
            .await;
        if let Err(error) = mark_complete_attempt {
            match &error {
                Error::Diesel(diesel::result::Error::DatabaseError(
                    diesel::result::DatabaseErrorKind::UniqueViolation,
                    _,
                )) => {
                    // Duplicate MARK_COMPLETE attempts are benign: proceed to finalize instance.
                }
                _ => return Err(error),
            }
        }

        self.state
            .sm
            .update_instance_status(instance_id, INSTANCE_STATUS_COMPLETED, Some(now))
            .await?;
        Ok(())
    }

    fn cuss_client_config(&self) -> gen_oas_client_cuss::apis::configuration::Configuration {
        let mut config = gen_oas_client_cuss::apis::configuration::Configuration::new();
        config.base_path = self.state.config.cuss.api_url.clone();
        config.user_agent = Some("user-storage/1.0.0".to_owned());
        config
    }

    async fn mark_attempt_failed(
        &self,
        attempt_id: &str,
        error_payload: Value,
    ) -> Result<(), Error> {
        let finished = Utc::now();
        let _ = self
            .state
            .sm
            .patch_step_attempt(
                attempt_id,
                SmStepAttemptPatch {
                    status: Some(ATTEMPT_STATUS_FAILED.to_owned()),
                    output: Some(None),
                    error: Some(Some(error_payload)),
                    queued_at: None,
                    started_at: None,
                    finished_at: Some(Some(finished)),
                    next_retry_at: Some(None),
                },
            )
            .await?;
        Ok(())
    }
}
