//! Configuration management for the tokenization backend.
//!
//! This module provides configuration structures that are loaded from YAML files
//! with environment variable expansion support. Configuration is hierarchical
//! and covers all aspects of the application including server settings,
//! authentication, storage, and external service integrations.

use crate::error::{Error, Result};
use regex::Regex;
use serde::Deserialize;
use serde_yaml::from_str;
use std::fs::read_to_string;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// Application runtime mode determining which components to start.
///
/// - Server: Start only the HTTP API server
/// - Worker: Start only the background worker for async tasks
/// - Shared: Start both server and worker in the same process
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum RuntimeMode {
    Server,
    Worker,
    #[default]
    Shared,
}

/// Runtime configuration controlling application mode.
#[derive(Debug, Clone, Deserialize)]
pub struct Runtime {
    #[serde(default)]
    pub mode: RuntimeMode,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            mode: RuntimeMode::Shared,
        }
    }
}

/// Redis connection configuration for caching and queues.
#[derive(Debug, Clone, Deserialize)]
pub struct Redis {
    pub url: String,
}

impl Default for Redis {
    fn default() -> Self {
        Self {
            url: "redis://127.0.0.1:6379".to_owned(),
        }
    }
}

/// HTTP server configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct Server {
    pub address: String,
    pub port: u16,
    pub tls: Tls,
}

/// Logging configuration with support for structured JSON output.
#[derive(Debug, Clone, Deserialize)]
pub struct Logging {
    pub level: String,
    pub data_dir: Option<String>,
    pub json: Option<bool>,
    #[serde(default)]
    pub log_requests_enabled: bool,
}

/// PostgreSQL database connection configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct Database {
    pub url: String,
    pub pool_size: Option<u32>,
}

/// OAuth2/OIDC configuration for JWT verification.
#[derive(Debug, Clone, Deserialize)]
pub struct Oauth2 {
    pub issuer: String,
    #[serde(default, alias = "base-paths")]
    pub base_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AwsS3 {
    /// Optional region override for S3.
    pub region: Option<String>,
    /// Optional region override for S3.
    pub force_path_style: Option<bool>,
    pub bucket: String,
    /// Optional custom S3 endpoint (e.g., LocalStack).
    pub endpoint: Option<String>,
    /// TTL for S3 presigned URLs.
    pub presign_ttl_seconds: u64,
}

/// Storage backend type selection.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StorageType {
    S3,
    Minio,
}

/// MinIO/S3-compatible storage configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct MinioStorage {
    pub endpoint: String,
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
    pub bucket: String,
    pub force_path_style: Option<bool>,
    pub presign_ttl_seconds: u64,
}

/// Storage configuration wrapper that selects between S3 and MinIO.
#[derive(Debug, Clone, Deserialize)]
pub struct Storage {
    #[serde(rename = "type")]
    pub r#type: StorageType,
    #[serde(default)]
    pub minio: Option<MinioStorage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AwsSns {
    /// Optional region override for SNS.
    pub region: Option<String>,
    /// Maximum publish attempts before giving up.
    pub max_attempts: u32,
    /// Initial retry backoff in seconds.
    pub initial_backoff_seconds: u64,
}

/// SMS provider selection for OTP delivery.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SmsProviderType {
    Console,
    Sns,
    Api,
}

/// SMS configuration for OTP delivery.
#[derive(Debug, Clone, Deserialize)]
pub struct SmsConfig {
    pub provider: SmsProviderType,
    #[serde(default)]
    pub api: Option<SmsApi>,
}

/// Third-party SMS API configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct SmsApi {
    pub base_url: String,
    pub auth_token: Option<String>,
}

/// Main application configuration containing all sub-systems.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: Server,
    pub logging: Logging,
    pub database: Database,
    pub oauth2: Oauth2,
    #[serde(default)]
    pub runtime: Runtime,
    #[serde(default)]
    pub redis: Redis,
    pub s3: Option<AwsS3>,
    #[serde(default)]
    pub storage: Option<Storage>,
    pub sns: Option<AwsSns>,
    pub sms: Option<SmsConfig>,

    pub kc: KcAuth,
    pub bff: BffAuth,
    pub staff: StaffAuth,
    #[serde(default)]
    pub auth: AuthApi,
    pub deposit_flow: Option<DepositFlow>,
    pub cuss: Cuss,
}

/// TLS certificate configuration for HTTPS.
#[derive(Debug, Clone, Deserialize)]
pub struct Tls {
    pub cert_path: String,
    pub key_path: String,
}

/// Keycloak API surface authentication configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct KcAuth {
    pub enabled: bool,
    #[serde(alias = "base-path")]
    pub base_path: String,
    pub signature_secret: String,
    pub max_clock_skew_seconds: i64,
    pub max_body_bytes: usize,
}

/// BFF (Backend-for-Frontend) API surface authentication configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct BffAuth {
    pub enabled: bool,
    #[serde(alias = "base-path")]
    pub base_path: String,
}

/// Staff API surface authentication configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct StaffAuth {
    pub enabled: bool,
    #[serde(alias = "base-path")]
    pub base_path: String,
}

/// Auth API surface configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct AuthApi {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_auth_base_path", alias = "base-path")]
    pub base_path: String,
    #[serde(default)]
    pub token_issuer: Option<String>,
    #[serde(default)]
    pub token_audience: Option<String>,
    #[serde(default = "default_token_ttl_seconds")]
    pub token_ttl_seconds: i64,
    #[serde(default = "default_auth_max_clock_skew_seconds")]
    pub max_clock_skew_seconds: i64,
}

