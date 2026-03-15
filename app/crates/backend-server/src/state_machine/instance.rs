use crate::state::AppState;
use crate::state_machine::types::*;
use backend_core::Error;
use backend_repository::SmInstanceCreateInput;
use serde_json::Value;
use std::sync::Arc;

pub struct InstanceManager {
    state: Arc<AppState>,
}

impl InstanceManager {
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

    pub async fn finish_instance_success(&self, instance_id: &str) -> Result<(), Error> {
        let now = chrono::Utc::now();
        let mark_complete_attempt = self
            .state
            .sm
            .create_step_attempt(backend_repository::SmStepAttemptCreateInput {
                id: backend_id::sm_attempt_id()?,
                instance_id: instance_id.to_owned(),
                step_name: STEP_MARK_COMPLETE.to_owned(),
                attempt_no: 1,
                status: ATTEMPT_STATUS_SUCCEEDED.to_owned(),
                external_ref: None,
                input: serde_json::json!({}),
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
                backend_core::Error::Diesel(diesel::result::Error::DatabaseError(
                    diesel::result::DatabaseErrorKind::UniqueViolation,
                    _,
                )) => {}
                _ => return Err(error),
            }
        }

        self.state
            .sm
            .update_instance_status(instance_id, INSTANCE_STATUS_COMPLETED, Some(now))
            .await?;
        Ok(())
    }
}
