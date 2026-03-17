use async_trait::async_trait;
use backend_flow_sdk::flow::StepRef;
use backend_flow_sdk::{Actor, FlowError, Step, StepContext, StepOutcome};
use serde_json::Value;
use std::sync::Arc;

pub fn steps() -> Vec<StepRef> {
    vec![Arc::new(BindDeviceStep), Arc::new(ActivateDeviceStep)]
}

pub struct BindDeviceStep;

#[async_trait]
impl Step for BindDeviceStep {
    fn step_type(&self) -> &'static str {
        "BIND_DEVICE"
    }

    fn actor(&self) -> Actor {
        Actor::EndUser
    }

    fn human_id(&self) -> &'static str {
        "bind"
    }

    fn feature(&self) -> Option<&'static str> {
        Some("flow-device-enroll")
    }

    async fn execute(&self, _ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        Ok(StepOutcome::Waiting {
            actor: Actor::EndUser,
        })
    }

    async fn validate_input(&self, input: &Value) -> Result<(), FlowError> {
        let object = input.as_object().ok_or_else(|| {
            FlowError::InvalidDefinition("BIND_DEVICE expects object input".to_owned())
        })?;

        let has_device_id = object
            .get("device_id")
            .or_else(|| object.get("deviceId"))
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty());
        if !has_device_id {
            return Err(FlowError::InvalidDefinition(
                "BIND_DEVICE requires device_id".to_owned(),
            ));
        }

        Ok(())
    }
}

pub struct ActivateDeviceStep;

#[async_trait]
impl Step for ActivateDeviceStep {
    fn step_type(&self) -> &'static str {
        "ACTIVATE_DEVICE"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "activate"
    }

    fn feature(&self) -> Option<&'static str> {
        Some("flow-device-enroll")
    }

    async fn execute(&self, _ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        Ok(StepOutcome::Done {
            output: None,
            updates: None,
        })
    }
}
