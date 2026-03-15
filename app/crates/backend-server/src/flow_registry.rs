use backend_flow_sdk::flow::StepRef;
use backend_flow_sdk::{Actor, Flow, FlowError, FlowRegistry, SessionDefinition, StepTransition};
use std::collections::HashMap;
use std::sync::Arc;

use crate::flow_logic;

pub const SESSION_TYPE_KYC_FULL: &str = "KYC_FULL";
pub const SESSION_TYPE_ACCOUNT_MANAGEMENT: &str = "ACCOUNT_MANAGEMENT";
pub const SESSION_TYPE_ADMIN_OPERATIONS: &str = "ADMIN_OPERATIONS";

#[derive(Default, Clone)]
pub struct RegistryImports {
    pub flows: Vec<backend_flow_sdk::flow::FlowDefinition>,
    pub sessions: Vec<backend_flow_sdk::SessionDefinition>,
}

pub fn build_registry(imports: RegistryImports) -> Result<FlowRegistry, FlowError> {
    let mut registry = FlowRegistry::new();
    let mut kyc_allowed_flows = Vec::new();
    let mut account_allowed_flows = Vec::new();
    let mut admin_allowed_flows = Vec::new();

    #[cfg(feature = "flow-phone-otp")]
    {
        let flow = static_flow(
            "PHONE_OTP",
            "phone_otp",
            Some("flow-phone-otp"),
            "ISSUE_PHONE_OTP",
            flow_logic::phone_otp::steps(),
            &[
                ("ISSUE_PHONE_OTP", "VERIFY_PHONE_OTP", Some("FAILED")),
                ("VERIFY_PHONE_OTP", "COMPLETE", Some("FAILED")),
            ],
        );
        register_flow_bundle(&mut registry, flow);
        kyc_allowed_flows.push("PHONE_OTP".to_owned());
    }

    #[cfg(feature = "flow-email-magic")]
    {
        let flow = static_flow(
            "EMAIL_MAGIC",
            "email_magic",
            Some("flow-email-magic"),
            "ISSUE_MAGIC_EMAIL",
            flow_logic::email_magic::steps(),
            &[
                ("ISSUE_MAGIC_EMAIL", "VERIFY_MAGIC_EMAIL", Some("FAILED")),
                ("VERIFY_MAGIC_EMAIL", "COMPLETE", Some("FAILED")),
            ],
        );
        register_flow_bundle(&mut registry, flow);
        kyc_allowed_flows.push("EMAIL_MAGIC".to_owned());
    }

    #[cfg(feature = "flow-first-deposit")]
    {
        let flow = static_flow(
            "FIRST_DEPOSIT",
            "first_deposit",
            Some("flow-first-deposit"),
            "AWAIT_PAYMENT_CONFIRMATION",
            flow_logic::first_deposit::steps(),
            &[
                (
                    "AWAIT_PAYMENT_CONFIRMATION",
                    "APPROVE_AND_DEPOSIT",
                    Some("FAILED"),
                ),
                ("APPROVE_AND_DEPOSIT", "COMPLETE", Some("FAILED")),
            ],
        );
        register_flow_bundle(&mut registry, flow);
        kyc_allowed_flows.push("FIRST_DEPOSIT".to_owned());
    }

    #[cfg(feature = "flow-id-document")]
    {
        let flow = static_flow(
            "ID_DOCUMENT",
            "id_document",
            Some("flow-id-document"),
            "SUBMIT_ID_DOCUMENT",
            flow_logic::id_document::steps(),
            &[
                ("SUBMIT_ID_DOCUMENT", "REVIEW_ID_DOCUMENT", Some("FAILED")),
                ("REVIEW_ID_DOCUMENT", "COMPLETE", Some("FAILED")),
            ],
        );
        register_flow_bundle(&mut registry, flow);
        kyc_allowed_flows.push("ID_DOCUMENT".to_owned());
    }

    #[cfg(feature = "flow-address-proof")]
    {
        let flow = static_flow(
            "ADDRESS_PROOF",
            "address_proof",
            Some("flow-address-proof"),
            "SUBMIT_ADDRESS_PROOF",
            flow_logic::address_proof::steps(),
            &[
                (
                    "SUBMIT_ADDRESS_PROOF",
                    "REVIEW_ADDRESS_PROOF",
                    Some("FAILED"),
                ),
                ("REVIEW_ADDRESS_PROOF", "COMPLETE", Some("FAILED")),
            ],
        );
        register_flow_bundle(&mut registry, flow);
        kyc_allowed_flows.push("ADDRESS_PROOF".to_owned());
    }

    #[cfg(feature = "flow-external-kyc")]
    {
        let flow = static_flow(
            "EXTERNAL_KYC",
            "external_kyc",
            Some("flow-external-kyc"),
            "WEBHOOK_HTTP",
            flow_logic::external_kyc::steps(),
            &[("WEBHOOK_HTTP", "COMPLETE", Some("FAILED"))],
        );
        register_flow_bundle(&mut registry, flow);
        kyc_allowed_flows.push("EXTERNAL_KYC".to_owned());
    }

    #[cfg(feature = "flow-device-enroll")]
    {
        let flow = static_flow(
            "DEVICE_ENROLL",
            "device_enroll",
            Some("flow-device-enroll"),
            "BIND_DEVICE",
            flow_logic::device_enroll::steps(),
            &[
                ("BIND_DEVICE", "ACTIVATE_DEVICE", Some("FAILED")),
                ("ACTIVATE_DEVICE", "COMPLETE", Some("FAILED")),
            ],
        );
        register_flow_bundle(&mut registry, flow);
        account_allowed_flows.push("DEVICE_ENROLL".to_owned());
    }

    #[cfg(feature = "flow-account-update")]
    {
        let flow = static_flow(
            "ACCOUNT_UPDATE",
            "account_update",
            Some("flow-account-update"),
            "SUBMIT_ACCOUNT_UPDATE",
            flow_logic::account_update::steps(),
            &[
                (
                    "SUBMIT_ACCOUNT_UPDATE",
                    "APPLY_ACCOUNT_UPDATE",
                    Some("FAILED"),
                ),
                ("APPLY_ACCOUNT_UPDATE", "COMPLETE", Some("FAILED")),
            ],
        );
        register_flow_bundle(&mut registry, flow);
        account_allowed_flows.push("ACCOUNT_UPDATE".to_owned());
    }

    #[cfg(feature = "flow-admin-user-management")]
    {
        let flow = static_flow(
            "ADMIN_USER_MANAGEMENT",
            "admin_user_management",
            Some("flow-admin-user-management"),
            "REVIEW_USER_ACCOUNT",
            flow_logic::admin_user_management::steps(),
            &[
                ("REVIEW_USER_ACCOUNT", "APPLY_USER_DECISION", Some("FAILED")),
                ("APPLY_USER_DECISION", "COMPLETE", Some("FAILED")),
            ],
        );
        register_flow_bundle(&mut registry, flow);
        admin_allowed_flows.push("ADMIN_USER_MANAGEMENT".to_owned());
    }

    registry.register_session(SessionDefinition {
        session_type: SESSION_TYPE_KYC_FULL.to_owned(),
        human_id_prefix: "kyc".to_owned(),
        feature: None,
        allowed_flows: kyc_allowed_flows,
        override_existing: None,
    });

    registry.register_session(SessionDefinition {
        session_type: SESSION_TYPE_ACCOUNT_MANAGEMENT.to_owned(),
        human_id_prefix: "auth".to_owned(),
        feature: None,
        allowed_flows: account_allowed_flows,
        override_existing: None,
    });

    registry.register_session(SessionDefinition {
        session_type: SESSION_TYPE_ADMIN_OPERATIONS.to_owned(),
        human_id_prefix: "admin".to_owned(),
        feature: None,
        allowed_flows: admin_allowed_flows,
        override_existing: None,
    });

    for flow_def in imports.flows {
        apply_flow_import(&mut registry, flow_def)?;
    }

    for session_def in imports.sessions {
        apply_session_import(&mut registry, session_def)?;
    }

    Ok(registry)
}

