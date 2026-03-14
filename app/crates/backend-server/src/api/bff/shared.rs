use super::super::BackendApi;
use crate::state_machine::types::{
    INSTANCE_STATUS_ACTIVE, INSTANCE_STATUS_CANCELLED, INSTANCE_STATUS_COMPLETED,
    INSTANCE_STATUS_FAILED, STEP_PHONE_ISSUE_OTP,
};
use backend_auth::{JwtToken, normalize_user_id};
use backend_core::Error;
use backend_model::db::SmStepAttemptRow;
use gen_oas_server_bff::models;
use gen_oas_server_bff::types::Object;
use serde_json::Value;
use std::collections::HashMap;

pub(super) const KIND_KYC_ID_DOCUMENT: &str = "KYC_ID_DOCUMENT";
pub(super) const KIND_KYC_ADDRESS_PROOF: &str = "KYC_ADDRESS_PROOF";
pub(super) const KIND_KYC_PHONE_OTP: &str = crate::state_machine::types::KIND_KYC_PHONE_OTP;
pub(super) const KIND_KYC_EMAIL_MAGIC: &str = crate::state_machine::types::KIND_KYC_EMAIL_MAGIC;
pub(super) const KIND_KYC_FIRST_DEPOSIT: &str = crate::state_machine::types::KIND_KYC_FIRST_DEPOSIT;

pub(super) const OTP_RATE_LIMIT_WINDOW_MINUTES: i64 = 10;
pub(super) const OTP_RATE_LIMIT_MAX_ISSUES: i64 = 5;
pub(super) const MAGIC_RATE_LIMIT_WINDOW_MINUTES: i64 = 10;
pub(super) const MAGIC_RATE_LIMIT_MAX_ISSUES: i64 = 5;

pub(super) const OTP_STEP_TYPE: &str = "PHONE_OTP";
pub(super) const MAGIC_STEP_TYPE: &str = "EMAIL_MAGIC";
pub(super) const DEPOSIT_STEP_TYPE: &str = "PHONE_DEPOSIT";
pub(super) const OTP_VERIFY_ATTEMPT_STEP: &str = "VERIFY_OTP_ATTEMPT";
pub(super) const MAGIC_ISSUE_STEP: &str = "ISSUE_MAGIC_EMAIL";

pub(super) fn step_id(session_id: &str, step_type: &str) -> String {
    format!("{session_id}__{step_type}")
}

pub(super) fn split_step_id(id: &str) -> Option<(String, String)> {
    let (session_id, step_type) = id.rsplit_once("__")?;
    Some((session_id.to_owned(), step_type.to_owned()))
}

pub(super) fn ensure_user_match(claims: &JwtToken, expected_user_id: &str) -> Result<(), Error> {
    let authed = BackendApi::require_user_id(claims)?;
    if authed != normalize_user_id(expected_user_id) {
        tracing::warn!(
            authed_user_id = %authed,
            expected_user_id = %normalize_user_id(expected_user_id),
            "Authentication user mismatch"
        );
        return Err(Error::unauthorized(
            "Authenticated user does not match request userId",
        ));
    }
    Ok(())
}

pub(super) fn normalized_user_id(user_id: &str) -> String {
    normalize_user_id(user_id).to_owned()
}

pub(super) fn user_id_matches(stored_user_id: Option<&str>, expected_user_id: &str) -> bool {
    stored_user_id
        .map(normalize_user_id)
        .is_some_and(|stored| stored == normalize_user_id(expected_user_id))
}

pub(super) fn parse_session_status(
    instance_status: &str,
    context: &Value,
) -> models::KycSessionStatus {
    if context
        .get("phone_locked")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return models::KycSessionStatus::Locked;
    }

    match instance_status {
        INSTANCE_STATUS_ACTIVE => models::KycSessionStatus::Open,
        "RUNNING" => models::KycSessionStatus::Running,
        INSTANCE_STATUS_COMPLETED => models::KycSessionStatus::Completed,
        INSTANCE_STATUS_CANCELLED => models::KycSessionStatus::Cancelled,
        INSTANCE_STATUS_FAILED => models::KycSessionStatus::Failed,
        _ => models::KycSessionStatus::Open,
    }
}

