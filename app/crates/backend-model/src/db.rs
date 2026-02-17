use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde_json::Value;

#[derive(Debug, Clone, Queryable, Selectable, Insertable, Identifiable)]
#[diesel(table_name = crate::schema::app_user)]
#[diesel(primary_key(user_id))]
pub struct UserRow {
    pub user_id: String,
    pub realm: String,
    pub username: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub email_verified: bool,
    pub phone_number: Option<String>,
    pub fineract_customer_id: Option<String>,
    pub disabled: bool,
    pub attributes: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Queryable, Selectable, Insertable, Identifiable)]
#[diesel(table_name = crate::schema::device)]
#[diesel(primary_key(device_id))]
pub struct DeviceRow {
    pub device_id: String,
    pub user_id: String,
    pub jkt: String,
    pub public_jwk: String,
    pub status: String,
    pub label: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Queryable, Selectable, Insertable, Identifiable)]
#[diesel(table_name = crate::schema::approval)]
#[diesel(primary_key(request_id))]
pub struct ApprovalRow {
    pub request_id: String,
    pub user_id: String,
    pub new_device_id: String,
    pub new_device_jkt: String,
    pub new_device_public_jwk: String,
    pub new_device_platform: Option<String>,
    pub new_device_model: Option<String>,
    pub new_device_app_version: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decided_by_device_id: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Queryable, Selectable, Insertable, Identifiable)]
#[diesel(table_name = crate::schema::sms_messages)]
#[diesel(primary_key(id))]
pub struct SmsMessageRow {
    pub id: String,
    pub realm: String,
    pub client_id: String,
    pub user_id: Option<String>,
    pub phone_number: String,
    pub hash: String,
    pub otp_sha256: Vec<u8>,
    pub ttl_seconds: i32,
    pub status: String,
    pub attempt_count: i32,
    pub max_attempts: i32,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub sns_message_id: Option<String>,
    pub session_id: Option<String>,
    pub trace_id: Option<String>,
    pub metadata: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
    pub confirmed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Queryable, Selectable, Insertable, Identifiable)]
#[diesel(table_name = crate::schema::kyc_case)]
pub struct KycCaseRow {
    pub id: String,
    pub user_id: String,
    pub case_status: String,
    pub active_submission_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Queryable, Selectable, Insertable, Identifiable)]
#[diesel(table_name = crate::schema::kyc_submission)]
pub struct KycSubmissionRow {
    pub id: String,
    pub kyc_case_id: String,
    pub version: i32,
    pub status: String,
    pub submitted_at: Option<DateTime<Utc>>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decided_by: Option<String>,
    pub provisioning_status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub phone_number: Option<String>,
    pub date_of_birth: Option<String>,
    pub nationality: Option<String>,
    pub rejection_reason: Option<String>,
    pub review_notes: Option<String>,
}

#[derive(Debug, Clone, Queryable, Selectable, Insertable, Identifiable)]
#[diesel(table_name = crate::schema::kyc_document)]
pub struct KycDocumentRow {
    pub id: String,
    pub submission_id: String,
    pub doc_type: String,
    pub s3_bucket: String,
    pub s3_key: String,
    pub file_name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub status: String,
    pub uploaded_at: DateTime<Utc>,
}
