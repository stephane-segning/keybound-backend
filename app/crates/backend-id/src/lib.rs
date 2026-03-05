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

pub fn kyc_otp_ref() -> Result<String> {
    prefixed("otp")
}

pub fn kyc_magic_ref() -> Result<String> {
    prefixed("mgc")
}

pub fn kyc_upload_id() -> Result<String> {
    prefixed("upl")
}

pub fn kyc_evidence_id() -> Result<String> {
    prefixed("evi")
}

pub fn sm_instance_id() -> Result<String> {
    prefixed("smi")
}

pub fn sm_event_id() -> Result<String> {
    prefixed("sme")
}

pub fn sm_attempt_id() -> Result<String> {
    prefixed("sma")
}
