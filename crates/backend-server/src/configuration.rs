use backend_core::{Config, Error, Result};
use std::net::SocketAddr;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct BackendServerConfig {
    pub listen_addr: SocketAddr,
    pub tls: Option<TlsConfig>,
    pub database_url: String,
    pub database_pool_size: u32,
    pub aws: AwsConfig,
}

#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct AwsConfig {
    pub region: String,
    pub s3: AwsS3Config,
    pub sns: AwsSnsConfig,
}

#[derive(Debug, Clone)]
pub struct AwsS3Config {
    pub bucket: String,
    pub endpoint: Option<String>,
    pub presign_ttl_seconds: u64,
}

#[derive(Debug, Clone)]
pub struct AwsSnsConfig {
    pub region: Option<String>,
    pub max_attempts: u32,
    pub initial_backoff_seconds: u64,
}

impl TryFrom<&Config> for BackendServerConfig {
    type Error = Error;

    fn try_from(cfg: &Config) -> Result<Self> {
        let listen_addr: SocketAddr = format!("{}:{}", cfg.server.api.address, cfg.server.api.port)
            .parse()
            .map_err(Error::AddrParseError)?;

        Ok(Self {
            listen_addr,
            tls: resolve_tls(cfg),
            database_url: cfg.database.url.clone(),
            database_pool_size: cfg.database.pool_size.unwrap_or(10),
            aws: AwsConfig {
                region: cfg.aws.region.clone(),
                s3: AwsS3Config {
                    bucket: cfg.aws.s3.bucket.clone(),
                    endpoint: cfg.aws.s3.endpoint.clone(),
                    presign_ttl_seconds: cfg.aws.s3.presign_ttl_seconds,
                },
                sns: AwsSnsConfig {
                    region: cfg.aws.sns.region.clone(),
                    max_attempts: cfg.aws.sns.max_attempts,
                    initial_backoff_seconds: cfg.aws.sns.initial_backoff_seconds,
                },
            },
        })
    }
}

fn resolve_tls(cfg: &Config) -> Option<TlsConfig> {
    let cert_path: PathBuf = cfg.server.api.tls.cert_path.clone().into();
    let key_path: PathBuf = cfg.server.api.tls.key_path.clone().into();

    if Path::new(&cert_path).exists() && Path::new(&key_path).exists() {
        Some(TlsConfig { cert_path, key_path })
    } else {
        None
    }
}
