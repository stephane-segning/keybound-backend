use chrono::{DateTime, Utc};
use serde_json::Value;

pub type RepoResult<T> = backend_core::Result<T>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KycSubmissionFilter {
    pub status: Option<String>,
    pub search: Option<String>,
    pub page: i32,
    pub limit: i32,
}

impl KycSubmissionFilter {
    pub fn normalized(self) -> Self {
        let page = self.page.max(1);
        let limit = self.limit.clamp(1, 100);

        let status = self
            .status
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let search = self
            .search
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());

        Self {
            status,
            search,
            page,
            limit,
        }
    }

    pub fn offset(&self) -> i64 {
        i64::from((self.page - 1) * self.limit)
    }
}

#[derive(Debug, Clone)]
pub struct KycStepCreateInput {
    pub session_id: String,
    pub user_id: String,
    pub step_type: String,
    pub policy: Value,
}

#[derive(Debug, Clone)]
pub struct OtpChallengeCreateInput {
    pub step_id: String,
    pub msisdn: String,
    pub channel: String,
    pub otp_hash: String,
    pub expires_at: DateTime<Utc>,
    pub tries_left: i32,
}

#[derive(Debug, Clone)]
pub struct MagicChallengeCreateInput {
    pub step_id: String,
    pub email: String,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UploadIntentCreateInput {
    pub step_id: String,
    pub user_id: String,
    pub purpose: String,
    pub asset_type: String,
    pub mime: String,
    pub size_bytes: i64,
    pub bucket: String,
    pub object_key: String,
    pub method: String,
    pub url: String,
    pub headers: Value,
    pub multipart: Option<Value>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UploadCompleteInput {
    pub upload_id: String,
    pub user_id: String,
    pub bucket: String,
    pub object_key: String,
    pub etag: Option<String>,
    pub computed_sha256: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UploadCompleteResult {
    pub evidence: backend_model::db::KycEvidenceRow,
    pub moved_to_pending_review: bool,
}

#[derive(Debug, Clone)]
pub struct KycStaffSubmissionSummaryRow {
    pub submission_id: String,
    pub user_id: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub phone_number: Option<String>,
    pub status: String,
    pub submitted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct KycStaffSubmissionDetailRow {
    pub submission_id: String,
    pub user_id: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub phone_number: Option<String>,
    pub date_of_birth: Option<String>,
    pub nationality: Option<String>,
    pub status: String,
    pub submitted_at: Option<DateTime<Utc>>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub reviewed_by: Option<String>,
    pub rejection_reason: Option<String>,
    pub review_notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct KycStaffDocumentRow {
    pub id: String,
    pub submission_id: String,
    pub document_type: String,
    pub file_name: String,
    pub mime_type: String,
    pub bucket: String,
    pub object_key: String,
    pub uploaded_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct KycReviewEvidenceRow {
    pub asset_type: String,
    pub evidence_id: String,
}

#[derive(Debug, Clone)]
pub struct KycReviewCaseRow {
    pub case_id: String,
    pub user_id: String,
    pub step_id: String,
    pub status: String,
    pub submitted_at: DateTime<Utc>,
    pub first_name: String,
    pub middle_name: Option<String>,
    pub last_name: String,
    pub evidence: Vec<KycReviewEvidenceRow>,
}

#[derive(Debug, Clone)]
pub struct KycReviewDecisionRecord {
    pub case_id: String,
    pub decision: String,
    pub decided_at: DateTime<Utc>,
}

pub trait KycRepo: Send + Sync {
    fn start_or_resume_session(
        &self,
        user_id: &str,
    ) -> impl Future<Output = RepoResult<(backend_model::db::KycSessionRow, Vec<String>)>> + Send;

    fn create_step(
        &self,
        input: KycStepCreateInput,
    ) -> impl Future<Output = RepoResult<backend_model::db::KycStepRow>> + Send;

    fn get_step(
        &self,
        step_id: &str,
    ) -> impl Future<Output = RepoResult<Option<backend_model::db::KycStepRow>>> + Send;

    fn count_recent_otp_challenges(
        &self,
        step_id: &str,
        since: DateTime<Utc>,
    ) -> impl Future<Output = RepoResult<i64>> + Send;

    fn create_otp_challenge(
        &self,
        input: OtpChallengeCreateInput,
    ) -> impl Future<Output = RepoResult<backend_model::db::KycOtpChallengeRow>> + Send;

    fn get_otp_challenge(
        &self,
        step_id: &str,
        otp_ref: &str,
    ) -> impl Future<Output = RepoResult<Option<backend_model::db::KycOtpChallengeRow>>> + Send;

    fn mark_otp_verified(
        &self,
        step_id: &str,
        otp_ref: &str,
    ) -> impl Future<Output = RepoResult<()>> + Send;

    fn decrement_otp_tries(
        &self,
        step_id: &str,
        otp_ref: &str,
    ) -> impl Future<Output = RepoResult<i32>> + Send;

    fn count_recent_magic_challenges(
        &self,
        step_id: &str,
        since: DateTime<Utc>,
    ) -> impl Future<Output = RepoResult<i64>> + Send;

    fn create_magic_challenge(
        &self,
        input: MagicChallengeCreateInput,
    ) -> impl Future<Output = RepoResult<backend_model::db::KycMagicEmailChallengeRow>> + Send;

    fn get_magic_challenge(
        &self,
        token_ref: &str,
    ) -> impl Future<Output = RepoResult<Option<backend_model::db::KycMagicEmailChallengeRow>>> + Send;

    fn mark_magic_verified(&self, token_ref: &str) -> impl Future<Output = RepoResult<()>> + Send;

    fn update_step_status(
        &self,
        step_id: &str,
        status: &str,
    ) -> impl Future<Output = RepoResult<()>> + Send;

    fn create_upload_intent(
        &self,
        input: UploadIntentCreateInput,
    ) -> impl Future<Output = RepoResult<backend_model::db::KycUploadRow>> + Send;

    fn complete_upload_and_register_evidence(
        &self,
        input: UploadCompleteInput,
    ) -> impl Future<Output = RepoResult<UploadCompleteResult>> + Send;

    fn list_staff_submissions(
        &self,
        filter: KycSubmissionFilter,
    ) -> impl Future<Output = RepoResult<(Vec<KycStaffSubmissionSummaryRow>, i64)>> + Send;

    fn get_staff_submission(
        &self,
        submission_id: &str,
    ) -> impl Future<Output = RepoResult<Option<KycStaffSubmissionDetailRow>>> + Send;

    fn list_staff_submission_documents(
        &self,
        submission_id: &str,
    ) -> impl Future<Output = RepoResult<Vec<KycStaffDocumentRow>>> + Send;

    fn get_staff_submission_document(
        &self,
        submission_id: &str,
        document_id: &str,
    ) -> impl Future<Output = RepoResult<Option<KycStaffDocumentRow>>> + Send;

    fn approve_submission(
        &self,
        submission_id: &str,
        reviewer_id: Option<&str>,
        notes: Option<&str>,
    ) -> impl Future<Output = RepoResult<bool>> + Send;

    fn reject_submission(
        &self,
        submission_id: &str,
        reviewer_id: Option<&str>,
        reason: &str,
        notes: Option<&str>,
    ) -> impl Future<Output = RepoResult<bool>> + Send;

    fn request_submission_info(
        &self,
        submission_id: &str,
        message: &str,
    ) -> impl Future<Output = RepoResult<bool>> + Send;

    fn list_review_cases(
        &self,
        page: i32,
        limit: i32,
    ) -> impl Future<Output = RepoResult<(Vec<KycReviewCaseRow>, i64)>> + Send;

    fn get_review_case(
        &self,
        case_id: &str,
    ) -> impl Future<Output = RepoResult<Option<KycReviewCaseRow>>> + Send;

    fn decide_review_case(
        &self,
        case_id: &str,
        outcome: &str,
        reason_code: &str,
        comment: Option<&str>,
        reviewer_id: Option<&str>,
    ) -> impl Future<Output = RepoResult<Option<KycReviewDecisionRecord>>> + Send;
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
