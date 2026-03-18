use backend_flow_sdk::flow::StepRef;
use backend_flow_sdk::{
    Actor, Flow, FlowConfigLoader, FlowError, FlowRegistry, LoadedConfigs, StepTransition,
};
use std::collections::HashMap;
use std::sync::Arc;

use tracing::{debug, info, warn};

#[derive(Default, Clone)]
pub struct RegistryImports {
    pub flows: Vec<backend_flow_sdk::flow::FlowDefinition>,
    pub sessions: Vec<backend_flow_sdk::SessionDefinition>,
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
    config: Option<serde_json::Value>,
    inner: StepRef,
}

impl ProxyStep {
    fn context_with_config(
        &self,
        ctx: &backend_flow_sdk::StepContext,
    ) -> backend_flow_sdk::StepContext {
        if let Some(config) = &self.config {
            let mut services = ctx.services.clone();
            services.config = Some(
                config
                    .as_object()
                    .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default(),
            );
            backend_flow_sdk::StepContext {
                services,
                ..ctx.clone()
            }
        } else {
            ctx.clone()
        }
    }
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
        let ctx_with_config = self.context_with_config(ctx);
        self.inner.execute(&ctx_with_config).await
    }

    async fn validate_input(&self, input: &serde_json::Value) -> Result<(), FlowError> {
        self.inner.validate_input(input).await
    }

    async fn verify_input(
        &self,
        ctx: &backend_flow_sdk::StepContext,
        input: &serde_json::Value,
    ) -> Result<backend_flow_sdk::StepOutcome, FlowError> {
        let ctx_with_config = self.context_with_config(ctx);
        self.inner.verify_input(&ctx_with_config, input).await
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

fn register_builtin_actions(registry: &mut FlowRegistry) {
    use backend_flow_sdk::{
        CloseSessionAction, ConditionalAction, DebugLogAction, ErrorAction, GenerateOtpAction,
        GetUserAction, NoopAction, RetryAction, ReviewDocumentAction, SetAction,
        UpdatePhoneNumberAction, UpdateUserMetadataAction, UploadDocumentAction,
        ValidateDepositAction, VerifyOtpAction, WaitAction, WebhookStep,
    };

    debug!("Registering built-in action steps...");

    registry.register_step(Arc::new(NoopAction));
    registry.register_step(Arc::new(ErrorAction));
    registry.register_step(Arc::new(RetryAction));
    registry.register_step(Arc::new(WaitAction));
    registry.register_step(Arc::new(SetAction));
    registry.register_step(Arc::new(GenerateOtpAction));
    registry.register_step(Arc::new(VerifyOtpAction));
    registry.register_step(Arc::new(GetUserAction));
    registry.register_step(Arc::new(DebugLogAction));
    registry.register_step(Arc::new(ConditionalAction));
    registry.register_step(Arc::new(UpdateUserMetadataAction));
    registry.register_step(Arc::new(UpdatePhoneNumberAction));
    registry.register_step(Arc::new(CloseSessionAction));
    registry.register_step(Arc::new(UploadDocumentAction));
    registry.register_step(Arc::new(ReviewDocumentAction));
    registry.register_step(Arc::new(ValidateDepositAction));
    registry.register_step(Arc::new(
        crate::flows::definitions::shared_steps::CheckUserExistsStep,
    ));
    registry.register_step(Arc::new(WebhookStep::new()));

    debug!(
        "Registered {} built-in actions",
        registry.step_types().len()
    );
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
            config: step_def.config.clone(),
            inner: base_step_arc,
        });
        proxy_steps.push(proxy as StepRef);

        let on_success = step_def
            .next
            .clone()
            .or_else(|| step_def.ok.clone())
            .unwrap_or_else(|| "COMPLETED".to_owned());

        transitions.insert(
            step_name.clone(),
            StepTransition {
                on_success,
                on_failure: step_def.fail.clone(),
                branches: step_def.branches.clone(),
            },
        );
    }

    let dynamic_flow = Arc::new(DynamicFlow {
        flow_type: flow_type.clone(),
        human_id: definition.human_id_prefix.clone(),
        feature: definition.feature.clone(),
        steps: proxy_steps,
        initial_step,
        transitions,
    });

    registry.register_flow_definition(definition);
    registry.register_flow(dynamic_flow);

    Ok(())
}
