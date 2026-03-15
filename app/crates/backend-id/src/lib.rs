//! ID generation utilities following application conventions.
#![allow(clippy::result_large_err)]
//!
//! Provides functions to generate strongly-typed, globally unique identifiers
//! for various domain entities. All IDs use a prefix CUID pattern for
//! improved readability and debugging (e.g. `usr_abc123`)
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

pub fn flow_session_id() -> Result<String> {
    prefixed("ses")
}

pub fn flow_instance_id() -> Result<String> {
    prefixed("flw")
}

pub fn flow_step_id() -> Result<String> {
    prefixed("stp")
}

pub fn signing_key_id() -> Result<String> {
    prefixed("kid")
}
