use crate::state::AppState;
use crate::state_machine::types::ActorType;
use backend_core::Error;
use backend_repository::SmEventCreateInput;
use serde_json::Value;
use std::sync::Arc;

pub struct EventEmitter {
    state: Arc<AppState>,
}

impl EventEmitter {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
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
}