pub(super) fn parse_step_status(
    kind: &str,
    instance_status: &str,
    step_type: &str,
    attempts: &[SmStepAttemptRow],
    context: &Value,
) -> models::KycStatus {
    if kind == KIND_KYC_PHONE_OTP && step_type == OTP_STEP_TYPE {
        if instance_status == INSTANCE_STATUS_COMPLETED {
            return models::KycStatus::Verified;
        }

        if context
            .get("phone_locked")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return models::KycStatus::Failed;
        }

        let latest_issue = attempts
            .iter()
            .filter(|a| a.step_name == STEP_PHONE_ISSUE_OTP)
            .max_by_key(|a| a.attempt_no);
        if latest_issue.is_some() {
            return models::KycStatus::InProgress;
        }
        return models::KycStatus::NotStarted;
    }

    if kind == KIND_KYC_EMAIL_MAGIC && step_type == MAGIC_STEP_TYPE {
        let latest_issue = attempts
            .iter()
            .filter(|a| a.step_name == MAGIC_ISSUE_STEP)
            .max_by_key(|a| a.attempt_no);
        if latest_issue.is_some() {
            return models::KycStatus::InProgress;
        }
        return models::KycStatus::NotStarted;
    }

    // Backward compatibility for records created before EMAIL_MAGIC got its own kind.
    if kind == KIND_KYC_PHONE_OTP
        && context
            .get("flow")
            .and_then(Value::as_str)
            .is_some_and(|flow| flow == models::KycFlowType::EmailMagic.to_string())
        && step_type == MAGIC_STEP_TYPE
    {
        let latest_issue = attempts
            .iter()
            .filter(|a| a.step_name == MAGIC_ISSUE_STEP)
            .max_by_key(|a| a.attempt_no);
        if latest_issue.is_some() {
            return models::KycStatus::InProgress;
        }
        return models::KycStatus::NotStarted;
    }

    models::KycStatus::NotStarted
}

pub(super) fn parse_step_type(raw: &str) -> Result<models::StepType, Error> {
    raw.parse::<models::StepType>().map_err(|_| {
        Error::internal(
            "INVALID_STEP_TYPE",
            format!("Unsupported step type stored: {raw}"),
        )
    })
}

pub(super) fn parse_flow(kind: &str, context: &Value) -> models::KycFlowType {
    if let Some(raw) = context.get("flow").and_then(Value::as_str)
        && let Ok(parsed) = raw.parse::<models::KycFlowType>()
    {
        return parsed;
    }

    match kind {
        KIND_KYC_FIRST_DEPOSIT => models::KycFlowType::FirstDeposit,
        KIND_KYC_EMAIL_MAGIC => models::KycFlowType::EmailMagic,
        KIND_KYC_ID_DOCUMENT => models::KycFlowType::IdDocument,
        KIND_KYC_ADDRESS_PROOF => models::KycFlowType::AddressProof,
        _ => models::KycFlowType::PhoneOtp,
    }
}

pub(super) fn flow_kind(flow: models::KycFlowType) -> &'static str {
    match flow {
        models::KycFlowType::PhoneOtp => KIND_KYC_PHONE_OTP,
        models::KycFlowType::EmailMagic => KIND_KYC_EMAIL_MAGIC,
        models::KycFlowType::FirstDeposit => KIND_KYC_FIRST_DEPOSIT,
        models::KycFlowType::IdDocument => KIND_KYC_ID_DOCUMENT,
        models::KycFlowType::AddressProof => KIND_KYC_ADDRESS_PROOF,
    }
}

pub(super) fn session_from_instance(
    instance: backend_model::db::SmInstanceRow,
) -> models::KycSession {
    let flow = parse_flow(&instance.kind, &instance.context);
    let status = parse_session_status(&instance.status, &instance.context);
    let step_ids = session_step_ids(&instance.context);

    let mut model = models::KycSession::new(
        instance.id,
        instance
            .user_id
            .as_deref()
            .map(normalize_user_id)
            .unwrap_or_default()
            .to_owned(),
        flow,
        status,
        step_ids,
        instance.created_at,
        instance.updated_at,
    );
    model.context = value_to_api_map(&instance.context);
    model
}

