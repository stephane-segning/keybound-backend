use backend_model::db;
use backend_model::{kc as kc_map, staff as staff_map};
use chrono::{DateTime, Utc};
pub type RepoResult<T> = backend_core::Result<T>;

pub trait InsertRepo<T, R> {
    fn insert(&self, entity: T) -> impl std::future::Future<Output = RepoResult<R>> + Send;
}

pub trait UpdateRepo<T, R> {
    fn update(&self, entity: T) -> impl std::future::Future<Output = RepoResult<R>> + Send;
}

pub trait DeleteRepo<K> {
    fn delete(&self, key: K) -> impl std::future::Future<Output = RepoResult<u64>> + Send;
}

pub trait FindRepo<K, R> {
    fn find(&self, key: K) -> impl std::future::Future<Output = RepoResult<Option<R>>> + Send;
}

pub trait ListRepo<Q, R> {
    fn list(&self, query: Q) -> impl std::future::Future<Output = RepoResult<R>> + Send;
}

pub trait AuditRepo<E> {
    fn audit(&self, event: E) -> impl std::future::Future<Output = RepoResult<()>> + Send;
}

#[derive(Debug, Clone)]
pub struct KycSubmissionsQuery {
    pub status: Option<String>,
    pub search: Option<String>,
    pub page: i32,
    pub limit: i32,
}

#[derive(Debug, Clone)]
pub struct KycSubmissionsPage {
    pub total: i32,
    pub items: Vec<db::KycProfileRow>,
}

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

pub trait BffRepo: Send + Sync {
    fn ensure_kyc_profile(
        &self,
        external_id: &str,
    ) -> impl std::future::Future<Output = RepoResult<()>> + Send;

    fn insert_kyc_document_intent(
        &self,
        input: KycDocumentInsert,
    ) -> impl std::future::Future<Output = RepoResult<db::KycDocumentRow>> + Send;

    fn get_kyc_profile(
        &self,
        external_id: &str,
    ) -> impl std::future::Future<Output = RepoResult<Option<db::KycProfileRow>>> + Send;

    fn list_kyc_documents(
        &self,
        external_id: &str,
    ) -> impl std::future::Future<Output = RepoResult<Vec<db::KycDocumentRow>>> + Send;

    fn get_kyc_tier(
        &self,
        external_id: &str,
    ) -> impl std::future::Future<Output = RepoResult<Option<i32>>> + Send;
}

pub trait StaffRepo: Send + Sync {
    fn list_kyc_submissions(
        &self,
        query: KycSubmissionsQuery,
    ) -> impl std::future::Future<Output = RepoResult<KycSubmissionsPage>> + Send;

    fn get_kyc_submission(
        &self,
        external_id: &str,
    ) -> impl std::future::Future<Output = RepoResult<Option<db::KycProfileRow>>> + Send;

    fn update_kyc_approved(
        &self,
        external_id: &str,
        req: &staff_map::KycApprovalRequest,
    ) -> impl std::future::Future<Output = RepoResult<bool>> + Send;

    fn update_kyc_rejected(
        &self,
        external_id: &str,
        req: &staff_map::KycRejectionRequest,
    ) -> impl std::future::Future<Output = RepoResult<bool>> + Send;

    fn update_kyc_request_info(
        &self,
        external_id: &str,
        req: &staff_map::KycRequestInfoRequest,
    ) -> impl std::future::Future<Output = RepoResult<bool>> + Send;
}

pub trait KcRepo: Send + Sync {
    fn create_user(
        &self,
        req: &kc_map::UserUpsert,
    ) -> impl std::future::Future<Output = RepoResult<db::UserRow>> + Send;

    fn get_user(
        &self,
        user_id: &str,
    ) -> impl std::future::Future<Output = RepoResult<Option<db::UserRow>>> + Send;

    fn update_user(
        &self,
        user_id: &str,
        req: &kc_map::UserUpsert,
    ) -> impl std::future::Future<Output = RepoResult<Option<db::UserRow>>> + Send;

    fn delete_user(
        &self,
        user_id: &str,
    ) -> impl std::future::Future<Output = RepoResult<u64>> + Send;