impl Default for AuthApi {
    fn default() -> Self {
        Self {
            enabled: true,
            base_path: default_auth_base_path(),
            token_issuer: None,
            token_audience: None,
            token_ttl_seconds: default_token_ttl_seconds(),
            max_clock_skew_seconds: default_auth_max_clock_skew_seconds(),
        }
    }
}

/// Deposit flow routing configuration for provider-specific recipients.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct DepositFlow {
    #[serde(default)]
    pub staff: DepositFlowStaff,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DepositFlowStaff {
    #[serde(default)]
    pub recipients: Vec<DepositRecipient>,
}

/// Static recipient row loaded from YAML and synced into app_deposit_recipients.
#[derive(Debug, Clone, Deserialize)]
pub struct DepositRecipient {
    pub provider: String,
    #[serde(rename = "fullname", alias = "full-name")]
    pub full_name: String,
    #[serde(alias = "phone-number")]
    pub phone_number: String,
    pub regex: String,
    pub currency: String,
}

/// CUSS (Customer Service System) integration configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct Cuss {
    pub api_url: String,
}

fn default_true() -> bool {
    true
}

fn default_auth_base_path() -> String {
    "/auth".to_owned()
}

fn default_token_ttl_seconds() -> i64 {
    3600
}

fn default_auth_max_clock_skew_seconds() -> i64 {
    60
}

/// Load configuration from a YAML file with environment variable expansion.
///
/// Supports `${VAR}` and `${VAR:-default}` syntax for environment variable substitution.
/// Returns an error if required environment variables are missing.
pub fn load_from_path<P: AsRef<std::path::Path>>(path: P) -> Result<Config> {
    let content = read_to_string(path)?;
    let expanded = expand_env_vars(&content)?;
    let cfg: Config = from_str(&expanded)?;
    Ok(cfg)
}

fn expand_env_vars(content: &str) -> Result<String> {
    let re = Regex::new(r"\$\{([a-zA-Z_][a-zA-Z0-9_]*)(:-([^}]*))?\}")
        .map_err(|e| Error::Server(e.to_string()))?;
    let mut missing_vars = Vec::new();

    let result = re
        .replace_all(content, |caps: &regex::Captures<'_>| {
            let var_name = &caps[1];
            let default_value = caps.get(3).map(|m| m.as_str());

            match std::env::var(var_name) {
                Ok(val) => val,
                Err(_) => match default_value {
                    Some(default) => default.to_owned(),
                    None => {
                        missing_vars.push(var_name.to_owned());
                        caps[0].to_owned()
                    }
                },
            }
        })
        .to_string();

    missing_vars.sort_unstable();
    missing_vars.dedup();

    if !missing_vars.is_empty() {
        return Err(Error::Server(format!(
            "Missing environment variables: {}",
            missing_vars.join(", ")
        )));
    }

    Ok(result)
}

impl Config {
    /// Returns the combined address and port for the HTTP server to listen on.
    pub fn api_listen_addr(&self) -> Result<SocketAddr> {
        Ok(format!("{}:{}", self.server.address, self.server.port).parse()?)
    }

    /// Returns the TLS certificate and key file paths if both files exist.
    /// Returns None if either file is missing, indicating TLS should be disabled.
    pub fn api_tls_files(&self) -> Option<(PathBuf, PathBuf)> {
        let cert_path: PathBuf = self.server.tls.cert_path.clone().into();
        let key_path: PathBuf = self.server.tls.key_path.clone().into();

        if Path::new(&cert_path).exists() && Path::new(&key_path).exists() {
            Some((cert_path, key_path))
        } else {
            None
        }
    }

    /// Returns the database connection pool size, defaulting to 10 if not configured.
    pub fn database_pool_size(&self) -> u32 {
        self.database.pool_size.unwrap_or(10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::{LazyLock, Mutex};

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[test]
    fn test_expand_env_vars_success() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe {
            env::set_var("TEST_VAR_1", "value1");
            env::set_var("TEST_VAR_2", "value2");
        }

        let content = "var1: ${TEST_VAR_1}, var2: ${TEST_VAR_2}";
        let expanded = expand_env_vars(content).unwrap();

        assert_eq!(expanded, "var1: value1, var2: value2");
    }

    #[test]
    fn test_expand_env_vars_default_used_when_missing() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe {
            env::remove_var("TEST_VAR_WITH_DEFAULT");
        }

        let content = "endpoint: ${TEST_VAR_WITH_DEFAULT:-http://minio:9000}";
        let expanded = expand_env_vars(content).unwrap();

        assert_eq!(expanded, "endpoint: http://minio:9000");
    }

    #[test]
    fn test_expand_env_vars_default_overridden_by_env() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe {
            env::set_var("TEST_VAR_WITH_DEFAULT", "http://override:9000");
        }

        let content = "endpoint: ${TEST_VAR_WITH_DEFAULT:-http://minio:9000}";
        let expanded = expand_env_vars(content).unwrap();

        assert_eq!(expanded, "endpoint: http://override:9000");
    }

    #[test]
    fn test_expand_env_vars_missing() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe {
            env::remove_var("MISSING_VAR");
        }

        let content = "var: ${MISSING_VAR}";
        let result = expand_env_vars(content);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Missing environment variables: MISSING_VAR"));
    }

    #[test]
    fn test_expand_env_vars_multiple_missing() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe {
            env::remove_var("MISSING_VAR_1");
            env::remove_var("MISSING_VAR_2");
        }

        let content = "var1: ${MISSING_VAR_1}, var2: ${MISSING_VAR_2}";
        let result = expand_env_vars(content);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("MISSING_VAR_1"));
        assert!(err.contains("MISSING_VAR_2"));
    }
}
