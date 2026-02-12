use backend_core::{Error, Result};

pub fn prefixed(prefix: &str) -> Result<String> {
    let id = cuid::cuid1().map_err(|e| Error::Server(e.to_string()))?;
    Ok(format!("{prefix}_{id}"))
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
