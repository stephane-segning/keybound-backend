use crate::state::AppState;
use backend_core::Error;
use backend_flow_sdk::{StepContext, StepOutcome};
use backend_repository::FlowStepPatch;
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;
use tracing::{debug, info, instrument};

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
            "Processing step: {} for flow: {}",
            step.step_type, step.flow_id
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
        };

        match step_def
            .execute(&context)
            .await
            .map_err(|e| Error::internal("STEP_EXECUTION_FAILED", e.to_string()))?
        {
            StepOutcome::Done { output, updates } => {
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
                        if let (Some(base_obj), Some(patch_obj)) = (
                            new_session_context.as_object_mut(),
                            session_patch.as_object(),
                        ) {
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
                        && let Some(user_id) = session.user_id.as_deref() {
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
                                        tracing::warn!("Failed to enqueue notification: {}", e);
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to deserialize notification job: {}", e);
                                }
                            }
                        }
                    }
                }

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
                            arr.push(
                                serde_json::json!({ "id": &next_step_id, "type": &next_step_type }),
                            );
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
                        self.state
                            .flow
                            .update_flow(
                                &flow.id,
                                Some(final_status.to_owned()),
                                Some(None),
                                None,
                                None,
                            )
                            .await?;
                        self.state
                            .flow
                            .update_session_status(&session.id, final_status, Some(Utc::now()))
                            .await?;
                    }
                } else {
                    self.state
                        .flow
                        .update_flow(
                            &flow.id,
                            Some("COMPLETED".to_owned()),
                            Some(None),
                            None,
                            None,
                        )
                        .await?;
                    self.state
                        .flow
                        .update_session_status(&session.id, "COMPLETED", Some(Utc::now()))
                        .await?;
                }
            }
            StepOutcome::Waiting { .. } => {
                debug!("Step execution waiting: {}", step.step_type);
                self.state
                    .flow
                    .patch_step(&step.id, FlowStepPatch::new().status("WAITING"))
                    .await?;
            }
            StepOutcome::Failed { error, retryable } => {
                info!(
                    "Step execution failed: {} (error={}, retryable={})",
                    step.step_type, error, retryable
                );
                self.state
                    .flow
                    .patch_step(
                        &step.id,
                        FlowStepPatch::new()
                            .status("FAILED")
                            .error(serde_json::json!({"error": error, "retryable": retryable}))
                            .finished_at(Utc::now()),
                    )
                    .await?;
                self.state
                    .flow
                    .update_flow(&flow.id, Some("FAILED".to_owned()), Some(None), None, None)
                    .await?;
                self.state
                    .flow
                    .update_session_status(&session.id, "FAILED", Some(Utc::now()))
                    .await?;
            }
            StepOutcome::Retry { after } => {
                debug!(
                    "Step execution retry: {} (after={:?})",
                    step.step_type, after
                );
                use chrono::Duration;
                self.state
                    .flow
                    .patch_step(
                        &step.id,
                        FlowStepPatch::new().status("WAITING").next_retry_at(
                            Utc::now()
                                + Duration::from_std(after)
                                    .unwrap_or_else(|_| Duration::seconds(0)),
                        ),
                    )
                    .await?;
            }
        }

        Ok(())
    }
}
