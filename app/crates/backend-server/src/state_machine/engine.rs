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

    pub async fn process_flow_step(&self, step: backend_model::db::FlowStepRow) -> Result<(), Error> {
        use backend_flow_sdk::{StepContext, StepOutcome};
        use backend_repository::FlowStepPatch;

        let flow = self.state.flow.get_flow(&step.flow_id).await?
            .ok_or_else(|| Error::internal("FLOW_NOT_FOUND", "Flow not found for step"))?;

        let session = self.state.flow.get_session(&flow.session_id).await?
            .ok_or_else(|| Error::internal("SESSION_NOT_FOUND", "Session not found for flow"))?;

        let flow_def = self.state.flow_registry.get_flow(&flow.flow_type)
            .ok_or_else(|| Error::internal("UNKNOWN_FLOW_TYPE", format!("Unknown flow type: {}", flow.flow_type)))?;

        let step_def = flow_def.steps().iter()
            .find(|s| s.step_type() == step.step_type)
            .ok_or_else(|| Error::internal("UNKNOWN_STEP_TYPE", format!("Unknown step type: {}", step.step_type)))?;

        let context = StepContext {
            session_id: session.id.clone(),
            flow_id: flow.id.clone(),
            step_id: step.id.clone(),
            input: step.input.clone().unwrap_or_else(|| serde_json::json!({})),
            session_context: session.context.clone(),
            flow_context: flow.context.clone(),
        };

        match step_def.execute(&context).await.map_err(|e| Error::internal("STEP_EXECUTION_FAILED", e.to_string()))? {
            StepOutcome::Done { output, updates } => {
                let actual_output = output.unwrap_or_else(|| serde_json::json!({"result": "done"}));
                
                self.state.flow.patch_step(
                    &step.id,
                    FlowStepPatch::new()
                        .status("COMPLETED")
                        .output(actual_output.clone())
                        .clear_error()
                        .finished_at(Utc::now()),
                ).await?;

                let mut next_flow_context = flow.context.clone();
                if let Some(root) = next_flow_context.as_object_mut() {
                    let entry = root.entry("step_output").or_insert_with(|| Value::Object(Default::default()));
                    if let Some(step_map) = entry.as_object_mut() {
                        step_map.insert(step.step_type.clone(), actual_output);
                    }
                }

                if let Some(updates) = updates {
                    if let Some(flow_patch) = updates.flow_context_patch {
                        if let (Some(base_obj), Some(patch_obj)) = (next_flow_context.as_object_mut(), flow_patch.as_object()) {
                            for (k, v) in patch_obj {
                                if v.is_null() { base_obj.remove(k); } else { base_obj.insert(k.clone(), v.clone()); }
                            }
                        }
                    }
                    if let Some(session_patch) = updates.session_context_patch {
                        let mut new_session_context = session.context.clone();
                        if let (Some(base_obj), Some(patch_obj)) = (new_session_context.as_object_mut(), session_patch.as_object()) {
                            for (k, v) in patch_obj {
                                if v.is_null() { base_obj.remove(k); } else { base_obj.insert(k.clone(), v.clone()); }
                            }
                        }
                        self.state.flow.update_session_context(&session.id, new_session_context).await?;
                    }
                    if let Some(metadata_patch) = updates.user_metadata_patch {
                        if let Some(user_id) = session.user_id.as_deref() {
                            self.state.user.update_metadata(user_id, metadata_patch).await?;
                        }
                    }
                }

                let mut updated_flow = self.state.flow.update_flow(&flow.id, None, None, None, Some(next_flow_context)).await?;

                if let Some(transition) = flow_def.transitions().get(&step.step_type) {
                    let next_step_type = &transition.on_success;
                    let has_next_step = flow_def.steps().iter().any(|s| s.step_type() == next_step_type);
                    
                    if has_next_step {
                        use crate::flow_registry::{actor_label, waiting_status};
                        use backend_repository::FlowStepCreateInput;
                        use backend_flow_sdk::HumanReadableId;

                        let next_step_def = flow_def.steps().iter().find(|s| s.step_type() == next_step_type).unwrap();
                        let step_id = backend_id::flow_step_id()?;
                        
                        let step_human_id = HumanReadableId::parse(flow.human_id.clone())
                            .map_err(|e| Error::internal("INVALID_HUMAN_ID", e.to_string()))?
                            .with_suffix(next_step_def.human_id())
                            .map_err(|e| Error::internal("INVALID_HUMAN_ID", e.to_string()))?
                            .to_string();

                        let created_step = self.state.flow.create_step(FlowStepCreateInput {
                            id: step_id.clone(),
                            human_id: step_human_id,
                            flow_id: flow.id.clone(),
                            step_type: next_step_type.clone(),
                            actor: actor_label(next_step_def.actor()).to_owned(),
                            status: waiting_status(next_step_def.actor()).to_owned(),
                            attempt_no: 0,
                            input: None,
                            output: None,
                            error: None,
                            next_retry_at: None,
                            finished_at: None,
                        }).await?;

                        let mut values = flow.step_ids.as_array().cloned().unwrap_or_default();
                        values.push(serde_json::Value::String(created_step.id.clone()));
                        let updated_step_ids = serde_json::Value::Array(values);

                        self.state.flow.update_flow(
                            &flow.id,
                            Some("RUNNING".to_owned()),
                            Some(Some(next_step_type.clone())),
                            Some(updated_step_ids),
                            None
                        ).await?;
                    } else {
                        let final_status = if next_step_type.eq_ignore_ascii_case("FAILED") { "FAILED" } else { "COMPLETED" };
                        self.state.flow.update_flow(&flow.id, Some(final_status.to_owned()), Some(None), None, None).await?;
                        self.state.flow.update_session_status(&session.id, final_status, Some(Utc::now())).await?;
                    }
                } else {
                    self.state.flow.update_flow(&flow.id, Some("COMPLETED".to_owned()), Some(None), None, None).await?;
                    self.state.flow.update_session_status(&session.id, "COMPLETED", Some(Utc::now())).await?;
                }
            }
            StepOutcome::Waiting { .. } => {
                self.state.flow.patch_step(&step.id, FlowStepPatch::new().status("WAITING")).await?;
            }
            StepOutcome::Failed { error, retryable } => {
                self.state.flow.patch_step(
                    &step.id,
                    FlowStepPatch::new()
                        .status("FAILED")
                        .error(serde_json::json!({"error": error, "retryable": retryable}))
                        .finished_at(Utc::now()),
                ).await?;
                self.state.flow.update_flow(&flow.id, Some("FAILED".to_owned()), Some(None), None, None).await?;
                self.state.flow.update_session_status(&session.id, "FAILED", Some(Utc::now())).await?;
            }
            StepOutcome::Retry { after } => {
                use chrono::Duration;
                self.state.flow.patch_step(
                    &step.id,
                    FlowStepPatch::new()
                        .status("WAITING")
                        .next_retry_at(Utc::now() + Duration::from_std(after).unwrap_or_else(|_| Duration::seconds(0))),
                ).await?;
            }
        }

        Ok(())
    }
}
