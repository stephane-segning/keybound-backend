use crate::error::Result;
use serde::Deserialize;
use serde_yaml::from_str;
use std::fs::read_to_string;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: Server,
    pub logging: Logging,
    pub database: Database,
    pub oauth2: Oauth2,
    pub aws: Aws,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Server {
    pub api: ApiServer,
    pub basic_auth: BasicAuth,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiServer {
    pub address: String,
    pub port: u16,
    pub tls: Tls,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tls {
    pub cert_path: String,
    pub key_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BasicAuth {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Logging {
    pub level: String,
    pub data_dir: Option<String>,
    pub json: Option<bool>,
    pub flame: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Database {
    pub url: String,
    pub pool_size: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Oauth2 {
    pub jwks_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Aws {
    pub region: String,
    pub s3: AwsS3,
    pub sns: AwsSns,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AwsS3 {
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

pub fn load_from_path<P: AsRef<std::path::Path>>(path: P) -> Result<Config> {
    let content = read_to_string(path)?;
    let cfg: Config = from_str(&content)?;
    Ok(cfg)
}
