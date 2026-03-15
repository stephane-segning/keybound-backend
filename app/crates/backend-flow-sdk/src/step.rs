use crate::{Actor, FlowError, StepContext};
use async_trait::async_trait;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum StepOutcome {
    Done,
    Waiting { actor: Actor },
    Failed { error: String, retryable: bool },
    Retry { after: Duration },
}

#[async_trait]
pub trait Step: Send + Sync + 'static {
    fn step_type(&self) -> &'static str;
    fn actor(&self) -> Actor;
    fn human_id(&self) -> &'static str;

    fn feature(&self) -> Option<&'static str> {
        None
    }

    async fn execute(&self, _ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        Ok(StepOutcome::Done)
    }

    async fn validate_input(&self, _input: &Value) -> Result<(), FlowError> {
        Ok(())
    }
}
