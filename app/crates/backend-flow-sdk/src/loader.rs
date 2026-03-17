use crate::{
    import_flow_definition, import_session_definition, FlowDefinition, FlowError, FlowRegistry,
    ImportFormat, SessionDefinition,
};
use std::path::PathBuf;
use tracing::{debug, info, warn};

#[cfg(feature = "embedded-config")]
use include_dir::Dir;

#[derive(Debug, Clone)]
pub struct FlowConfigLoader {
    flows_dir: PathBuf,
    sessions_dir: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct LoadedConfigs {
    pub flows: Vec<FlowDefinition>,
    pub sessions: Vec<SessionDefinition>,
}

impl FlowConfigLoader {
    pub fn new(flows_dir: impl Into<PathBuf>, sessions_dir: impl Into<PathBuf>) -> Self {
        Self {
            flows_dir: flows_dir.into(),
            sessions_dir: sessions_dir.into(),
        }
    }

    pub fn new_default() -> Self {
        Self::new("flows", "sessions")
    }

    pub fn flows_dir(&self) -> &std::path::Path {
        &self.flows_dir
    }

    pub fn sessions_dir(&self) -> &std::path::Path {
        &self.sessions_dir
    }

    #[cfg(feature = "embedded-config")]
    pub fn load_embedded(
        flows_dir: &Dir<'_>,
        sessions_dir: &Dir<'_>,
    ) -> Result<LoadedConfigs, FlowError> {
        let mut configs = LoadedConfigs::default();

        for file in flows_dir.files() {
            if let Some(content) = file.contents_utf8() {
                let format = ImportFormat::from_path(file.path());
                match import_flow_definition(content, format) {
                    Ok(def) => {
                        debug!("Loaded embedded flow: {}", def.flow_type);
                        configs.flows.push(def);
                    }
                    Err(e) => {
                        warn!(
                            "Failed to load embedded flow {}: {}",
                            file.path().display(),
                            e
                        );
                    }
                }
            }
        }

        for file in sessions_dir.files() {
            if let Some(content) = file.contents_utf8() {
                let format = ImportFormat::from_path(file.path());
                match import_session_definition(content, format) {
                    Ok(def) => {
                        debug!("Loaded embedded session: {}", def.session_type);
                        configs.sessions.push(def);
                    }
                    Err(e) => {
                        warn!(
                            "Failed to load embedded session {}: {}",
                            file.path().display(),
                            e
                        );
                    }
                }
            }
        }

        Ok(configs)
    }

    #[cfg(not(feature = "embedded-config"))]
    pub fn load_embedded() -> Result<LoadedConfigs, FlowError> {
        Ok(LoadedConfigs::default())
    }

    pub fn load_from_fs(&self) -> Result<LoadedConfigs, FlowError> {
        let mut configs = LoadedConfigs::default();

        if self.flows_dir.exists() {
            configs.flows = self.load_flows_from_dir(&self.flows_dir)?;
        } else {
            debug!(
                "Flows directory does not exist: {}",
                self.flows_dir.display()
            );
        }

        if self.sessions_dir.exists() {
            configs.sessions = self.load_sessions_from_dir(&self.sessions_dir)?;
        } else {
            debug!(
                "Sessions directory does not exist: {}",
                self.sessions_dir.display()
            );
        }

        Ok(configs)
    }

    fn load_flows_from_dir(&self, dir: &std::path::Path) -> Result<Vec<FlowDefinition>, FlowError> {
        let mut flows = Vec::new();

        for pattern in &["*.yaml", "*.yml", "*.json"] {
            let glob_pattern = dir.join(pattern);
            let pattern_str = glob_pattern.to_string_lossy();

            for entry in glob::glob(&pattern_str).map_err(|e| {
                FlowError::InvalidDefinition(format!(
                    "Invalid glob pattern '{}': {}",
                    pattern_str, e
                ))
            })? {
                match entry {
                    Ok(path) => match self.load_flow_file(&path) {
                        Ok(flow) => {
                            debug!(
                                "Loaded flow from file: {} -> {}",
                                path.display(),
                                flow.flow_type
                            );
                            flows.push(flow);
                        }
                        Err(e) => {
                            warn!("Failed to load flow from {}: {}", path.display(), e);
                        }
                    },
                    Err(e) => {
                        warn!("Glob error for path: {}", e);
                    }
                }
            }
        }

        Ok(flows)
    }

