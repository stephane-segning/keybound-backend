//! Data transfer objects for KC (Keycloak) API surface.
//!
//! These types use the o2o crate for automatic conversion from generated
//! OpenAPI types (gen_oas_server_kc) to internal domain types.

use chrono::{DateTime, Utc};
use hex::encode;
use o2o::o2o;
use serde_json::Value;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::db;

pub const USER_DATA_NAME_REGISTRATION_OUTPUT: &str = "registration_output";
pub const USER_DATA_TYPE_REGISTRATION_OUTPUT: &str = "cuss.registration_response";

/// Keycloak attribute map (string -> string)
pub type KcMap = std::collections::HashMap<String, String>;
/// Keycloak any-type attribute map (string -> Object)
pub type KcAnyMap = std::collections::HashMap<String, gen_oas_server_kc::types::Object>;

/// User upsert request from Keycloak.
#[derive(Debug, Clone, o2o)]
#[from_owned(gen_oas_server_kc::models::UserUpsertRequest)]
pub struct UserUpsert {
    pub realm: String,
    pub username: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub enabled: Option<bool>,
    pub email_verified: Option<bool>,
    pub attributes: Option<KcMap>,
}

/// User search request from Keycloak.
#[derive(Debug, Clone, o2o)]
#[from_owned(gen_oas_server_kc::models::UserSearchRequest)]
pub struct UserSearch {
    pub realm: String,
    pub search: Option<String>,
    pub username: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub enabled: Option<bool>,
    pub email_verified: Option<bool>,
    pub exact: Option<bool>,
    pub attributes: Option<KcMap>,
    pub first_result: Option<i32>,
    pub max_results: Option<i32>,
}

/// Device descriptor for device binding operations.
#[derive(Debug, Clone, o2o)]
#[from_owned(gen_oas_server_kc::models::DeviceDescriptor)]
pub struct DeviceDescriptor {
    pub device_id: String,
    pub jkt: String,
    #[map(public_jwk)]
    pub public_jwk: Option<KcAnyMap>,
    pub platform: String,
    pub model: String,
    pub app_version: Option<String>,
}

/// Device lookup request from Keycloak.
#[derive(Debug, Clone, o2o)]
#[from_owned(gen_oas_server_kc::models::DeviceLookupRequest)]
pub struct DeviceLookupRequest {
    pub device_id: Option<String>,
    pub jkt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ApprovalCreateRequest {
    pub realm: String,
    pub client_id: String,
    pub user_id: String,
    pub new_device: DeviceDescriptor,
    pub reason: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub context: Option<KcAnyMap>,
}

#[derive(Debug, Clone)]
pub struct ApprovalDecisionRequest {
    pub decision: String,
    pub decided_by_device_id: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EnrollmentPrecheckRequest {
    pub realm: String,
    pub client_id: String,
    pub user_hint: Option<String>,
    pub device_id: String,
    pub jkt: String,
    pub public_jwk: Option<KcAnyMap>,
    pub proof_context: Option<KcAnyMap>,
}

#[derive(Debug, Clone)]
pub struct SmsSendRequest {
    pub realm: String,
    pub client_id: String,
    pub user_id: Option<String>,
    pub phone_number: String,
    pub session_id: Option<String>,
    pub trace_id: Option<String>,
    pub metadata: Option<KcAnyMap>,
}

#[derive(Debug, Clone)]
pub struct SmsConfirmRequest {
    pub hash: String,
    pub otp: String,
}

#[derive(Debug, Clone, o2o)]
#[from_owned(gen_oas_server_kc::models::EnrollmentBindRequest)]
pub struct EnrollmentBindRequest {
    pub realm: String,
    pub client_id: String,
    pub user_id: String,
    pub user_hint: Option<String>,
    pub device_id: String,
    pub jkt: String,
    #[map(public_jwk)]
    pub public_jwk: KcAnyMap,
    pub attributes: Option<KcMap>,
    pub created_at: Option<DateTime<Utc>>,
    #[map(proof)]
    pub proof: Option<KcAnyMap>,
}

#[derive(Debug, Clone, o2o)]
#[owned_into(gen_oas_server_kc::models::UserRecord)]
pub struct UserRecordDto {
    pub user_id: String,
    pub realm: Option<String>,
    pub username: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub enabled: bool,
    pub email_verified: bool,
    pub created_at: Option<DateTime<Utc>>,
    pub attributes: Option<KcMap>,
    pub custom: Option<KcMap>,
}

impl UserRecordDto {
    fn parse_attributes(value: Option<Value>) -> Option<KcMap> {
        let Value::Object(map) = value? else {
            return None;
        };

        let mut out = KcMap::new();
        for (k, v) in map {
            if let Some(s) = v.as_str() {
                out.insert(k, s.to_string());
            }
        }
        Some(out)
    }

    fn build_parameters_json(user_data_rows: &[db::UserDataRow]) -> Option<Value> {
        if user_data_rows.is_empty() {
            return None;
        }

        let mut by_name = serde_json::Map::new();
        for row in user_data_rows {
            let entry = json!({
                "data_type": row.data_type,
                "content": row.content,
            });

            if let Some(existing) = by_name.get_mut(&row.name) {
                match existing {
                    Value::Array(arr) => arr.push(entry),
                    _ => {
                        let previous = existing.clone();
                        *existing = Value::Array(vec![previous, entry]);
                    }
                }
            } else {
                by_name.insert(row.name.clone(), entry);
            }
        }

        Some(Value::Object(by_name))
    }

