use chrono::{DateTime, Utc};

pub type RepoResult<T> = backend_core::Result<T>;

#[derive(Debug, Clone)]
pub struct KycDocumentInsert {
    pub user_id: String,
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

pub trait KycRepo: Send + Sync {
    fn ensure_kyc_profile(&self, user_id: &str) -> impl Future<Output = RepoResult<()>> + Send;
    fn insert_kyc_document_intent(
        &self,
        input: KycDocumentInsert,
    ) -> impl Future<Output = RepoResult<backend_model::db::KycDocumentRow>> + Send;
    fn get_kyc_profile(
        &self,
        user_id: &str,
    ) -> impl Future<Output = RepoResult<Option<backend_model::db::KycSubmissionRow>>> + Send;
    fn list_kyc_documents(
        &self,
        user_id: String,
    ) -> impl Future<Output = RepoResult<Vec<backend_model::db::KycDocumentRow>>> + Send;
    fn get_kyc_document(
        &self,
        user_id: &str,
        document_id: &str,
    ) -> impl Future<Output = RepoResult<Option<backend_model::db::KycDocumentRow>>> + Send;
    fn get_kyc_tier(&self, user_id: &str) -> impl Future<Output = RepoResult<Option<i32>>> + Send;
    fn list_kyc_submissions(
        &self,
        status: Option<String>,
        search: Option<String>,
        limit: i64,
        offset: i64,
    ) -> impl Future<Output = RepoResult<Vec<backend_model::db::KycSubmissionRow>>> + Send;
    fn count_kyc_submissions(
        &self,
        status: Option<String>,
        search: Option<String>,
    ) -> impl Future<Output = RepoResult<i64>> + Send;
    fn get_kyc_submission(
        &self,
        user_id: &str,
    ) -> impl Future<Output = RepoResult<Option<backend_model::db::KycSubmissionRow>>> + Send;
    fn update_kyc_approved(
        &self,
        user_id: &str,
        req: &backend_model::staff::KycApprovalRequest,
    ) -> impl Future<Output = RepoResult<bool>> + Send;
    fn update_kyc_rejected(
        &self,
        user_id: &str,
        req: &backend_model::staff::KycRejectionRequest,
    ) -> impl Future<Output = RepoResult<bool>> + Send;
    fn update_kyc_request_info(
        &self,
        user_id: &str,
        req: &backend_model::staff::KycRequestInfoRequest,
    ) -> impl Future<Output = RepoResult<bool>> + Send;
    fn submit_kyc_profile(
        &self,
        submission_id: &str,
        user_id: &str,
    ) -> impl Future<Output = RepoResult<bool>> + Send;
    fn patch_kyc_profile(
        &self,
        user_id: &str,
        req: &backend_model::bff::KycInformationPatchRequest,
    ) -> impl Future<Output = RepoResult<Option<backend_model::db::KycSubmissionRow>>> + Send;
}

pub trait UserRepo: Send + Sync {
    fn create_user(
        &self,
        req: &backend_model::kc::UserUpsert,
    ) -> impl Future<Output = RepoResult<backend_model::db::UserRow>> + Send;
    fn get_user(
        &self,
        user_id: &str,
    ) -> impl Future<Output = RepoResult<Option<backend_model::db::UserRow>>> + Send;
    fn update_user(
        &self,
        user_id: &str,
        req: &backend_model::kc::UserUpsert,
    ) -> impl Future<Output = RepoResult<Option<backend_model::db::UserRow>>> + Send;
    fn delete_user(&self, user_id: &str) -> impl Future<Output = RepoResult<u64>> + Send;
    fn search_users(
        &self,
        req: &backend_model::kc::UserSearch,
    ) -> impl Future<Output = RepoResult<Vec<backend_model::db::UserRow>>> + Send;
    fn resolve_user_by_phone(
        &self,
        realm: &str,
        phone: &str,
    ) -> impl Future<Output = RepoResult<Option<backend_model::db::UserRow>>> + Send;
    fn resolve_or_create_user_by_phone(
        &self,
        realm: &str,
        phone: &str,
    ) -> impl Future<Output = RepoResult<(backend_model::db::UserRow, bool)>> + Send;
}

pub trait DeviceRepo: Send + Sync {
    fn lookup_device(
        &self,
        req: &backend_model::kc::DeviceLookupRequest,
    ) -> impl Future<Output = RepoResult<Option<backend_model::db::DeviceRow>>> + Send;
    fn list_user_devices(
        &self,
        user_id: &str,
        include_revoked: bool,
    ) -> impl Future<Output = RepoResult<Vec<backend_model::db::DeviceRow>>> + Send;
    fn get_user_device(
        &self,
        user_id: &str,
        device_id: &str,
    ) -> impl Future<Output = RepoResult<Option<backend_model::db::DeviceRow>>> + Send;
    fn update_device_status(
        &self,
        record_id: &str,
        status: &str,
    ) -> impl Future<Output = RepoResult<backend_model::db::DeviceRow>> + Send;
    fn find_device_binding(
        &self,
        device_id: &str,
        jkt: &str,
    ) -> impl Future<Output = RepoResult<Option<(String, String)>>> + Send;
    fn bind_device(
        &self,
        req: &backend_model::kc::EnrollmentBindRequest,
    ) -> impl Future<Output = RepoResult<String>> + Send;
    fn count_user_devices(&self, user_id: &str) -> impl Future<Output = RepoResult<i64>> + Send;
}

pub trait ApprovalRepo: Send + Sync {
    fn create_approval(
        &self,
        req: &backend_model::kc::ApprovalCreateRequest,
        idempotency_key: Option<String>,
    ) -> impl Future<Output = RepoResult<ApprovalCreated>> + Send;
    fn get_approval(
        &self,
        request_id: &str,
    ) -> impl Future<Output = RepoResult<Option<backend_model::db::ApprovalRow>>> + Send;
    fn list_user_approvals(
        &self,
        user_id: &str,
        statuses: Option<Vec<String>>,
    ) -> impl Future<Output = RepoResult<Vec<backend_model::db::ApprovalRow>>> + Send;
    fn decide_approval(
        &self,
        request_id: &str,
        req: &backend_model::kc::ApprovalDecisionRequest,
    ) -> impl Future<Output = RepoResult<Option<backend_model::db::ApprovalRow>>> + Send;
    fn cancel_approval(&self, request_id: &str) -> impl Future<Output = RepoResult<u64>> + Send;
}

pub trait SmsRepo: Send + Sync {
    fn queue_sms(
        &self,
        sms: SmsPendingInsert,
    ) -> impl Future<Output = RepoResult<SmsQueued>> + Send;
    fn get_sms_by_hash(
        &self,
        hash: &str,
    ) -> impl Future<Output = RepoResult<Option<backend_model::db::SmsMessageRow>>> + Send;
    fn mark_sms_confirmed(&self, hash: &str) -> impl Future<Output = RepoResult<()>> + Send;
    fn list_retryable_sms(
        &self,
        limit: i64,
    ) -> impl Future<Output = RepoResult<Vec<backend_model::db::SmsMessageRow>>> + Send;
    fn mark_sms_sent(
        &self,
        id: &str,
        sns_message_id: Option<String>,
    ) -> impl Future<Output = RepoResult<()>> + Send;
    fn mark_sms_failed(
        &self,
        update: SmsPublishFailure,
    ) -> impl Future<Output = RepoResult<()>> + Send;
    fn mark_sms_gave_up(
        &self,
        id: &str,
        reason: &str,
    ) -> impl Future<Output = RepoResult<()>> + Send;
}