    fn search_users(
        &self,
        req: &kc_map::UserSearch,
    ) -> impl std::future::Future<Output = RepoResult<Vec<db::UserRow>>> + Send;

    fn lookup_device(
        &self,
        req: &kc_map::DeviceLookupRequest,
    ) -> impl std::future::Future<Output = RepoResult<Option<db::DeviceRow>>> + Send;

    fn list_user_devices(
        &self,
        user_id: &str,
        include_revoked: bool,
    ) -> impl std::future::Future<Output = RepoResult<Vec<db::DeviceRow>>> + Send;

    fn get_user_device(
        &self,
        user_id: &str,
        device_id: &str,
    ) -> impl std::future::Future<Output = RepoResult<Option<db::DeviceRow>>> + Send;

    fn update_device_status(
        &self,
        record_id: &str,
        status: &str,
    ) -> impl std::future::Future<Output = RepoResult<db::DeviceRow>> + Send;

    fn find_device_binding(
        &self,
        device_id: &str,
        jkt: &str,
    ) -> impl std::future::Future<Output = RepoResult<Option<(String, String)>>> + Send;

    fn bind_device(
        &self,
        req: &kc_map::EnrollmentBindRequest,
    ) -> impl std::future::Future<Output = RepoResult<String>> + Send;

    fn create_approval(
        &self,
        req: &kc_map::ApprovalCreateRequest,
        idempotency_key: Option<String>,
    ) -> impl std::future::Future<Output = RepoResult<ApprovalCreated>> + Send;

    fn get_approval(
        &self,
        request_id: &str,
    ) -> impl std::future::Future<Output = RepoResult<Option<db::ApprovalRow>>> + Send;

    fn list_user_approvals(
        &self,
        user_id: &str,
        statuses: Option<Vec<String>>,
    ) -> impl std::future::Future<Output = RepoResult<Vec<db::ApprovalRow>>> + Send;

    fn decide_approval(
        &self,
        request_id: &str,
        req: &kc_map::ApprovalDecisionRequest,
    ) -> impl std::future::Future<Output = RepoResult<Option<db::ApprovalRow>>> + Send;

    fn cancel_approval(
        &self,
        request_id: &str,
    ) -> impl std::future::Future<Output = RepoResult<u64>> + Send;

    fn resolve_user_by_phone(
        &self,
        realm: &str,
        phone: &str,
    ) -> impl std::future::Future<Output = RepoResult<Option<db::UserRow>>> + Send;

    fn resolve_or_create_user_by_phone(
        &self,
        realm: &str,
        phone: &str,
    ) -> impl std::future::Future<Output = RepoResult<(db::UserRow, bool)>> + Send;

    fn count_user_devices(
        &self,
        user_id: &str,
    ) -> impl std::future::Future<Output = RepoResult<i64>> + Send;

    fn queue_sms(
        &self,
        sms: SmsPendingInsert,
    ) -> impl std::future::Future<Output = RepoResult<SmsQueued>> + Send;

    fn get_sms_by_hash(
        &self,
        hash: &str,
    ) -> impl std::future::Future<Output = RepoResult<Option<db::SmsMessageRow>>> + Send;

    fn mark_sms_confirmed(
        &self,
        hash: &str,
    ) -> impl std::future::Future<Output = RepoResult<()>> + Send;
}

pub trait SmsRetryRepo: Send + Sync {
    fn list_retryable_sms(
        &self,
        limit: i64,
    ) -> impl std::future::Future<Output = RepoResult<Vec<db::SmsMessageRow>>> + Send;

    fn mark_sms_sent(
        &self,
        id: &str,
        sns_message_id: Option<String>,
    ) -> impl std::future::Future<Output = RepoResult<()>> + Send;

    fn mark_sms_failed(
        &self,
        update: SmsPublishFailure,
    ) -> impl std::future::Future<Output = RepoResult<()>> + Send;

    fn mark_sms_gave_up(
        &self,
        id: &str,
        reason: &str,
    ) -> impl std::future::Future<Output = RepoResult<()>> + Send;
}
