use chrono::{DateTime, Utc};
use o2o::o2o;
use serde_json::Value;

use crate::db;

pub type KcMap = std::collections::HashMap<String, String>;
pub type KcAnyMap = std::collections::HashMap<String, gen_oas_server_kc::types::Object>;

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

#[derive(Debug, Clone, o2o)]
#[from_owned(gen_oas_server_kc::models::DeviceDescriptor)]
pub struct DeviceDescriptor {
    pub device_id: String,
    pub jkt: String,
    #[map(public_jwk)]
    pub public_jwk: Option<KcAnyMap>,
    pub platform: Option<String>,
    pub model: Option<String>,
    pub app_version: Option<String>,
}

#[derive(Debug, Clone, o2o)]
#[from_owned(gen_oas_server_kc::models::ApprovalCreateRequest)]
pub struct ApprovalCreateRequest {
    pub realm: String,
    pub client_id: String,
    pub user_id: String,
    pub new_device: gen_oas_server_kc::models::DeviceDescriptor,
    pub reason: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    #[map(context)]
    pub context: Option<KcAnyMap>,
}

#[derive(Debug, Clone, o2o)]
#[from_owned(gen_oas_server_kc::models::ApprovalDecisionRequest)]
pub struct ApprovalDecisionRequest {
    pub decision: gen_oas_server_kc::models::QueryApprovalStatus,
    pub decided_by_device_id: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, o2o)]
#[from_owned(gen_oas_server_kc::models::DeviceLookupRequest)]
pub struct DeviceLookupRequest {
    pub device_id: Option<String>,
    pub jkt: Option<String>,
}

#[derive(Debug, Clone, o2o)]
#[from_owned(gen_oas_server_kc::models::EnrollmentPrecheckRequest)]
pub struct EnrollmentPrecheckRequest {
    pub realm: String,
    pub client_id: String,
    pub user_hint: Option<String>,
    pub device_id: String,
    pub jkt: String,
    #[map(public_jwk)]
    pub public_jwk: Option<KcAnyMap>,
    #[map(proof_context)]
    pub proof_context: Option<KcAnyMap>,
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
#[from_owned(gen_oas_server_kc::models::SmsSendRequest)]
pub struct SmsSendRequest {
    pub realm: String,
    pub client_id: String,
    pub user_id: Option<String>,
    pub phone_number: String,
    pub otp: String,
    pub session_id: Option<String>,
    pub trace_id: Option<String>,
    #[map(metadata)]
    pub metadata: Option<KcAnyMap>,
}

#[derive(Debug, Clone, o2o)]
#[from_owned(gen_oas_server_kc::models::SmsConfirmRequest)]
pub struct SmsConfirmRequest {
    pub hash: String,
    pub otp: String,
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
}

pub fn kc_any_map_to_value(map: KcAnyMap) -> Value {
    let mut out = serde_json::Map::new();
    for (k, v) in map {
        out.insert(k, v.0);
    }
    Value::Object(out)
}

impl From<db::UserRow> for UserRecordDto {
    fn from(row: db::UserRow) -> Self {
        Self {
            user_id: row.user_id,
            realm: Some(row.realm),
            username: row.username,
            first_name: row.first_name,
            last_name: row.last_name,
            email: row.email,
            enabled: row.enabled,
            email_verified: row.email_verified,
            created_at: Some(row.created_at),
            attributes: Self::parse_attributes(row.attributes),
        }
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
        }
    }
}

#[derive(Debug, Clone, o2o)]
#[owned_into(gen_oas_server_kc::models::ApprovalStatusResponse)]
pub struct ApprovalStatusDto {
    pub request_id: String,
    pub status: gen_oas_server_kc::models::QueryApprovalStatus,
    pub decided_at: Option<DateTime<Utc>>,
    pub decided_by_device_id: Option<String>,
    pub message: Option<String>,
}

impl ApprovalStatusDto {
    fn parse_status(s: &str) -> gen_oas_server_kc::models::QueryApprovalStatus {
        s.parse()
            .unwrap_or(gen_oas_server_kc::models::QueryApprovalStatus::Pending)
    }
}

impl From<db::ApprovalRow> for ApprovalStatusDto {
    fn from(row: db::ApprovalRow) -> Self {
        Self {
            request_id: row.request_id,
            status: Self::parse_status(&row.status),
            decided_at: row.decided_at,
            decided_by_device_id: row.decided_by_device_id,
            message: row.message,
        }
    }
}

#[derive(Debug, Clone, o2o)]
#[owned_into(gen_oas_server_kc::models::UserApprovalRecord)]
pub struct UserApprovalRecordDto {
    pub request_id: String,
    pub user_id: String,
    pub device_id: String,
    pub status: gen_oas_server_kc::models::UserApprovalRecordStatus,
    pub created_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decided_by_device_id: Option<String>,
    pub message: Option<String>,
}

impl UserApprovalRecordDto {
    fn parse_status(s: &str) -> gen_oas_server_kc::models::UserApprovalRecordStatus {
        s.parse()
            .unwrap_or(gen_oas_server_kc::models::UserApprovalRecordStatus::Pending)
    }
}

impl From<db::ApprovalRow> for UserApprovalRecordDto {
    fn from(row: db::ApprovalRow) -> Self {
        Self {
            request_id: row.request_id,
            user_id: row.user_id,
            device_id: row.device_id,
            status: Self::parse_status(&row.status),
            created_at: row.created_at,
            decided_at: row.decided_at,
            decided_by_device_id: row.decided_by_device_id,
            message: row.message,
        }
    }
}
