use crate::{Actor, FlowError, StepContext};
use async_trait::async_trait;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct ContextUpdates {
    pub session_context_patch: Option<Value>,
    pub flow_context_patch: Option<Value>,
    pub user_metadata_patch: Option<Value>,
}

#[derive(Debug, Clone)]
pub enum StepOutcome {
    Done {
        output: Option<Value>,
        updates: Option<ContextUpdates>,
    },
    Waiting {
        actor: Actor,
    },
    Failed {
        error: String,
        retryable: bool,
    },
    Retry {
        after: Duration,
    },
}

#[async_trait]
pub trait Step: Send + Sync + 'static {
    fn step_type(&self) -> &str;
    fn actor(&self) -> Actor;
    fn human_id(&self) -> &str;

    fn feature(&self) -> Option<&str> {
        None
    }

    async fn execute(&self, _ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        Ok(StepOutcome::Done {
            output: None,
            updates: None,
        })
    }

    async fn validate_input(&self, _input: &Value) -> Result<(), FlowError> {
        Ok(())
    }
}
