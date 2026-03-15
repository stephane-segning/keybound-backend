use async_trait::async_trait;
use backend_flow_sdk::flow::StepRef;
use backend_flow_sdk::{Actor, FlowError, Step, StepContext, StepOutcome};
use serde_json::Value;
use std::sync::Arc;

pub fn steps() -> Vec<StepRef> {
    vec![
        Arc::new(ReviewUserAccountStep),
        Arc::new(ApplyUserDecisionStep),
    ]
}

pub struct ReviewUserAccountStep;

#[async_trait]
impl Step for ReviewUserAccountStep {
    fn step_type(&self) -> &'static str {
        "REVIEW_USER_ACCOUNT"
    }

    fn actor(&self) -> Actor {
        Actor::Admin
    }

    fn human_id(&self) -> &'static str {
        "review"
    }

    fn feature(&self) -> Option<&'static str> {
        Some("flow-admin-user-management")
    }

    async fn execute(&self, _ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        Ok(StepOutcome::Waiting {
            actor: Actor::Admin,
        })
    }

    async fn validate_input(&self, input: &Value) -> Result<(), FlowError> {
        let object = input.as_object().ok_or_else(|| {
            FlowError::InvalidDefinition("REVIEW_USER_ACCOUNT expects object input".to_owned())
        })?;

        let decision = object
            .get("decision")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if decision.is_empty() {
            return Err(FlowError::InvalidDefinition(
                "REVIEW_USER_ACCOUNT requires decision".to_owned(),
            ));
        }

        Ok(())
    }
}

pub struct ApplyUserDecisionStep;

#[async_trait]
impl Step for ApplyUserDecisionStep {
    fn step_type(&self) -> &'static str {
        "APPLY_USER_DECISION"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "apply"
    }

    fn feature(&self) -> Option<&'static str> {
        Some("flow-admin-user-management")
    }

    async fn execute(&self, _ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        Ok(StepOutcome::Done)
    }
}