    fn load_sessions_from_dir(
        &self,
        dir: &std::path::Path,
    ) -> Result<Vec<SessionDefinition>, FlowError> {
        let mut sessions = Vec::new();

        for pattern in &["*.yaml", "*.yml", "*.json"] {
            let glob_pattern = dir.join(pattern);
            let pattern_str = glob_pattern.to_string_lossy();

            for entry in glob::glob(&pattern_str).map_err(|e| {
                FlowError::InvalidDefinition(format!(
                    "Invalid glob pattern '{}': {}",
                    pattern_str, e
                ))
            })? {
                match entry {
                    Ok(path) => match self.load_session_file(&path) {
                        Ok(session) => {
                            debug!(
                                "Loaded session from file: {} -> {}",
                                path.display(),
                                session.session_type
                            );
                            sessions.push(session);
                        }
                        Err(e) => {
                            warn!("Failed to load session from {}: {}", path.display(), e);
                        }
                    },
                    Err(e) => {
                        warn!("Glob error for path: {}", e);
                    }
                }
            }
        }

        Ok(sessions)
    }

    fn load_flow_file(&self, path: &std::path::Path) -> Result<FlowDefinition, FlowError> {
        let content = std::fs::read_to_string(path)?;
        let format = ImportFormat::from_path(path);
        import_flow_definition(&content, format)
    }

    fn load_session_file(&self, path: &std::path::Path) -> Result<SessionDefinition, FlowError> {
        let content = std::fs::read_to_string(path)?;
        let format = ImportFormat::from_path(path);
        import_session_definition(&content, format)
    }

    pub fn load_with_override(&self, embedded: LoadedConfigs) -> Result<LoadedConfigs, FlowError> {
        let fs_configs = self.load_from_fs()?;

        let mut flows = embedded.flows;
        for fs_flow in fs_configs.flows {
            if let Some(pos) = flows.iter().position(|f| f.flow_type == fs_flow.flow_type) {
                info!(
                    "Overriding embedded flow '{}' with filesystem version",
                    fs_flow.flow_type
                );
                flows[pos] = fs_flow;
            } else {
                debug!("Adding new flow from filesystem: {}", fs_flow.flow_type);
                flows.push(fs_flow);
            }
        }

        let mut sessions = embedded.sessions;
        for fs_session in fs_configs.sessions {
            if let Some(pos) = sessions
                .iter()
                .position(|s| s.session_type == fs_session.session_type)
            {
                info!(
                    "Overriding embedded session '{}' with filesystem version",
                    fs_session.session_type
                );
                sessions[pos] = fs_session;
            } else {
                debug!(
                    "Adding new session from filesystem: {}",
                    fs_session.session_type
                );
                sessions.push(fs_session);
            }
        }

        Ok(LoadedConfigs { flows, sessions })
    }

    pub fn register_all(
        &self,
        registry: &mut FlowRegistry,
        configs: LoadedConfigs,
    ) -> Result<(), FlowError> {
        for flow in configs.flows {
            let flow_type = flow.flow_type.clone();
            match self.register_flow(registry, flow) {
                Ok(()) => debug!("Registered flow: {}", flow_type),
                Err(e) => warn!("Failed to register flow '{}': {}", flow_type, e),
            }
        }

        for session in configs.sessions {
            let session_type = session.session_type.clone();
            match self.register_session(registry, session) {
                Ok(()) => debug!("Registered session: {}", session_type),
                Err(e) => warn!("Failed to register session '{}': {}", session_type, e),
            }
        }

        Ok(())
    }

    fn register_flow(
        &self,
        registry: &mut FlowRegistry,
        definition: FlowDefinition,
    ) -> Result<(), FlowError> {
        if registry
            .get_flow_definition(&definition.flow_type)
            .is_some()
        {
            return Err(FlowError::InvalidDefinition(format!(
                "Flow '{}' already exists in registry",
                definition.flow_type
            )));
        }

        registry.register_flow_definition(definition);
        Ok(())
    }

