use thiserror::Error;

#[derive(Debug, Error)]
pub enum IdError {
    #[error("failed to create cuid: {0}")]
    Cuid(#[from] cuid::CuidError),
}

pub type Result<T> = std::result::Result<T, IdError>;

pub fn prefixed(prefix: &str) -> Result<String> {
    Ok(format!("{prefix}_{}", cuid::cuid1()?))
}

pub fn user_id() -> Result<String> {
    prefixed("usr")
}

pub fn device_id() -> Result<String> {
    prefixed("dvc")
}

pub fn approval_id() -> Result<String> {
    prefixed("apr")
}

pub fn sms_hash() -> Result<String> {
    prefixed("sms")
}

pub fn kyc_document_id() -> Result<String> {
    prefixed("kyd")
}
