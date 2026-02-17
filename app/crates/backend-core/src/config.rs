use crate::error::{Error, Result};
use regex::Regex;
use serde::Deserialize;
use serde_yaml::from_str;
use std::fs::read_to_string;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeMode {
    Server,
    Worker,
    Shared,
}

impl Default for RuntimeMode {
    fn default() -> Self {
        Self::Shared
    }
}

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

#[derive(Debug, Clone, Deserialize)]
pub struct Server {
    pub address: String,
    pub port: u16,
    pub tls: Tls,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Logging {
    pub level: String,
    pub data_dir: Option<String>,
    pub json: Option<bool>,
    pub flame: Option<bool>,
    #[serde(default)]
    pub log_requests_enabled: bool,
    #[serde(default)]
    pub request_logging: RequestLogging,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RequestLogging {
    pub enabled: bool,
}

impl Default for RequestLogging {
    fn default() -> Self {
        Self { enabled: false }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Database {
    pub url: String,
    pub pool_size: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Oauth2 {
    pub jwks_url: String,
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

#[derive(Debug, Clone, Deserialize)]
pub struct AwsSns {
    /// Optional region override for SNS.
    pub region: Option<String>,
    /// Maximum publish attempts before giving up.
    pub max_attempts: u32,
    /// Initial retry backoff in seconds.
    pub initial_backoff_seconds: u64,
}

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
    pub sns: Option<AwsSns>,

    pub kc: KcAuth,
    pub bff: BffAuth,
    pub staff: StaffAuth,
    pub cuss: Cuss,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tls {
    pub cert_path: String,
    pub key_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KcAuth {
    pub enabled: bool,
    #[serde(alias = "base-path")]
    pub base_path: String,
    pub signature_secret: String,
    pub max_clock_skew_seconds: i64,
    pub max_body_bytes: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BffAuth {
    pub enabled: bool,
    #[serde(alias = "base-path")]
    pub base_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StaffAuth {
    pub enabled: bool,
    #[serde(alias = "base-path")]
    pub base_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Cuss {
    pub api_url: String,
}

pub fn load_from_path<P: AsRef<std::path::Path>>(path: P) -> Result<Config> {
    let content = read_to_string(path)?;
    let expanded = expand_env_vars(&content)?;
    let cfg: Config = from_str(&expanded)?;
    Ok(cfg)
}

fn expand_env_vars(content: &str) -> Result<String> {
    let re = Regex::new(r"\$\{([a-zA-Z_][a-zA-Z0-9_]*)\}").map_err(|e| Error::Server(e.to_string()))?;
    let mut result = content.to_string();
    let mut missing_vars = Vec::new();

    for cap in re.captures_iter(content) {
        let full_match = &cap[0];
        let var_name = &cap[1];
        match std::env::var(var_name) {
            Ok(val) => {
                result = result.replace(full_match, &val);
            }
            Err(_) => {
                missing_vars.push(var_name.to_string());
            }
        }
    }

    if !missing_vars.is_empty() {
        return Err(Error::Server(format!(
            "Missing environment variables: {}",
            missing_vars.join(", ")
        )));
    }

    Ok(result)
}

impl Config {
    pub fn api_listen_addr(&self) -> Result<SocketAddr> {
        Ok(format!("{}:{}", self.server.address, self.server.port).parse()?)
    }

    pub fn api_tls_files(&self) -> Option<(PathBuf, PathBuf)> {
        let cert_path: PathBuf = self.server.tls.cert_path.clone().into();
        let key_path: PathBuf = self.server.tls.key_path.clone().into();

        if Path::new(&cert_path).exists() && Path::new(&key_path).exists() {
            Some((cert_path, key_path))
        } else {
            None
        }
    }

    pub fn database_pool_size(&self) -> u32 {
        self.database.pool_size.unwrap_or(10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_expand_env_vars_success() {
        unsafe {
            env::set_var("TEST_VAR_1", "value1");
            env::set_var("TEST_VAR_2", "value2");
        }

        let content = "var1: ${TEST_VAR_1}, var2: ${TEST_VAR_2}";
        let expanded = expand_env_vars(content).unwrap();

        assert_eq!(expanded, "var1: value1, var2: value2");
    }

    #[test]
    fn test_expand_env_vars_missing() {
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
