use crate::state::AppState;
use backend_core::Error;
use backend_flow_sdk::{RetryConfig, StepContext, StepOutcome, StepServices};
use backend_repository::FlowStepPatch;
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, instrument, warn};

pub struct FlowExecutor {
    state: Arc<AppState>,
}

impl FlowExecutor {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    #[instrument(skip(self, step))]
    pub async fn process_flow_step(
        &self,
        step: backend_model::db::FlowStepRow,
    ) -> Result<(), Error> {
        debug!(
            "Processing step: {} for flow: {} (attempt {})",
            step.step_type, step.flow_id, step.attempt_no
        );
        let flow = self
            .state
            .flow
            .get_flow(&step.flow_id)
            .await?
            .ok_or_else(|| Error::internal("FLOW_NOT_FOUND", "Flow not found for step"))?;

        let session = self
            .state
            .flow
            .get_session(&flow.session_id)
            .await?
            .ok_or_else(|| Error::internal("SESSION_NOT_FOUND", "Session not found for flow"))?;

        let flow_def = self
            .state
            .flow_registry
            .get_flow(&flow.flow_type)
            .ok_or_else(|| {
                Error::internal(
                    "UNKNOWN_FLOW_TYPE",
                    format!("Unknown flow type: {}", flow.flow_type),
                )
            })?;

        let flow_definition = self.state.flow_registry.get_flow_definition(&flow.flow_type);
        let retry_config = flow_definition
            .map(|fd| fd.get_step_retry_config(&step.step_type))
            .unwrap_or_default();

        let step_def = flow_def
            .steps()
            .iter()
            .find(|s| s.step_type() == step.step_type)
            .ok_or_else(|| {
                Error::internal(
                    "UNKNOWN_STEP_TYPE",
                    format!("Unknown step type: {}", step.step_type),
                )
            })?;

        let context = StepContext {
            session_id: session.id.clone(),
            flow_id: flow.id.clone(),
            step_id: step.id.clone(),
            input: step.input.clone().unwrap_or_else(|| serde_json::json!({})),
            session_context: session.context.clone(),
            flow_context: flow.context.clone(),
            services: StepServices::default(),
        };

        let outcome = step_def
            .execute(&context)
            .await
            .map_err(|e| Error::internal("STEP_EXECUTION_FAILED", e.to_string()))?;

        match outcome {
            StepOutcome::Done { output, updates } => {
                self.handle_done(&step, &flow, &session, output, updates, flow_def)
                    .await?;
            }
            StepOutcome::Waiting { .. } => {
                debug!("Step execution waiting: {}", step.step_type);
                self.state
                    .flow
                    .patch_step(&step.id, FlowStepPatch::new().status("WAITING"))
                    .await?;
            }
            StepOutcome::Failed { error, retryable } => {
                self.handle_failed(&step, &flow, &session, error, retryable, &retry_config)
                    .await?;
            }
            StepOutcome::Retry { after } => {
                self.handle_retry(&step, &flow, after, &retry_config).await?;
            }
        }

        Ok(())
    }

    async fn handle_done(
        &self,
        step: &backend_model::db::FlowStepRow,
        flow: &backend_model::db::FlowInstanceRow,
        session: &backend_model::db::FlowSessionRow,
        output: Option<Value>,
        updates: Option<Box<backend_flow_sdk::ContextUpdates>>,
        flow_def: &dyn backend_flow_sdk::Flow,
    ) -> Result<(), Error> {
        debug!("Step execution done: {}", step.step_type);
        let actual_output = output.unwrap_or_else(|| serde_json::json!({"result": "done"}));

        self.state
            .flow
            .patch_step(
                &step.id,
                FlowStepPatch::new()
                    .status("COMPLETED")
                    .output(actual_output.clone())
                    .clear_error()
                    .finished_at(Utc::now()),
            )
            .await?;

        let mut next_flow_context = flow.context.clone();
        if let Some(root) = next_flow_context.as_object_mut() {
            let entry = root
                .entry("step_output")
                .or_insert_with(|| Value::Object(Default::default()));
            if let Some(step_map) = entry.as_object_mut() {
                step_map.insert(step.step_type.clone(), actual_output);
            }
        }

        if let Some(updates) = updates {
            self.apply_updates(session, &mut next_flow_context, *updates)
                .await?;
        }

        self.advance_to_next_step(flow, session, step, flow_def, next_flow_context)
            .await?;
        Ok(())
    }

