use crate::{Actor, FlowError, StepContext};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct ContextUpdates {
    pub session_context_patch: Option<Value>,
    pub flow_context_patch: Option<Value>,
    pub user_metadata_patch: Option<Value>,
    pub user_metadata_eager_patch: Option<Value>,
    pub notifications: Option<Vec<Value>>,
}

#[derive(Debug, Clone)]
pub enum StepOutcome {
    Done {
        output: Option<Value>,
        updates: Option<Box<ContextUpdates>>,
    },
    Branched {
        branch: String,
        output: Option<Value>,
        updates: Option<Box<ContextUpdates>>,
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
        tracing::debug!(step = self.step_type(), "Executing step");
        Ok(StepOutcome::Done {
            output: None,
            updates: None,
        })
    }

    async fn validate_input(&self, _input: &Value) -> Result<(), FlowError> {
        Ok(())
    }

    async fn verify_input(
        &self,
        _ctx: &StepContext,
        _input: &Value,
    ) -> Result<StepOutcome, FlowError> {
        Ok(StepOutcome::Done {
            output: Some(json!({"verified": true})),
            updates: None,
        })
    }
}
