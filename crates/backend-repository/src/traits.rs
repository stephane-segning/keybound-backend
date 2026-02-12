use chrono::{DateTime, Utc};

pub type RepoResult<T> = backend_core::Result<T>;

#[derive(Debug, Clone)]
pub struct KycDocumentInsert {
    pub external_id: String,
    pub document_type: String,
    pub file_name: String,
    pub mime_type: String,
    pub content_length: i64,
    pub s3_bucket: String,
    pub s3_key: String,
    pub presigned_expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ApprovalCreated {
    pub request_id: String,
    pub status: String,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct SmsQueued {
    pub hash: String,
    pub ttl_seconds: i32,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct SmsPendingInsert {
    pub realm: String,
    pub client_id: String,
    pub user_id: Option<String>,
    pub phone_number: String,
    pub otp_sha256: Vec<u8>,
    pub ttl_seconds: i32,
    pub max_attempts: i32,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct SmsPublishFailure {
    pub id: String,
    pub gave_up: bool,
    pub error: String,
    pub next_retry_at: Option<DateTime<Utc>>,
}
