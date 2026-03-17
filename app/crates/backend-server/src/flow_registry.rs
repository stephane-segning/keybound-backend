use backend_flow_sdk::flow::StepRef;
use backend_flow_sdk::{
    Actor, Flow, FlowConfigLoader, FlowError, FlowRegistry, LoadedConfigs, SessionDefinition,
    StepTransition,
};
use std::collections::HashMap;
use std::sync::Arc;

use crate::flow_logic;

use tracing::{debug, info, warn};

pub const SESSION_TYPE_KYC_FULL: &str = "KYC_FULL";
pub const SESSION_TYPE_ACCOUNT_MANAGEMENT: &str = "ACCOUNT_MANAGEMENT";
pub const SESSION_TYPE_ADMIN_OPERATIONS: &str = "ADMIN_OPERATIONS";

#[derive(Default, Clone)]
pub struct RegistryImports {
    pub flows: Vec<backend_flow_sdk::flow::FlowDefinition>,
    pub sessions: Vec<backend_flow_sdk::SessionDefinition>,
    #[cfg(feature = "flow-cuss-integration")]
    pub cuss_url: Option<String>,
    pub flows_dir: Option<String>,
    pub sessions_dir: Option<String>,
}

pub fn build_registry(imports: RegistryImports) -> Result<FlowRegistry, FlowError> {
    info!("Building flow registry...");
    let mut registry = FlowRegistry::new();

    register_builtin_actions(&mut registry);

    let yaml_configs = load_yaml_configs(&imports);
    for flow_def in &yaml_configs.flows {
        debug!("Registering YAML flow: {}", flow_def.flow_type);
        register_flow_definition(&mut registry, flow_def.clone())?;
    }
    for session_def in &yaml_configs.sessions {
        debug!("Registering YAML session: {}", session_def.session_type);
        registry.register_session(session_def.clone());
    }

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
        #[cfg(feature = "flow-cuss-integration")]
        {
            if let Some(cuss_url) = imports.cuss_url.clone() {
                let steps: Vec<StepRef> = vec![
                    Arc::new(super::flow_logic::builtin_steps::CheckUserExistsStep),
                    Arc::new(super::flow_logic::builtin_steps::ValidateDepositStep),
                    Arc::new(super::flow_logic::first_deposit::AwaitPaymentConfirmationStep),
                ];
                let mut all_steps = steps;
                all_steps.extend(super::flow_logic::cuss_integration::steps(cuss_url));

                let mut transitions = HashMap::new();
                transitions.insert(
                    "check_user_exists".to_string(),
                    StepTransition {
                        on_success: "validate_deposit".to_string(),
                        on_failure: Some("FAILED".to_string()),
                    },
                );
                transitions.insert(
                    "validate_deposit".to_string(),
                    StepTransition {
                        on_success: "AWAIT_PAYMENT_CONFIRMATION".to_string(),
                        on_failure: Some("FAILED".to_string()),
                    },
                );
                transitions.insert(
                    "AWAIT_PAYMENT_CONFIRMATION".to_string(),
                    StepTransition {
                        on_success: "CUSS_REGISTER_CUSTOMER".to_string(),
                        on_failure: Some("FAILED".to_string()),
                    },
                );
                transitions.insert(
                    "CUSS_REGISTER_CUSTOMER".to_string(),
                    StepTransition {
                        on_success: "CUSS_APPROVE_AND_DEPOSIT".to_string(),
                        on_failure: Some("FAILED".to_string()),
                    },
                );
                transitions.insert(
                    "CUSS_APPROVE_AND_DEPOSIT".to_string(),
                    StepTransition {
                        on_success: "COMPLETE".to_string(),
                        on_failure: Some("FAILED".to_string()),
                    },
                );

                let flow = Arc::new(StaticFlow {
                    flow_type: "FIRST_DEPOSIT",
                    human_id: "first_deposit",
                    feature: Some("flow-first-deposit"),
                    steps: all_steps,
                    initial_step: "check_user_exists",
                    transitions,
                });
                register_flow_bundle(&mut registry, flow);
            } else {
                let flow = static_flow(
                    "FIRST_DEPOSIT",
                    "first_deposit",
                    Some("flow-first-deposit"),
                    "check_user_exists",
                    flow_logic::first_deposit::steps(),
                    &[
                        ("check_user_exists", "validate_deposit", Some("FAILED")),
                        (
                            "validate_deposit",
                            "AWAIT_PAYMENT_CONFIRMATION",
                            Some("FAILED"),
                        ),
                        (
                            "AWAIT_PAYMENT_CONFIRMATION",
                            "APPROVE_AND_DEPOSIT",
                            Some("FAILED"),
                        ),
                        ("APPROVE_AND_DEPOSIT", "COMPLETE", Some("FAILED")),
                    ],
                );
                register_flow_bundle(&mut registry, flow);
            }
        }

        #[cfg(not(feature = "flow-cuss-integration"))]
        {
            let flow = static_flow(
                "FIRST_DEPOSIT",
                "first_deposit",
                Some("flow-first-deposit"),
                "check_user_exists",
                flow_logic::first_deposit::steps(),
                &[
                    ("check_user_exists", "validate_deposit", Some("FAILED")),
                    (
                        "validate_deposit",
                        "AWAIT_PAYMENT_CONFIRMATION",
                        Some("FAILED"),
                    ),
                    (
                        "AWAIT_PAYMENT_CONFIRMATION",
                        "APPROVE_AND_DEPOSIT",
                        Some("FAILED"),
                    ),
                    ("APPROVE_AND_DEPOSIT", "COMPLETE", Some("FAILED")),
                ],
            );
            register_flow_bundle(&mut registry, flow);
        }
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

    for flow_def in imports.flows {
        debug!("Importing flow: {}", flow_def.flow_type);
        apply_flow_import(&mut registry, flow_def)?;
    }

    for session_def in imports.sessions {
        debug!("Importing session: {}", session_def.session_type);
        apply_session_import(&mut registry, session_def)?;
    }

    Ok(registry)
}