pub fn apply_flow_import(
    registry: &mut FlowRegistry,
    definition: backend_flow_sdk::flow::FlowDefinition,
) -> Result<(), FlowError> {
    let flow_type = definition.metadata.flow_type.clone();

    if registry.get_flow(&flow_type).is_some()
        && !definition.metadata.override_existing.unwrap_or(false)
    {
        return Err(FlowError::InvalidDefinition(format!(
            "Flow '{}' already exists in registry and override_existing is false or missing",
            flow_type
        )));
    }

    let mut proxy_steps = Vec::new();
    let mut transitions = HashMap::new();

    let initial_step = definition
        .spec
        .steps
        .first()
        .map(|s| s.step_type.clone())
        .ok_or_else(|| {
            FlowError::InvalidDefinition(format!("Flow '{}' has no steps", flow_type))
        })?;

    for step_def in definition.spec.steps {
        let base_step = registry
            .get_step(&step_def.step_type)
            .ok_or_else(|| {
                FlowError::InvalidDefinition(format!(
                    "Flow '{}' references unknown step '{}'",
                    flow_type, step_def.step_type
                ))
            })?;

        // Extract a cloned Arc for the base step by relying on the fact that `FlowRegistry`
        // allows cloning step implementations when accessed properly. Wait, `get_step` returns `&dyn Step`.
        // If we can't get an Arc directly, we might need a workaround. But wait, we can just look up the steps
        // from the definitions. Wait! `get_step` returns a reference. How do we get the Arc?
        // We will need to change `FlowRegistry` to expose `get_step_arc`.
        // Let's assume we'll add `get_step_arc` to `FlowRegistry` next.
        let base_step_arc = registry.get_step_arc(&step_def.step_type).unwrap();

        let proxy = Arc::new(ProxyStep {
            step_type: step_def.step_type.clone(),
            actor: step_def.actor.clone(),
            human_id: step_def.human_id.clone(),
            feature: step_def.feature.clone(),
            inner: base_step_arc,
        });
        proxy_steps.push(proxy as StepRef);

        if let Some(on_success) = step_def.on_success {
            transitions.insert(
                step_def.step_type.clone(),
                StepTransition {
                    on_success,
                    on_failure: step_def.on_failure,
                },
            );
        } else if step_def.on_failure.is_some() {
            // on_failure without on_success doesn't make much sense in our model unless implicit COMPLETE
            transitions.insert(
                step_def.step_type.clone(),
                StepTransition {
                    on_success: "COMPLETE".to_owned(),
                    on_failure: step_def.on_failure,
                },
            );
        }
    }

    let dynamic_flow = Arc::new(DynamicFlow {
        flow_type: flow_type.clone(),
        human_id: definition.metadata.human_id_prefix.clone(),
        feature: definition.metadata.feature.clone(),
        steps: proxy_steps,
        initial_step,
        transitions,
    });

    registry.register_flow(dynamic_flow);

    Ok(())
}

