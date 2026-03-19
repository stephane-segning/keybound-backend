use crate::step::ContextUpdates;
use crate::{Actor, FlowError, Step, StepContext, StepOutcome};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhoneSource {
    #[default]
    Session,
    Flow,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdatePhoneNumberConfig {
    #[serde(default)]
    pub source: PhoneSource,
    #[serde(default = "default_source_path")]
    pub source_path: String,
    #[serde(default = "default_update_user")]
    pub update_user: bool,
    #[serde(default = "default_save_to_session")]
    pub save_to_session: bool,
}

fn default_source_path() -> String {
    "/phone_number".to_owned()
}

fn default_update_user() -> bool {
    true
}

fn default_save_to_session() -> bool {
    true
}

impl Default for UpdatePhoneNumberConfig {
    fn default() -> Self {
        Self {
            source: PhoneSource::Session,
            source_path: default_source_path(),
            update_user: default_update_user(),
            save_to_session: default_save_to_session(),
        }
    }
}

pub struct UpdatePhoneNumberAction;

#[async_trait]
impl Step for UpdatePhoneNumberAction {
    fn step_type(&self) -> &'static str {
        "update-phone-number"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "update_phone_number"
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        tracing::debug!(step = self.step_type(), "Executing step");
        let config: UpdatePhoneNumberConfig = super::parse_step_config(ctx)?;

        let phone = resolve_phone_number(ctx, &config)?;
        if config.update_user {
            let user_id = ctx.session_user_id.as_deref().ok_or_else(|| {
                FlowError::InvalidDefinition(
                    "update-phone-number requires session user id".to_owned(),
                )
            })?;

            let service = ctx.services.user_contact.as_ref().ok_or_else(|| {
                FlowError::InvalidDefinition(
                    "update-phone-number requires user contact service".to_owned(),
                )
            })?;

            service
                .update_phone_number(user_id, &phone)
                .await
                .map_err(FlowError::InvalidDefinition)?;
        }

        Ok(StepOutcome::Done {
            output: Some(json!({
                "phone_number": phone,
                "user_updated": config.update_user
            })),
            updates: Some(Box::new(ContextUpdates {
                flow_context_patch: Some(json!({
                    "phone_number": phone
                })),
                session_context_patch: config
                    .save_to_session
                    .then(|| json!({ "phone_number": phone })),
                ..Default::default()
            })),
        })
    }
}

fn resolve_phone_number(
    ctx: &StepContext,
    config: &UpdatePhoneNumberConfig,
) -> Result<String, FlowError> {
    let raw = match config.source {
        PhoneSource::Session => ctx.session_pointer(&config.source_path),
        PhoneSource::Flow => ctx.flow_pointer(&config.source_path),
    };

    let phone = raw
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| {
            FlowError::InvalidDefinition(format!(
                "Phone number not found at {:?} {}",
                config.source, config.source_path
            ))
        })?;

    Ok(phone.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StepServices, UserContactService};
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Default)]
    struct TestContactService {
        calls: Arc<Mutex<Vec<(String, String)>>>,
    }

    #[async_trait]
    impl UserContactService for TestContactService {
        async fn update_phone_number(
            &self,
            user_id: &str,
            phone_number: &str,
        ) -> Result<(), String> {
            let mut calls = self.calls.lock().map_err(|_| "lock poisoned".to_owned())?;
            calls.push((user_id.to_owned(), phone_number.to_owned()));
            Ok(())
        }

        async fn update_full_name(&self, _user_id: &str, _full_name: &str) -> Result<(), String> {
            Ok(())
        }
    }

    fn ctx(
        config: HashMap<String, serde_json::Value>,
        user_contact: Option<Arc<dyn UserContactService>>,
    ) -> StepContext {
        StepContext {
            session_id: "sess-1".to_owned(),
            session_user_id: Some("usr-1".to_owned()),
            flow_id: "flow-1".to_owned(),
            step_id: "step-1".to_owned(),
            input: json!({}),
            session_context: json!({ "phone_number": "+237690000001" }),
            flow_context: json!({}),
            services: StepServices {
                config: Some(config),
                user_contact,
                ..Default::default()
            },
        }
    }

    #[tokio::test]
    async fn updates_user_phone_and_flow_context() {
        let action = UpdatePhoneNumberAction;
        let tracker = Arc::new(TestContactService::default());

        let outcome = action
            .execute(&ctx(HashMap::new(), Some(tracker.clone())))
            .await
            .unwrap();

        match outcome {
            StepOutcome::Done { updates, output } => {
                let output = output.unwrap();
                assert_eq!(output["phone_number"], "+237690000001");
                assert_eq!(output["user_updated"], true);

                let updates = updates.unwrap();
                assert_eq!(
                    updates.flow_context_patch.unwrap()["phone_number"],
                    "+237690000001"
                );
            }
            _ => panic!("expected done outcome"),
        }

        let calls = tracker.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "usr-1");
        assert_eq!(calls[0].1, "+237690000001");
    }

    #[tokio::test]
    async fn can_skip_user_update_and_only_cache_flow_phone() {
        let action = UpdatePhoneNumberAction;
        let tracker = Arc::new(TestContactService::default());

        let mut config = HashMap::new();
        config.insert("update_user".to_owned(), json!(false));

        let outcome = action
            .execute(&ctx(config, Some(tracker.clone())))
            .await
            .unwrap();

        match outcome {
            StepOutcome::Done { output, .. } => {
                assert_eq!(output.unwrap()["user_updated"], false);
            }
            _ => panic!("expected done outcome"),
        }

        let calls = tracker.calls.lock().unwrap();
        assert!(calls.is_empty());
    }
}
