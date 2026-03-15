use crate::state::AppState;
use crate::state_machine::types::*;
use backend_core::Error;
use backend_repository::{SmStepAttemptCreateInput, SmStepAttemptPatch};
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;

pub struct ManualStepManager {
    state: Arc<AppState>,
}

impl ManualStepManager {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
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
                SmStepAttemptPatch::new()
                    .status(ATTEMPT_STATUS_SUCCEEDED)
                    .clear_error()
                    .finished_at(now),
            )
            .await?;

        Ok(())
    }
}