    async fn apply_updates(
        &self,
        session: &backend_model::db::FlowSessionRow,
        next_flow_context: &mut Value,
        updates: backend_flow_sdk::ContextUpdates,
    ) -> Result<(), Error> {
        if let Some(flow_patch) = updates.flow_context_patch
            && let (Some(base_obj), Some(patch_obj)) =
                (next_flow_context.as_object_mut(), flow_patch.as_object())
            {
                for (k, v) in patch_obj {
                    if v.is_null() {
                        base_obj.remove(k);
                    } else {
                        base_obj.insert(k.clone(), v.clone());
                    }
                }
            }

        if let Some(session_patch) = updates.session_context_patch {
            let mut new_session_context = session.context.clone();
            if let (Some(base_obj), Some(patch_obj)) =
                (new_session_context.as_object_mut(), session_patch.as_object())
            {
                for (k, v) in patch_obj {
                    if v.is_null() {
                        base_obj.remove(k);
                    } else {
                        base_obj.insert(k.clone(), v.clone());
                    }
                }
            }
            self.state
                .flow
                .update_session_context(&session.id, new_session_context)
                .await?;
        }

        if let Some(metadata_patch) = updates.user_metadata_patch
            && let Some(user_id) = session.user_id.as_deref()
        {
            self.state
                .user
                .update_metadata(user_id, metadata_patch)
                .await?;
        }

        if let Some(notifications) = updates.notifications {
            for notification in notifications {
                match serde_json::from_value::<backend_core::NotificationJob>(notification.clone()) {
                    Ok(job) => {
                        if let Err(e) = self.state.notification_queue.enqueue(job).await {
                            warn!("Failed to enqueue notification: {}", e);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to deserialize notification job: {}", e);
                    }
                }
            }
        }
        Ok(())
    }

    async fn advance_to_next_step(
        &self,
        flow: &backend_model::db::FlowInstanceRow,
        session: &backend_model::db::FlowSessionRow,
        step: &backend_model::db::FlowStepRow,
        flow_def: &dyn backend_flow_sdk::Flow,
        next_flow_context: Value,
    ) -> Result<(), Error> {
        if let Some(next_step_type) = flow_def.find_next_step(&step.step_type) {
            if next_step_type != "FAILED" && next_step_type != "END" {
                let next_step_def = flow_def
                    .steps()
                    .iter()
                    .find(|s| s.step_type() == next_step_type)
                    .ok_or_else(|| {
                        Error::internal(
                            "NEXT_STEP_NOT_FOUND",
                            format!("Next step not found: {}", next_step_type),
                        )
                    })?;

                let next_step_id = backend_id::flow_step_id()?;
                let mut updated_step_ids = flow.step_ids.clone();
                if let Some(arr) = updated_step_ids.as_array_mut() {
                    arr.push(serde_json::json!({ "id": &next_step_id, "type": &next_step_type }));
                }

                self.state
                    .flow
                    .create_step(backend_repository::FlowStepCreateInput {
                        id: next_step_id,
                        human_id: step.human_id.clone(),
                        flow_id: flow.id.clone(),
                        step_type: next_step_type.to_string(),
                        actor: next_step_def.actor().to_string(),
                        status: "WAITING".to_owned(),
                        attempt_no: 1,
                        input: None,
                        output: None,
                        error: None,
                        next_retry_at: None,
                        finished_at: None,
                    })
                    .await?;

                self.state
                    .flow
                    .update_flow(
                        &flow.id,
                        Some("RUNNING".to_owned()),
                        Some(Some(next_step_type.to_string())),
                        Some(updated_step_ids),
                        Some(next_flow_context),
                    )
                    .await?;
            } else {
                let final_status = if next_step_type.eq_ignore_ascii_case("FAILED") {
                    "FAILED"
                } else {
                    "COMPLETED"
                };
                self.finalize_flow(&flow.id, Some(&session.id), final_status)
                    .await?;
            }
        } else {
            self.finalize_flow(&flow.id, Some(&session.id), "COMPLETED")
                .await?;
        }
        Ok(())
    }

    async fn handle_failed(
        &self,
        step: &backend_model::db::FlowStepRow,
        flow: &backend_model::db::FlowInstanceRow,
        session: &backend_model::db::FlowSessionRow,
        error: String,
        retryable: bool,
        retry_config: &RetryConfig,
    ) -> Result<(), Error> {
        info!(
            "Step execution failed: {} (error={}, retryable={}, attempt={}/{})",
            step.step_type, error, retryable, step.attempt_no, retry_config.max
        );

        if retryable && step.attempt_no < retry_config.max {
            let delay = Duration::from_millis(retry_config.delay_ms);
            self.schedule_retry(&step.id, step.attempt_no + 1, delay)
                .await?;
            debug!(
                "Scheduled retry {} for step {} after {}ms",
                step.attempt_no + 1,
                step.step_type,
                retry_config.delay_ms
            );
        } else {
            self.state
                .flow
                .patch_step(
                    &step.id,
                    FlowStepPatch::new()
                        .status("FAILED")
                        .error(serde_json::json!({"error": error, "retryable": retryable, "attempts": step.attempt_no}))
                        .finished_at(Utc::now()),
                )
                .await?;
            self.finalize_flow(&flow.id, Some(&session.id), "FAILED").await?;
        }
        Ok(())
    }

    async fn handle_retry(
        &self,
        step: &backend_model::db::FlowStepRow,
        flow: &backend_model::db::FlowInstanceRow,
        after: Duration,
        retry_config: &RetryConfig,
    ) -> Result<(), Error> {
        debug!(
            "Step execution retry requested: {} (after={:?}, attempt={}/{})",
            step.step_type, after, step.attempt_no, retry_config.max
        );

        if step.attempt_no < retry_config.max {
            let delay = after.max(Duration::from_millis(retry_config.delay_ms));
            self.schedule_retry(&step.id, step.attempt_no + 1, delay)
                .await?;
        } else {
            warn!(
                "Max retries exceeded for step {} (attempts={})",
                step.step_type, step.attempt_no
            );
            self.state
                .flow
                .patch_step(
                    &step.id,
                    FlowStepPatch::new()
                        .status("FAILED")
                        .error(serde_json::json!({"error": "max_retries_exceeded", "attempts": step.attempt_no}))
                        .finished_at(Utc::now()),
                )
                .await?;
            self.finalize_flow(&flow.id, None, "FAILED").await?;
        }
        Ok(())
    }

    async fn schedule_retry(
        &self,
        step_id: &str,
        next_attempt: i32,
        delay: Duration,
    ) -> Result<(), Error> {
        use chrono::Duration as ChronoDuration;
        let next_retry_at = Utc::now()
            + ChronoDuration::from_std(delay).unwrap_or_else(|_| ChronoDuration::seconds(1));

        self.state
            .flow
            .patch_step(
                step_id,
                FlowStepPatch::new()
                    .status("RETRY")
                    .attempt_no(next_attempt)
                    .next_retry_at(next_retry_at),
            )
            .await?;
        Ok(())
    }

    async fn finalize_flow(
        &self,
        flow_id: &str,
        session_id: Option<&str>,
        status: &str,
    ) -> Result<(), Error> {
        self.state
            .flow
            .update_flow(flow_id, Some(status.to_owned()), Some(None), None, None)
            .await?;
        if let Some(sid) = session_id {
            self.state
                .flow
                .update_session_status(sid, status, Some(Utc::now()))
                .await?;
        }
        Ok(())
    }
}
