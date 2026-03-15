use crate::{Flow, FlowError, SessionDefinition, Step};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Default)]
pub struct FlowRegistry {
    steps: HashMap<String, Arc<dyn Step>>,
    flows: HashMap<String, Arc<dyn Flow>>,
    sessions: HashMap<String, SessionDefinition>,
}

impl FlowRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_step(&mut self, step: Arc<dyn Step>) {
        self.steps.insert(step.step_type().to_owned(), step);
    }

    pub fn register_flow(&mut self, flow: Arc<dyn Flow>) {
        self.flows.insert(flow.flow_type().to_owned(), flow);
    }

    pub fn register_session(&mut self, session: SessionDefinition) {
        self.sessions.insert(session.session_type.clone(), session);
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
}
