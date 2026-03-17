use backend_core::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Canonical payload for device-bound signature auth.
/// Format: JSON with keys in specific order matching frontend
/// {"deviceId":"...","publicKey":"...","ts":"...","nonce":"..."}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct DeviceSignaturePayload {
    device_id: String,
    public_key: String,
    ts: String,
    nonce: String,
}

/// Canonical payload format for signature auth:
/// timestamp\nnonce\nMETHOD\nPATH\nBODY\nPUBLIC_KEY\nDEVICE_ID\nUSER_ID_HINT
#[allow(clippy::too_many_arguments)]
pub fn canonicalize_payload(
    timestamp: i64,
    nonce: &str,
    method: &str,
    path: &str,
    body: &str,
    public_key_json: &str,
    device_id: &str,
    user_id_hint: Option<&str>,
) -> Result<String> {
    let provided_public_key = canonicalize_public_key(public_key_json)?;

    let canonical = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        timestamp,
        nonce,
        method.to_uppercase(),
        path,
        body,
        provided_public_key,
        device_id,
        user_id_hint.unwrap_or_default(),
    );

    Ok(canonical)
}

/// Canonical payload for device authentication (used by BFF/frontend).
/// Creates a JSON string with keys in specific order matching frontend:
/// {"deviceId":"...","publicKey":"...","ts":"...","nonce":"..."}
pub fn canonicalize_device_auth_payload(
    device_id: &str,
    nonce: &str,
    public_key_json: &str,
    timestamp: i64,
) -> Result<String> {
    let escaped_public_key = public_key_json.replace('\\', "\\\\").replace('"', "\\\"");

    let canonical = format!(
        r#"{{"deviceId":"{}","publicKey":"{}","ts":"{}","nonce":"{}"}}"#,
        device_id, escaped_public_key, timestamp, nonce
    );

    Ok(canonical)
}

/// Parses a JWK JSON string, sorts its keys, and returns the canonicalized string.
pub fn canonicalize_public_key(raw: &str) -> Result<String> {
    let parsed: Value = serde_json::from_str(raw)
        .map_err(|_| Error::unauthorized("x-auth-public-key must be valid JSON"))?;

    let object = parsed
        .as_object()
        .ok_or_else(|| Error::unauthorized("x-auth-public-key must be a JSON object"))?;

    let sorted = object
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<String, Value>>();

    serde_json::to_string(&sorted)
        .map_err(|error| Error::internal("PUBLIC_KEY_SERIALIZATION_FAILED", error.to_string()))
}