pub fn apply_session_import(
    registry: &mut FlowRegistry,
    definition: backend_flow_sdk::SessionDefinition,
) -> Result<(), FlowError> {
    if registry.get_session(&definition.session_type).is_some()
        && !definition.override_existing.unwrap_or(false)
    {
        return Err(FlowError::InvalidDefinition(format!(
            "Session '{}' already exists in registry and override_existing is false or missing",
            definition.session_type
        )));
    }
    registry.register_session(definition);
    Ok(())
}

struct ProxyStep {
    step_type: String,
    actor: Actor,
    human_id: String,
    feature: Option<String>,
    inner: StepRef,
}

#[async_trait::async_trait]
impl backend_flow_sdk::Step for ProxyStep {
    fn step_type(&self) -> &str {
        &self.step_type
    }

    fn actor(&self) -> Actor {
        self.actor.clone()
    }

    fn human_id(&self) -> &str {
        &self.human_id
    }

    fn feature(&self) -> Option<&str> {
        self.feature.as_deref()
    }

    async fn execute(
        &self,
        ctx: &backend_flow_sdk::StepContext,
    ) -> Result<backend_flow_sdk::StepOutcome, FlowError> {
        self.inner.execute(ctx).await
    }

    async fn validate_input(&self, input: &serde_json::Value) -> Result<(), FlowError> {
        self.inner.validate_input(input).await
    }
}

struct DynamicFlow {
    flow_type: String,
    human_id: String,
    feature: Option<String>,
    steps: Vec<StepRef>,
    initial_step: String,
    transitions: HashMap<String, StepTransition>,
}

impl Flow for DynamicFlow {
    fn flow_type(&self) -> &str {
        &self.flow_type
    }

    fn human_id(&self) -> &str {
        &self.human_id
    }

    fn feature(&self) -> Option<&str> {
        self.feature.as_deref()
    }

    fn steps(&self) -> &[StepRef] {
        &self.steps
    }

    fn initial_step(&self) -> &str {
        &self.initial_step
    }

    fn transitions(&self) -> &HashMap<String, StepTransition> {
        &self.transitions
    }
}

pub fn actor_label(actor: Actor) -> &'static str {
    match actor {
        Actor::System => "SYSTEM",
        Actor::Admin => "ADMIN",
        Actor::EndUser => "END_USER",
    }
}

pub fn waiting_status(actor: Actor) -> &'static str {
    match actor {
        Actor::System => "RUNNING",
        Actor::Admin | Actor::EndUser => "WAITING",
    }
}

fn register_flow_bundle(registry: &mut FlowRegistry, flow: Arc<dyn Flow>) {
    for step in flow.steps() {
        registry.register_step(step.clone());
    }
    registry.register_flow(flow);
}

struct StaticFlow {
    flow_type: &'static str,
    human_id: &'static str,
    feature: Option<&'static str>,
    steps: Vec<StepRef>,
    initial_step: &'static str,
    transitions: HashMap<String, StepTransition>,
}

impl Flow for StaticFlow {
    fn flow_type(&self) -> &str {
        self.flow_type
    }

    fn human_id(&self) -> &str {
        self.human_id
    }

    fn feature(&self) -> Option<&str> {
        self.feature
    }

    fn steps(&self) -> &[StepRef] {
        &self.steps
    }

    fn initial_step(&self) -> &str {
        self.initial_step
    }

    fn transitions(&self) -> &HashMap<String, StepTransition> {
        &self.transitions
    }
}

fn static_flow(
    flow_type: &'static str,
    human_id: &'static str,
    feature: Option<&'static str>,
    initial_step: &'static str,
    steps: Vec<StepRef>,
    transitions: &[(&'static str, &'static str, Option<&'static str>)],
) -> Arc<dyn Flow> {
    let mut map = HashMap::new();
    for (step_type, on_success, on_failure) in transitions {
        map.insert(
            (*step_type).to_owned(),
            StepTransition {
                on_success: (*on_success).to_owned(),
                on_failure: on_failure.map(ToOwned::to_owned),
            },
        );
    }

    Arc::new(StaticFlow {
        flow_type,
        human_id,
        feature,
        steps,
        initial_step,
        transitions: map,
    })
}
