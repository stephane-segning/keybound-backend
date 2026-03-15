use crate::state::AppState;
use crate::state_machine::event::EventEmitter;
use crate::state_machine::jobs::StateMachineStepJob;
use crate::state_machine::manual_step::ManualStepManager;
use crate::state_machine::types::*;
use backend_core::Error;
use backend_repository::{SmStepAttemptCreateInput, SmStepAttemptPatch, UserDataUpsertInput};
use chrono::Utc;
use serde_json::{Value, json};
use std::sync::Arc;
use tracing::warn;

pub struct DepositEngine {
    state: Arc<AppState>,
    events: EventEmitter,
    manual_steps: ManualStepManager,
}

impl DepositEngine {
    pub fn new(state: Arc<AppState>) -> Self {
        let events = EventEmitter::new(state.clone());
        let manual_steps = ManualStepManager::new(state.clone());
        Self {
            state,
            events,
            manual_steps,
        }
    }

    pub async fn staff_confirm_deposit_payment(
        &self,
        instance_id: &str,
        staff_user_id: Option<String>,
        payload: Value,
    ) -> Result<(), Error> {
        self.events
            .emit_event(
                instance_id,
                "PAYMENT_CONFIRMED",
                ActorType::Staff,
                staff_user_id,
                payload,
            )
            .await?;

        self.manual_steps
            .mark_manual_step_succeeded(instance_id, STEP_DEPOSIT_AWAIT_PAYMENT)
            .await?;
        self.manual_steps
            .ensure_manual_step_running(instance_id, STEP_DEPOSIT_AWAIT_APPROVAL)
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
            && latest_register_attempt.status != ATTEMPT_STATUS_FAILED
        {
            return Err(Error::conflict(
                "DEPOSIT_ALREADY_APPROVED",
                "Deposit has already been approved for processing",
            ));
        }

        self.events
            .emit_event(
                instance_id,
                "DEPOSIT_APPROVED",
                ActorType::Staff,
                staff_user_id,
                payload.clone(),
            )
            .await?;

        self.manual_steps
            .mark_manual_step_succeeded(instance_id, STEP_DEPOSIT_AWAIT_APPROVAL)
            .await?;

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
                attempt_id: attempt_id.clone(),
            })
            .await
            .map_err(|err| {
                warn!(
                    instance_id = %instance_id,
                    step_name = %STEP_DEPOSIT_REGISTER_CUSTOMER,
                    attempt_id = %attempt_id,
                    error = %err,
                    "SM enqueue failed for deposit register customer step"
                );
                Error::internal("SM_ENQUEUE_FAILED", err.to_string())
            })?;

        Ok(())
    }

    pub async fn run_deposit_register_customer(
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

        let full_name = approval
            .get("full_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();

        let req = RegistrationRequest {
            full_name,
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
                warn!(
                    attempt_id = %job.attempt_id,
                    user_id = %user_id,
                    status = %response_error.status,
                    "CUSS register customer failed with API error response"
                );
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
                warn!(
                    attempt_id = %job.attempt_id,
                    user_id = %user_id,
                    error = %error,
                    "CUSS register customer call failed"
                );
                return Err(Error::internal(
                    "CUS_REGISTER_CALL_FAILED",
                    error.to_string(),
                ));
            }
        };

        let resp = serde_json::to_value(resp)
            .map_err(|e| Error::internal("CUS_RESP_SERIALIZE_FAILED", e.to_string()))?;

        if let Err(error) = self
            .state
            .user
            .upsert_user_data(UserDataUpsertInput {
                user_id: user_id.clone(),
                name: backend_model::kc::USER_DATA_NAME_REGISTRATION_OUTPUT.to_owned(),
                data_type: backend_model::kc::USER_DATA_TYPE_REGISTRATION_OUTPUT.to_owned(),
                content: resp.clone(),
                eager_fetch: true,
            })
            .await
        {
            self.mark_attempt_failed(
                &job.attempt_id,
                json!({
                    "error": error.to_string(),
                }),
            )
            .await?;
            return Err(Error::internal(
                "SM_USER_DATA_SAVE_FAILED",
                error.to_string(),
            ));
        }

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
                attempt_id: next_attempt_id.clone(),
            })
            .await
            .map_err(|err| {
                warn!(
                    instance_id = %instance.id,
                    step_name = %STEP_DEPOSIT_APPROVE_AND_DEPOSIT,
                    attempt_id = %next_attempt_id,
                    error = %err,
                    "SM enqueue failed for deposit approve and deposit step"
                );
                Error::internal("SM_ENQUEUE_FAILED", err.to_string())
            })?;

        Ok(())
    }

    pub async fn run_deposit_approve_and_deposit(
        &self,
        instance: backend_model::db::SmInstanceRow,
        job: StateMachineStepJob,
    ) -> Result<(), Error> {
        use gen_oas_client_cuss::apis::Error as CussApiError;
        use gen_oas_client_cuss::apis::registration_api;
        use gen_oas_client_cuss::models::ApproveAndDepositRequest;

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
                warn!(
                    attempt_id = %job.attempt_id,
                    savings_account_id = %savings_account_id,
                    status = %response_error.status,
                    "CUSS approve and deposit failed with API error response"
                );
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
                warn!(
                    attempt_id = %job.attempt_id,
                    savings_account_id = %savings_account_id,
                    error = %error,
                    "CUSS approve and deposit call failed"
                );
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