    fn build_custom(
        attributes: Option<KcMap>,
        user_data_rows: &[db::UserDataRow],
    ) -> Option<KcMap> {
        let mut custom = attributes.unwrap_or_default();

        if let Some(parameters) = Self::build_parameters_json(user_data_rows) {
            custom.insert("parameters".to_owned(), parameters.to_string());
        }

        if custom.is_empty() {
            None
        } else {
            Some(custom)
        }
    }

    pub fn from_row_with_user_data(row: db::UserRow, user_data_rows: &[db::UserDataRow]) -> Self {
        let (first_name, last_name) = match row.full_name.clone() {
            Some(full) => (Some(full), Some(String::new())),
            None => (None, None),
        };

        let attributes = Self::parse_attributes(row.attributes.clone());
        let custom = Self::build_custom(attributes.clone(), user_data_rows);

        Self {
            user_id: row.user_id,
            realm: Some(row.realm),
            username: row.username,
            first_name,
            last_name,
            email: row.email,
            enabled: !row.disabled,
            email_verified: row.email_verified,
            created_at: Some(row.created_at),
            attributes,
            custom,
        }
    }
}

pub fn kc_any_map_to_value(map: KcAnyMap) -> Value {
    let mut out = serde_json::Map::new();
    for (k, v) in map {
        out.insert(k, v.0);
    }
    Value::Object(out)
}

pub fn device_record_id(device_id: &str, public_jwk: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(public_jwk.as_bytes());
    let digest = hasher.finalize();
    let hash = encode(digest);
    format!("{device_id}:{hash}")
}

#[cfg(test)]
mod tests {
    use super::{UserRecordDto, device_record_id};
    use crate::db;
    use chrono::Utc;
    use hex::encode as hex_encode;
    use serde_json::json;
    use sha2::{Digest, Sha256};

    #[test]
    fn device_record_id_is_deterministic() {
        let device_id = "dvc_test";
        let public_jwk = "{\"a\":1,\"b\":2}";
        let expected_hash = {
            let mut hasher = Sha256::new();
            hasher.update(public_jwk.as_bytes());
            hex_encode(hasher.finalize())
        };

        assert_eq!(
            device_record_id(device_id, public_jwk),
            format!("{device_id}:{expected_hash}")
        );
    }

    #[test]
    fn user_record_custom_includes_parameters_from_user_data() {
        let row = db::UserRow {
            user_id: "usr_test".to_owned(),
            realm: "realm-a".to_owned(),
            username: "alice".to_owned(),
            full_name: Some("Alice Tester".to_owned()),
            email: Some("alice@example.test".to_owned()),
            email_verified: true,
            phone_number: Some("+4912345678".to_owned()),
            fineract_customer_id: None,
            disabled: false,
            attributes: Some(json!({ "tier": "gold" })),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let user_data = vec![db::UserDataRow {
            user_id: "usr_test".to_owned(),
            name: "registration_output".to_owned(),
            data_type: "cuss.registration_response".to_owned(),
            content: json!({
                "fineractClientId": 12345,
                "savingsAccountId": 45678
            }),
            eager_fetch: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }];

        let dto = UserRecordDto::from_row_with_user_data(row, &user_data);
        let custom = dto.custom.expect("custom map");
        assert_eq!(custom.get("tier"), Some(&"gold".to_owned()));

        let params = custom
            .get("parameters")
            .expect("parameters key should exist");
        let parsed: serde_json::Value =
            serde_json::from_str(params).expect("parameters should be valid json");
        assert_eq!(
            parsed.get("registration_output"),
            Some(&json!({
                "data_type": "cuss.registration_response",
                "content": {
                    "fineractClientId": 12345,
                    "savingsAccountId": 45678
                }
            }))
        );
    }
}

impl From<db::UserRow> for UserRecordDto {
    fn from(row: db::UserRow) -> Self {
        Self::from_row_with_user_data(row, &[])
    }
}

#[derive(Debug, Clone, o2o)]
#[owned_into(gen_oas_server_kc::models::DeviceRecord)]
pub struct DeviceRecordDto {
    pub device_id: String,
    pub jkt: String,
    pub status: gen_oas_server_kc::models::DeviceRecordStatus,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub label: Option<String>,
    pub device_os: Option<String>,
    pub device_model: Option<String>,
    pub device_app_version: Option<String>,
}

impl DeviceRecordDto {
    fn parse_status(status: &str) -> gen_oas_server_kc::models::DeviceRecordStatus {
        status
            .parse()
            .unwrap_or(gen_oas_server_kc::models::DeviceRecordStatus::Active)
    }
}

impl From<db::DeviceRow> for DeviceRecordDto {
    fn from(row: db::DeviceRow) -> Self {
        Self {
            device_id: row.device_id,
            jkt: row.jkt,
            status: Self::parse_status(&row.status),
            created_at: row.created_at,
            last_seen_at: row.last_seen_at,
            label: row.label,
            device_os: None,
            device_model: None,
            device_app_version: None,
        }
    }
}
