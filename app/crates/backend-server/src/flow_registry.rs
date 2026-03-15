use backend_flow_sdk::flow::StepRef;
use backend_flow_sdk::{Actor, Flow, FlowRegistry, SessionDefinition, StepTransition};
use std::collections::HashMap;
use std::sync::Arc;

use crate::flow_logic;

pub const SESSION_TYPE_KYC_FULL: &str = "KYC_FULL";
pub const SESSION_TYPE_ACCOUNT_MANAGEMENT: &str = "ACCOUNT_MANAGEMENT";
pub const SESSION_TYPE_ADMIN_OPERATIONS: &str = "ADMIN_OPERATIONS";

pub fn build_registry() -> FlowRegistry {
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
    });

    registry.register_session(SessionDefinition {
        session_type: SESSION_TYPE_ACCOUNT_MANAGEMENT.to_owned(),
        human_id_prefix: "auth".to_owned(),
        feature: None,
        allowed_flows: account_allowed_flows,
    });

    registry.register_session(SessionDefinition {
        session_type: SESSION_TYPE_ADMIN_OPERATIONS.to_owned(),
        human_id_prefix: "admin".to_owned(),
        feature: None,
        allowed_flows: admin_allowed_flows,
    });

    registry
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
    fn flow_type(&self) -> &'static str {
        self.flow_type
    }

    fn human_id(&self) -> &'static str {
        self.human_id
    }

    fn feature(&self) -> Option<&'static str> {
        self.feature
    }

    fn steps(&self) -> &[StepRef] {
        &self.steps
    }

    fn initial_step(&self) -> &'static str {
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
