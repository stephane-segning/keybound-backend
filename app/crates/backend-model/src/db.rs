use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct UserRow {
    pub user_id: String,
    pub realm: String,
    pub username: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub enabled: bool,
    pub email_verified: bool,
    pub attributes: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DeviceRow {
    pub id: String,
    pub realm: String,
    pub client_id: String,
    pub user_id: String,
    pub user_hint: Option<String>,
    pub device_id: String,
    pub jkt: String,
    pub status: String, // device_status::text
    pub public_jwk: Value,
    pub attributes: Option<Value>,
    pub proof: Option<Value>,
    pub label: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ApprovalRow {
    pub request_id: String,
    pub realm: String,
    pub client_id: String,
    pub user_id: String,
    pub device_id: String,
    pub jkt: String,
    pub public_jwk: Option<Value>,
    pub platform: Option<String>,
    pub model: Option<String>,
    pub app_version: Option<String>,
    pub reason: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub context: Option<Value>,
    pub idempotency_key: Option<String>,
    pub status: String, // approval_status::text
    pub created_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decided_by_device_id: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct SmsMessageRow {
    pub id: String,
    pub realm: String,
    pub client_id: String,
    pub user_id: Option<String>,
    pub phone_number: String,
    pub hash: String,
    pub otp_sha256: Vec<u8>,
    pub ttl_seconds: Option<i32>,
    pub status: String, // sms_status::text
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

#[derive(Debug, Clone, FromRow)]
pub struct KycProfileRow {
    pub external_id: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub phone_number: Option<String>,
    pub date_of_birth: Option<String>,
    pub nationality: Option<String>,
    pub kyc_tier: i32,
    pub kyc_status: String, // kyc_status::text
    pub submitted_at: Option<DateTime<Utc>>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub reviewed_by: Option<String>,
    pub rejection_reason: Option<String>,
    pub review_notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i32,
}

#[derive(Debug, Clone, FromRow)]
pub struct KycDocumentRow {
    pub id: String,
    pub external_id: String,
    pub document_type: String,
    pub status: String, // kyc_document_status::text
    pub uploaded_at: Option<DateTime<Utc>>,
    pub rejection_reason: Option<String>,
    pub file_name: String,
    pub mime_type: String,
    pub content_length: i64,
    pub s3_bucket: String,
    pub s3_key: String,
    pub presigned_expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