pub(super) fn session_step_ids(context: &Value) -> Vec<String> {
    context
        .get("step_ids")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(super) fn upsert_step_id_in_context(context: &mut Value, id: &str) -> bool {
    if !context.is_object() {
        *context = Value::Object(serde_json::Map::new());
    }

    let Some(obj) = context.as_object_mut() else {
        return false;
    };

    let mut changed = false;
    if !obj.get("step_ids").is_some_and(Value::is_array) {
        obj.insert("step_ids".to_owned(), Value::Array(vec![]));
        changed = true;
    }

    if let Some(Value::Array(ids)) = obj.get_mut("step_ids")
        && !ids.iter().any(|v| v.as_str() == Some(id))
    {
        ids.push(Value::String(id.to_owned()));
        changed = true;
    }

    changed
}

pub(super) fn put_flow_in_context(context: &mut Value, flow: models::KycFlowType) -> bool {
    if !context.is_object() {
        *context = Value::Object(serde_json::Map::new());
    }
    let Some(obj) = context.as_object_mut() else {
        return false;
    };

    let flow_value = flow.to_string();
    let current = obj.get("flow").and_then(Value::as_str);
    if current == Some(flow_value.as_str()) {
        return false;
    }

    obj.insert("flow".to_owned(), Value::String(flow_value));
    true
}

pub(super) fn ensure_step_registered(context: &Value, target_step_id: &str) -> Result<(), Error> {
    let registered = context
        .get("step_ids")
        .and_then(Value::as_array)
        .map(|ids| ids.iter().any(|v| v.as_str() == Some(target_step_id)))
        .unwrap_or(false);

    // Verify step has been created in session context before attempting operations
    if !registered {
        return Err(Error::bad_request(
            "STEP_NOT_CREATED",
            "Step must be created before this operation",
        ));
    }
    Ok(())
}

pub(super) fn api_map_to_value(map: Option<HashMap<String, Object>>) -> Option<Value> {
    map.map(|entries| {
        let mut obj = serde_json::Map::new();
        for (key, value) in entries {
            obj.insert(key, value.0);
        }
        Value::Object(obj)
    })
}

pub(super) fn value_to_api_map(value: &Value) -> Option<HashMap<String, Object>> {
    let Value::Object(obj) = value else {
        return None;
    };

    let mut map = HashMap::new();
    for (key, value) in obj {
        map.insert(key.clone(), Object(value.clone()));
    }
    Some(map)
}

pub(super) fn rate_limited_error(key: &'static str, message: &str) -> Error {
    Error::Http {
        error_key: key,
        status_code: 429,
        message: message.to_owned(),
        context: None,
    }
}

pub(super) fn is_instance_active(instance_status: &str, context: &Value) -> bool {
    let status = parse_session_status(instance_status, context);
    matches!(
        status,
        models::KycSessionStatus::Open | models::KycSessionStatus::Running
    )
}

#[cfg(test)]
mod tests {
    use super::{ensure_user_match, normalized_user_id, user_id_matches};
    use backend_auth::{Claims, JwtToken};

    fn claims(sub: &str) -> JwtToken {
        JwtToken::new(Claims {
            sub: sub.to_owned(),
            name: None,
            iss: "https://issuer.example".to_owned(),
            exp: usize::MAX,
            preferred_username: None,
        })
    }

    #[test]
    fn normalize_user_id_collapses_prefixed_identity() {
        assert_eq!(
            normalized_user_id("f:backend-user-storage:usr_123"),
            "usr_123"
        );
    }

    #[test]
    fn ensure_user_match_accepts_prefixed_expected_id() {
        let jwt = claims("usr_123");
        ensure_user_match(&jwt, "f:backend-user-storage:usr_123")
            .expect("equivalent IDs should match");
    }

    #[test]
    fn user_id_matches_compares_normalized_values() {
        assert!(user_id_matches(
            Some("f:backend-user-storage:usr_123"),
            "usr_123"
        ));
        assert!(!user_id_matches(Some("usr_999"), "usr_123"));
    }
}