pub fn apply_flow_import(
    registry: &mut FlowRegistry,
    definition: backend_flow_sdk::flow::FlowDefinition,
) -> Result<(), FlowError> {
    register_flow_definition(registry, definition)
}

pub fn apply_session_import(
    registry: &mut FlowRegistry,
    definition: backend_flow_sdk::SessionDefinition,
) -> Result<(), FlowError> {
    if registry.get_session(&definition.session_type).is_some() {
        return Err(FlowError::InvalidDefinition(format!(
            "Session '{}' already exists in registry",
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
        self.actor
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

fn register_builtin_actions(registry: &mut FlowRegistry) {
    use backend_flow_sdk::{
        ErrorAction, GenerateOtpAction, NoopAction, RetryAction, ReviewDocumentAction, SetAction,
        UploadDocumentAction, ValidateDepositAction, VerifyOtpAction, WaitAction,
    };

    debug!("Registering built-in action steps...");

    registry.register_step(Arc::new(NoopAction));
    registry.register_step(Arc::new(ErrorAction));
    registry.register_step(Arc::new(RetryAction));
    registry.register_step(Arc::new(WaitAction));
    registry.register_step(Arc::new(SetAction));
    registry.register_step(Arc::new(GenerateOtpAction));
    registry.register_step(Arc::new(VerifyOtpAction));
    registry.register_step(Arc::new(UploadDocumentAction));
    registry.register_step(Arc::new(ReviewDocumentAction));
    registry.register_step(Arc::new(ValidateDepositAction));

    debug!("Registered {} built-in actions", registry.step_types().len());
}

fn load_yaml_configs(imports: &RegistryImports) -> LoadedConfigs {
    let flows_dir = imports.flows_dir.as_deref().unwrap_or("flows");
    let sessions_dir = imports.sessions_dir.as_deref().unwrap_or("sessions");

    let loader = FlowConfigLoader::new(flows_dir, sessions_dir);

    match loader.load_from_fs() {
        Ok(configs) => {
            info!(
                "Loaded {} flows and {} sessions from YAML files",
                configs.flows.len(),
                configs.sessions.len()
            );
            configs
        }
        Err(e) => {
            warn!("Failed to load YAML configs, using defaults: {}", e);
            LoadedConfigs::default()
        }
    }
}

fn register_flow_definition(
    registry: &mut FlowRegistry,
    definition: backend_flow_sdk::flow::FlowDefinition,
) -> Result<(), FlowError> {
    let flow_type = definition.flow_type.clone();

    if registry.get_flow(&flow_type).is_some() {
        return Err(FlowError::InvalidDefinition(format!(
            "Flow '{}' already exists in registry",
            flow_type
        )));
    }

    let mut proxy_steps = Vec::new();
    let mut transitions = HashMap::new();

    let initial_step = definition.initial_step.clone();

    for (step_name, step_def) in &definition.steps {
        let base_step_arc = registry.get_step_arc(&step_def.action).ok_or_else(|| {
            FlowError::InvalidDefinition(format!(
                "Flow '{}' references unknown action '{}'",
                flow_type, step_def.action
            ))
        })?;

        let proxy = Arc::new(ProxyStep {
            step_type: step_name.clone(),
            actor: step_def.actor,
            human_id: step_name.clone(),
            feature: None,
            inner: base_step_arc,
        });
        proxy_steps.push(proxy as StepRef);

        if let Some(next) = &step_def.next {
            transitions.insert(
                step_name.clone(),
                StepTransition {
                    on_success: next.clone(),
                    on_failure: step_def.fail.clone(),
                },
            );
        } else if let Some(ok) = &step_def.ok {
            transitions.insert(
                step_name.clone(),
                StepTransition {
                    on_success: ok.clone(),
                    on_failure: step_def.fail.clone(),
                },
            );
        }
    }

    let dynamic_flow = Arc::new(DynamicFlow {
        flow_type: flow_type.clone(),
        human_id: definition.human_id_prefix.clone(),
        feature: definition.feature.clone(),
        steps: proxy_steps,
        initial_step,
        transitions,
    });

    registry.register_flow(dynamic_flow);

    Ok(())
}
