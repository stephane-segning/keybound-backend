use crate::{Flow, FlowDefinition, FlowError, SessionDefinition, Step};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::debug;

#[derive(Default)]
pub struct FlowRegistry {
    steps: HashMap<String, Arc<dyn Step>>,
    flows: HashMap<String, Arc<dyn Flow>>,
    flow_definitions: HashMap<String, FlowDefinition>,
    sessions: HashMap<String, SessionDefinition>,
}

impl FlowRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_step(&mut self, step: Arc<dyn Step>) {
        debug!("Registering step: {}", step.step_type());
        self.steps.insert(step.step_type().to_owned(), step);
    }

    pub fn register_flow(&mut self, flow: Arc<dyn Flow>) {
        debug!("Registering flow: {}", flow.flow_type());
        self.flows.insert(flow.flow_type().to_owned(), flow);
    }

    pub fn register_session(&mut self, session: SessionDefinition) {
        debug!("Registering session: {}", session.session_type);
        self.sessions.insert(session.session_type.clone(), session);
    }

    pub fn register_flow_definition(&mut self, definition: FlowDefinition) {
        debug!("Registering flow definition: {}", definition.flow_type);
        self.flow_definitions
            .insert(definition.flow_type.clone(), definition);
    }

    pub fn get_step(&self, step_type: &str) -> Option<&dyn Step> {
        self.steps.get(step_type).map(Arc::as_ref)
    }

    pub fn get_step_arc(&self, step_type: &str) -> Option<Arc<dyn Step>> {
        self.steps.get(step_type).cloned()
    }

    pub fn get_flow(&self, flow_type: &str) -> Option<&dyn Flow> {
        self.flows.get(flow_type).map(Arc::as_ref)
    }

    pub fn get_session(&self, session_type: &str) -> Option<&SessionDefinition> {
        self.sessions.get(session_type)
    }

    pub fn get_flow_definition(&self, flow_type: &str) -> Option<&FlowDefinition> {
        self.flow_definitions.get(flow_type)
    }

    pub fn flow_definitions(&self) -> Vec<&FlowDefinition> {
        let mut values: Vec<_> = self.flow_definitions.values().collect();
        values.sort_by(|a, b| a.flow_type.cmp(&b.flow_type));
        values
    }

    pub fn validate_features(&self, enabled_features: &[&str]) -> Result<(), FlowError> {
        let enabled = enabled_features
            .iter()
            .map(|feature| (*feature).to_owned())
            .collect::<HashSet<_>>();

        for step in self.steps.values() {
            if let Some(feature) = step.feature()
                && !enabled.contains(feature)
            {
                return Err(FlowError::FeatureNotEnabled {
                    feature: feature.to_owned(),
                    item_kind: "step",
                    item: step.step_type().to_owned(),
                });
            }
        }

        for flow in self.flows.values() {
            if let Some(feature) = flow.feature()
                && !enabled.contains(feature)
            {
                return Err(FlowError::FeatureNotEnabled {
                    feature: feature.to_owned(),
                    item_kind: "flow",
                    item: flow.flow_type().to_owned(),
                });
            }
        }

        for session in self.sessions.values() {
            if let Some(feature) = session.feature.as_ref()
                && !enabled.contains(feature)
            {
                return Err(FlowError::FeatureNotEnabled {
                    feature: feature.clone(),
                    item_kind: "session",
                    item: session.session_type.clone(),
                });
            }
        }

        Ok(())
    }

    pub fn step_types(&self) -> Vec<String> {
        let mut values = self.steps.keys().cloned().collect::<Vec<_>>();
        values.sort();
        values
    }

    pub fn flow_types(&self) -> Vec<String> {
        let mut values = self.flows.keys().cloned().collect::<Vec<_>>();
        values.sort();
        values
    }

    pub fn session_types(&self) -> Vec<String> {
        let mut values = self.sessions.keys().cloned().collect::<Vec<_>>();
        values.sort();
        values
    }

    pub fn sessions(&self) -> Vec<&SessionDefinition> {
        let mut values: Vec<_> = self.sessions.values().collect();
        values.sort_by(|a, b| a.session_type.cmp(&b.session_type));
        values
    }
}
