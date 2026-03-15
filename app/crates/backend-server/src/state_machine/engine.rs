use crate::state::AppState;
use crate::state_machine::deposit::DepositEngine;
use crate::state_machine::event::EventEmitter;
use crate::state_machine::instance::InstanceManager;
use crate::state_machine::jobs::StateMachineStepJob;
use crate::state_machine::manual_step::ManualStepManager;
use crate::state_machine::phone_otp::{PhoneIssueOtpOutcome, PhoneOtpEngine};
use crate::state_machine::types::*;
use backend_core::Error;
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;

pub struct Engine {
    state: Arc<AppState>,
    instances: InstanceManager,
    events: EventEmitter,
    phone_otp: PhoneOtpEngine,
    deposit: DepositEngine,
    manual_steps: ManualStepManager,
}

impl Engine {
    pub fn new(state: Arc<AppState>) -> Self {
        let instances = InstanceManager::new(state.clone());
        let events = EventEmitter::new(state.clone());
        let phone_otp = PhoneOtpEngine::new(state.clone());
        let deposit = DepositEngine::new(state.clone());
        let manual_steps = ManualStepManager::new(state.clone());
        Self {
            state,
            instances,
            events,
            phone_otp,
            deposit,
            manual_steps,
        }
    }

    pub async fn ensure_active_instance(
        &self,
        kind: &str,
        user_id: Option<String>,
        idempotency_key: String,
        context: Value,
    ) -> Result<backend_model::db::SmInstanceRow, Error> {
        self.instances
            .ensure_active_instance(kind, user_id, idempotency_key, context)
            .await
    }

    pub async fn emit_event(
        &self,
        instance_id: &str,
        kind: &str,
        actor_type: ActorType,
        actor_id: Option<String>,
        payload: Value,
    ) -> Result<(), Error> {
        self.events
            .emit_event(instance_id, kind, actor_type, actor_id, payload)
            .await
    }

    pub async fn enqueue_phone_issue_otp(
        &self,
        instance_id: &str,
        msisdn: &str,
        channel: &str,
        ttl_seconds: i64,
    ) -> Result<(String, chrono::DateTime<Utc>, i32), Error> {
        self.phone_otp
            .enqueue_phone_issue_otp(instance_id, msisdn, channel, ttl_seconds)
            .await
    }

    pub async fn staff_confirm_deposit_payment(
        &self,
        instance_id: &str,
        staff_user_id: Option<String>,
        payload: Value,
    ) -> Result<(), Error> {
        self.deposit
            .staff_confirm_deposit_payment(instance_id, staff_user_id, payload)
            .await
    }

    pub async fn staff_approve_deposit(
        &self,
        instance_id: &str,
        staff_user_id: Option<String>,
        payload: Value,
    ) -> Result<(), Error> {
        self.deposit
            .staff_approve_deposit(instance_id, staff_user_id, payload)
            .await
    }

    pub async fn ensure_manual_step_running(
        &self,
        instance_id: &str,
        step_name: &str,
    ) -> Result<(), Error> {
        self.manual_steps
            .ensure_manual_step_running(instance_id, step_name)
            .await
    }

    pub async fn mark_manual_step_succeeded(
        &self,
        instance_id: &str,
        step_name: &str,
    ) -> Result<(), Error> {
        self.manual_steps
            .mark_manual_step_succeeded(instance_id, step_name)
            .await
    }

    pub async fn process_step_job(&self, job: StateMachineStepJob) -> Result<(), Error> {
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
                let outcome = self
                    .phone_otp
                    .run_phone_issue_otp(job, claimed_attempt)
                    .await?;
                if matches!(outcome, PhoneIssueOtpOutcome::Succeeded) {
                    self.ensure_manual_step_running(&instance.id, STEP_PHONE_VERIFY_OTP)
                        .await?;
                }
            }
            (KIND_KYC_FIRST_DEPOSIT, STEP_DEPOSIT_REGISTER_CUSTOMER) => {
                self.deposit
                    .run_deposit_register_customer(instance, job)
                    .await?;
            }
            (KIND_KYC_FIRST_DEPOSIT, STEP_DEPOSIT_APPROVE_AND_DEPOSIT) => {
                self.deposit
                    .run_deposit_approve_and_deposit(instance, job)
                    .await?;
                self.instances.finish_instance_success(&instance_id).await?;
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
}
