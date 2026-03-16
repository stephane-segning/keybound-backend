pub mod canonical;
pub mod replay;
pub mod verify;

pub use canonical::{canonicalize_device_auth_payload, canonicalize_payload, canonicalize_public_key};
#[cfg(any(test, feature = "test-utils"))]
pub use replay::in_memory::{InMemoryReplayGuard, SharedInMemoryReplayGuard};
pub use replay::{RedisReplayGuard, ReplayGuard};
pub use verify::verify_signature;

use backend_core::{Error, Result};
use chrono::Utc;

/// Validates that the timestamp is within the allowed clock skew.
pub fn validate_timestamp(timestamp: i64, max_skew_seconds: i64) -> Result<()> {
    let now = Utc::now().timestamp();
    let skew = (now - timestamp).abs();

    if skew > max_skew_seconds {
        return Err(Error::unauthorized("Timestamp out of skew"));
    }

    Ok(())
}

/// Validates that the provided public key matches the bound device key.
pub fn validate_public_key_match(provided_jwk: &str, bound_jwk: &str) -> Result<()> {
    let provided = canonicalize_public_key(provided_jwk)?;
    let bound = canonicalize_public_key(bound_jwk)?;

    if provided != bound {
        return Err(Error::unauthorized(
            "x-auth-public-key does not match bound device key",
        ));
    }

    Ok(())
}

/// Validates that the optional user ID hint matches the device owner.
pub fn validate_user_id_hint(hint: Option<&str>, device_owner_id: &str) -> Result<()> {
    if let Some(hint_id) = hint {
        if hint_id != device_owner_id {
            return Err(Error::unauthorized(
                "x-auth-user-id does not match device owner",
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_timestamp_within_skew() {
        let now = Utc::now().timestamp();
        assert!(validate_timestamp(now, 300).is_ok());
        assert!(validate_timestamp(now - 100, 300).is_ok());
        assert!(validate_timestamp(now + 100, 300).is_ok());
    }

    #[test]
    fn test_validate_timestamp_outside_skew() {
        let now = Utc::now().timestamp();
        assert!(validate_timestamp(now - 400, 300).is_err());
        assert!(validate_timestamp(now + 400, 300).is_err());
    }

    #[test]
    fn test_validate_user_id_hint_matches() {
        assert!(validate_user_id_hint(Some("usr_123"), "usr_123").is_ok());
        assert!(validate_user_id_hint(None, "usr_123").is_ok());
    }

    #[test]
    fn test_validate_user_id_hint_mismatch() {
        assert!(validate_user_id_hint(Some("usr_456"), "usr_123").is_err());
    }

    #[test]
    fn test_canonicalize_payload() {
        let result = canonicalize_payload(
            1234567890,
            "nonce123",
            "POST",
            "/api/test",
            "{\"data\":\"value\"}",
            "{\"kty\":\"EC\"}",
            "dvc_123",
            Some("usr_456"),
        )
        .unwrap();

        assert_eq!(
            result,
            "1234567890\nnonce123\nPOST\n/api/test\n{\"data\":\"value\"}\n{\"kty\":\"EC\"}\ndvc_123\nusr_456"
        );
    }

    #[test]
    fn test_canonicalize_payload_no_user_hint() {
        let result = canonicalize_payload(
            1234567890,
            "nonce123",
            "GET",
            "/api/test",
            "",
            "{\"kty\":\"EC\"}",
            "dvc_123",
            None,
        )
        .unwrap();

        assert_eq!(
            result,
            "1234567890\nnonce123\nGET\n/api/test\n\n{\"kty\":\"EC\"}\ndvc_123\n"
        );
    }

    #[test]
    fn test_canonicalize_public_key_sorts_keys() {
        let unsorted = r#"{"kty":"EC","crv":"P-256","x":"abc","y":"def"}"#;
        let sorted = canonicalize_public_key(unsorted).unwrap();

        assert!(sorted.starts_with("{\"crv\":\"P-256\",\"kty\":\"EC\""));
    }

    #[test]
    fn test_canonicalize_public_key_invalid_json() {
        let result = canonicalize_public_key("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_canonicalize_device_auth_payload() {
        let public_key = r#"{"crv":"P-256","kty":"EC","x":"abc","y":"def"}"#;
        let result = canonicalize_device_auth_payload(
            "dvc_123",
            "nonce456",
            public_key,
            1234567890,
        )
        .unwrap();

        assert!(result.starts_with(r#"{"deviceId":"dvc_123","publicKey":"{"#));
        assert!(result.contains(r#""ts":"1234567890""#));
        assert!(result.contains(r#""nonce":"nonce456""#));
    }
}
