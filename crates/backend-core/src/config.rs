use crate::error::Result;
use serde::Deserialize;
use serde_yaml::from_str;
use std::fs::read_to_string;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: Server,
    pub logging: Logging,
    pub database: Database,
    pub oauth2: Oauth2,
    pub aws: Aws,
    pub auth: Auth,
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
pub struct Auth {
    pub kc: KcAuth,
    pub bff: BffAuth,
    pub staff: StaffAuth,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KcAuth {
    pub enabled: bool,
    pub signature_secret: String,
    pub max_clock_skew_seconds: i64,
    pub max_body_bytes: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BffAuth {
    pub enabled: bool,
    pub require_bearer: bool,
    pub require_signature: bool,
    pub max_clock_skew_seconds: i64,
    pub external_id_claim: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StaffAuth {
    pub require_bearer: bool,
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

impl Config {
    pub fn api_listen_addr(&self) -> Result<SocketAddr> {
        Ok(format!("{}:{}", self.server.api.address, self.server.api.port).parse()?)
    }

    pub fn api_tls_files(&self) -> Option<(PathBuf, PathBuf)> {
        let cert_path: PathBuf = self.server.api.tls.cert_path.clone().into();
        let key_path: PathBuf = self.server.api.tls.key_path.clone().into();

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