    fn register_session(
        &self,
        registry: &mut FlowRegistry,
        definition: SessionDefinition,
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
}

#[cfg(feature = "embedded-config")]
#[macro_export]
macro_rules! embed_configs {
    ($flows_dir:expr, $sessions_dir:expr) => {{
        let flows_dir: include_dir::Dir = include_dir::include_dir!($flows_dir);
        let sessions_dir: include_dir::Dir = include_dir::include_dir!($sessions_dir);
        $crate::FlowConfigLoader::load_embedded(&flows_dir, &sessions_dir)
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_flow_yaml() -> &'static str {
        r#"
flow_type: TEST_FLOW
human_id_prefix: test
initial_step: test_step
steps:
  test_step:
    action: test_action
    actor: SYSTEM
    ok: COMPLETE
"#
    }

    fn create_test_session_yaml() -> &'static str {
        r#"
session_type: TEST_SESSION
human_id_prefix: test
allowed_flows:
  - TEST_FLOW
"#
    }

    #[test]
    fn test_load_from_fs_empty_dirs() {
        let temp_dir = TempDir::new().unwrap();
        let flows_dir = temp_dir.path().join("flows");
        let sessions_dir = temp_dir.path().join("sessions");

        std::fs::create_dir(&flows_dir).unwrap();
        std::fs::create_dir(&sessions_dir).unwrap();

        let loader = FlowConfigLoader::new(&flows_dir, &sessions_dir);
        let configs = loader.load_from_fs().unwrap();

        assert!(configs.flows.is_empty());
        assert!(configs.sessions.is_empty());
    }

    #[test]
    fn test_load_flow_from_file() {
        let temp_dir = TempDir::new().unwrap();
        let flows_dir = temp_dir.path().join("flows");
        std::fs::create_dir(&flows_dir).unwrap();

        let flow_file = flows_dir.join("test_flow.yaml");
        let mut file = std::fs::File::create(&flow_file).unwrap();
        file.write_all(create_test_flow_yaml().as_bytes()).unwrap();

        let loader = FlowConfigLoader::new(&flows_dir, temp_dir.path().join("sessions"));
        let configs = loader.load_from_fs().unwrap();

        assert_eq!(configs.flows.len(), 1);
        assert_eq!(configs.flows[0].flow_type, "TEST_FLOW");
    }

    #[test]
    fn test_load_session_from_file() {
        let temp_dir = TempDir::new().unwrap();
        let sessions_dir = temp_dir.path().join("sessions");
        std::fs::create_dir(&sessions_dir).unwrap();

        let session_file = sessions_dir.join("test_session.yaml");
        let mut file = std::fs::File::create(&session_file).unwrap();
        file.write_all(create_test_session_yaml().as_bytes())
            .unwrap();

        let loader = FlowConfigLoader::new(temp_dir.path().join("flows"), &sessions_dir);
        let configs = loader.load_from_fs().unwrap();

        assert_eq!(configs.sessions.len(), 1);
        assert_eq!(configs.sessions[0].session_type, "TEST_SESSION");
    }

    #[test]
    fn test_load_with_override() {
        let temp_dir = TempDir::new().unwrap();
        let flows_dir = temp_dir.path().join("flows");
        std::fs::create_dir(&flows_dir).unwrap();

        let mut embedded = LoadedConfigs::default();
        embedded.flows.push(FlowDefinition {
            flow_type: "TEST_FLOW".to_string(),
            human_id_prefix: "embedded".to_string(),
            feature: None,
            initial_step: "step1".to_string(),
            steps: std::collections::HashMap::new(),
        });

        let override_flow = r#"
flow_type: TEST_FLOW
human_id_prefix: filesystem
initial_step: step2
steps:
  step2:
    action: action2
    actor: SYSTEM
    ok: COMPLETE
"#;
        let flow_file = flows_dir.join("test_flow.yaml");
        let mut file = std::fs::File::create(&flow_file).unwrap();
        file.write_all(override_flow.as_bytes()).unwrap();

        let loader = FlowConfigLoader::new(&flows_dir, temp_dir.path().join("sessions"));
        let configs = loader.load_with_override(embedded).unwrap();

        assert_eq!(configs.flows.len(), 1);
        assert_eq!(configs.flows[0].human_id_prefix, "filesystem");
    }

    #[test]
    fn test_register_all() {
        let temp_dir = TempDir::new().unwrap();
        let flows_dir = temp_dir.path().join("flows");
        let sessions_dir = temp_dir.path().join("sessions");

        std::fs::create_dir(&flows_dir).unwrap();
        std::fs::create_dir(&sessions_dir).unwrap();

        let flow_file = flows_dir.join("test_flow.yaml");
        let mut file = std::fs::File::create(&flow_file).unwrap();
        file.write_all(create_test_flow_yaml().as_bytes()).unwrap();

        let session_file = sessions_dir.join("test_session.yaml");
        let mut file = std::fs::File::create(&session_file).unwrap();
        file.write_all(create_test_session_yaml().as_bytes())
            .unwrap();

        let loader = FlowConfigLoader::new(&flows_dir, &sessions_dir);
        let configs = loader.load_from_fs().unwrap();

        let mut registry = FlowRegistry::new();
        loader.register_all(&mut registry, configs).unwrap();

        assert!(registry.get_flow_definition("TEST_FLOW").is_some());
        assert!(registry.get_session("TEST_SESSION").is_some());
    }
}
