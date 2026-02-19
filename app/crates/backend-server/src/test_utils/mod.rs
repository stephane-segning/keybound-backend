use crate::file_storage::{EncryptionMode, FileStorage, PresignedUpload};
use crate::sms_provider::SmsProvider;
use crate::state::AppState;
use crate::worker::{NotificationJob, NotificationQueue, ProvisioningQueue, WorkerHttpClient};
use backend_auth::{OidcState, SignatureState};
use backend_core::async_trait;
use backend_core::{Config, Error, Result};
use backend_repository::{
    DeviceRepo, KycRepo, KycReviewCaseRow, KycReviewDecisionRecord, KycStaffDocumentRow,
    KycStaffSubmissionDetailRow, KycStaffSubmissionSummaryRow, KycStepCreateInput,
    MagicChallengeCreateInput, OtpChallengeCreateInput, RepoResult, UploadCompleteInput,
    UploadCompleteResult, UploadIntentCreateInput, UserRepo,
};
use mockall::mock;
use std::sync::Arc;
use std::time::Duration;

mock! {
    pub WorkerHttpClient {}
    #[async_trait]
    impl WorkerHttpClient for WorkerHttpClient {
        async fn post_json(
            &self,
            url: &str,
            body: &serde_json::Value,
        ) -> std::result::Result<(http::StatusCode, String), apalis::prelude::BoxDynError>;
    }
    impl std::fmt::Debug for WorkerHttpClient {
        fn fmt<'a>(&self, f: &mut std::fmt::Formatter<'a>) -> std::fmt::Result;
    }
}

mock! {
    pub NotificationQueue {}
    #[async_trait]
    impl NotificationQueue for NotificationQueue {
        async fn enqueue(&self, job: NotificationJob) -> backend_core::Result<()>;
    }
}

mock! {
    pub ProvisioningQueue {}
    #[async_trait]
    impl ProvisioningQueue for ProvisioningQueue {
        async fn enqueue_fineract_provisioning(&self, user_id: &str) -> backend_core::Result<()>;
    }
}

mock! {
    pub FileStorage {}
    #[async_trait]
    impl FileStorage for FileStorage {
        async fn head_object(&self, bucket: &str, key: &str) -> std::result::Result<(), Error>;

        async fn presign_get_object(
            &self,
            bucket: &str,
            key: &str,
            expires_in: Duration,
            content_disposition: Option<String>,
        ) -> std::result::Result<String, Error>;

        async fn presign_put_object(
            &self,
            bucket: &str,
            key: &str,
            mime_type: &str,
            encryption: EncryptionMode,
            expires_in: Duration,
        ) -> std::result::Result<PresignedUpload, Error>;
    }
}

mock! {
    pub KycRepo {}
    #[async_trait]
    impl KycRepo for KycRepo {
        async fn start_or_resume_session(
            &self,
            user_id: &str,
        ) -> RepoResult<(backend_model::db::KycSessionRow, Vec<String>)>;

        async fn create_step(
            &self,
            input: KycStepCreateInput,
        ) -> RepoResult<backend_model::db::KycStepRow>;

        async fn get_step(
            &self,
            step_id: &str,
        ) -> RepoResult<Option<backend_model::db::KycStepRow>>;

        async fn count_recent_otp_challenges(
            &self,
            step_id: &str,
            since: chrono::DateTime<chrono::Utc>,
        ) -> RepoResult<i64>;

        async fn create_otp_challenge(
            &self,
            input: OtpChallengeCreateInput,
        ) -> RepoResult<backend_model::db::KycOtpChallengeRow>;

        async fn get_otp_challenge(
            &self,
            step_id: &str,
            otp_ref: &str,
        ) -> RepoResult<Option<backend_model::db::KycOtpChallengeRow>>;

        async fn mark_otp_verified(
            &self,
            step_id: &str,
            otp_ref: &str,
        ) -> RepoResult<()>;

        async fn decrement_otp_tries(
            &self,
            step_id: &str,
            otp_ref: &str,
        ) -> RepoResult<i32>;

        async fn count_recent_magic_challenges(
            &self,
            step_id: &str,
            since: chrono::DateTime<chrono::Utc>,
        ) -> RepoResult<i64>;

        async fn create_magic_challenge(
            &self,
            input: MagicChallengeCreateInput,
        ) -> RepoResult<backend_model::db::KycMagicEmailChallengeRow>;

        async fn get_magic_challenge(
            &self,
            token_ref: &str,
        ) -> RepoResult<Option<backend_model::db::KycMagicEmailChallengeRow>>;

        async fn mark_magic_verified(&self, token_ref: &str) -> RepoResult<()>;

        async fn update_step_status(
            &self,
            step_id: &str,
            status: &str,
        ) -> RepoResult<()>;

        async fn create_upload_intent(
            &self,
            input: UploadIntentCreateInput,
        ) -> RepoResult<backend_model::db::KycUploadRow>;

        async fn complete_upload_and_register_evidence(
            &self,
            input: UploadCompleteInput,
        ) -> RepoResult<UploadCompleteResult>;

        async fn list_staff_submissions(
            &self,
            filter: backend_repository::KycSubmissionFilter,
        ) -> RepoResult<(Vec<KycStaffSubmissionSummaryRow>, i64)>;

        async fn get_staff_submission(
            &self,
            submission_id: &str,
        ) -> RepoResult<Option<KycStaffSubmissionDetailRow>>;

        async fn list_staff_submission_documents(
            &self,
            submission_id: &str,
        ) -> RepoResult<Vec<KycStaffDocumentRow>>;

        async fn get_staff_submission_document(
            &self,
            submission_id: &str,
            document_id: &str,
        ) -> RepoResult<Option<KycStaffDocumentRow>>;

        async fn approve_submission(
            &self,
            submission_id: String,
            reviewer_id: Option<String>,
            notes: Option<String>,
        ) -> RepoResult<bool>;

        async fn reject_submission(
            &self,
            submission_id: String,
            reviewer_id: Option<String>,
            reason: String,
            notes: Option<String>,
        ) -> RepoResult<bool>;

        async fn request_submission_info(
            &self,
            submission_id: &str,
            message: &str,
        ) -> RepoResult<bool>;

        async fn list_review_cases(
            &self,
            page: i32,
            limit: i32,
        ) -> RepoResult<(Vec<KycReviewCaseRow>, i64)>;

        async fn get_review_case(
            &self,
            case_id: &str,
        ) -> RepoResult<Option<KycReviewCaseRow>>;

        async fn decide_review_case(
            &self,
            case_id: String,
            outcome: String,
            reason_code: String,
            comment: Option<String>,
            reviewer_id: Option<String>,
        ) -> RepoResult<Option<KycReviewDecisionRecord>>;
    }
}

mock! {
    pub UserRepo {}
    #[async_trait]
    impl UserRepo for UserRepo {
        async fn create_user(
            &self,
            req: &backend_model::kc::UserUpsert,
        ) -> RepoResult<backend_model::db::UserRow>;
        async fn get_user(
            &self,
            user_id: &str,
        ) -> RepoResult<Option<backend_model::db::UserRow>>;
        async fn update_user(
            &self,
            user_id: &str,
            req: &backend_model::kc::UserUpsert,
        ) -> RepoResult<Option<backend_model::db::UserRow>>;
        async fn delete_user(&self, user_id: &str) -> RepoResult<u64>;
        async fn search_users(
            &self,
            req: &backend_model::kc::UserSearch,
        ) -> RepoResult<Vec<backend_model::db::UserRow>>;
        async fn resolve_user_by_phone(
            &self,
            realm: &str,
            phone: &str,
        ) -> RepoResult<Option<backend_model::db::UserRow>>;
        async fn resolve_or_create_user_by_phone(
            &self,
            realm: &str,
            phone: &str,
        ) -> RepoResult<(backend_model::db::UserRow, bool)>;
    }
}

mock! {
    pub DeviceRepo {}
    #[async_trait]
    impl DeviceRepo for DeviceRepo {
        async fn lookup_device(
            &self,
            req: &backend_model::kc::DeviceLookupRequest,
        ) -> RepoResult<Option<backend_model::db::DeviceRow>>;
        async fn list_user_devices(
            &self,
            user_id: &str,
            include_revoked: bool,
        ) -> RepoResult<Vec<backend_model::db::DeviceRow>>;
        async fn get_user_device(
            &self,
            user_id: &str,
            device_id: &str,
        ) -> RepoResult<Option<backend_model::db::DeviceRow>>;
        async fn update_device_status(
            &self,
            record_id: &str,
            status: &str,
        ) -> RepoResult<backend_model::db::DeviceRow>;
        async fn find_device_binding(
            &self,
            device_id: &str,
            jkt: &str,
        ) -> RepoResult<Option<(String, String)>>;
        async fn bind_device(
            &self,
            req: &backend_model::kc::EnrollmentBindRequest,
        ) -> RepoResult<String>;
        async fn count_user_devices(&self, user_id: &str) -> RepoResult<i64>;
    }
}

mock! {
    pub SmsProvider {}
    #[async_trait]
    impl SmsProvider for SmsProvider {
        async fn send_otp(&self, phone: &str, otp: &str) -> Result<()>;
    }
}

pub struct TestAppStateBuilder {
    pub kyc: Option<Arc<dyn KycRepo>>,
    pub user: Option<Arc<dyn UserRepo>>,
    pub device: Option<Arc<dyn DeviceRepo>>,
    pub sms: Option<Arc<dyn SmsProvider>>,
    pub notification_queue: Option<Arc<dyn NotificationQueue>>,
    pub provisioning_queue: Option<Arc<dyn ProvisioningQueue>>,
    pub worker_http_client: Option<Arc<dyn WorkerHttpClient>>,
    pub s3: Option<Arc<dyn FileStorage>>,
    pub config: Option<Config>,
}

impl Default for TestAppStateBuilder {
    fn default() -> Self {
        Self {
            kyc: None,
            user: None,
            device: None,
            sms: None,
            notification_queue: None,
            provisioning_queue: None,
            worker_http_client: None,
            s3: None,
            config: None,
        }
    }
}

impl TestAppStateBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_kyc(mut self, kyc: Arc<dyn KycRepo>) -> Self {
        self.kyc = Some(kyc);
        self
    }

    pub fn with_user(mut self, user: Arc<dyn UserRepo>) -> Self {
        self.user = Some(user);
        self
    }

    pub fn with_device(mut self, device: Arc<dyn DeviceRepo>) -> Self {
        self.device = Some(device);
        self
    }

    pub fn with_sms(mut self, sms: Arc<dyn SmsProvider>) -> Self {
        self.sms = Some(sms);
        self
    }

    pub fn with_notification_queue(mut self, queue: Arc<dyn NotificationQueue>) -> Self {
        self.notification_queue = Some(queue);
        self
    }

    pub fn with_provisioning_queue(mut self, queue: Arc<dyn ProvisioningQueue>) -> Self {
        self.provisioning_queue = Some(queue);
        self
    }

    pub fn with_worker_http_client(mut self, client: Arc<dyn WorkerHttpClient>) -> Self {
        self.worker_http_client = Some(client);
        self
    }

    pub fn with_s3(mut self, s3: Arc<dyn FileStorage>) -> Self {
        self.s3 = Some(s3);
        self
    }

    pub fn with_config(mut self, config: Config) -> Self {
        self.config = Some(config);
        self
    }

    pub async fn build(self) -> AppState {
        let config = self.config.unwrap_or_else(|| {
            // Minimal config for testing
            serde_yaml::from_str(
                r#"
server:
  address: "127.0.0.1"
  port: 8080
  tls:
    cert_path: "cert.pem"
    key_path: "key.pem"
logging:
  level: "info"
database:
  url: "postgres://localhost/test"
oauth2:
  issuer: "http://localhost:8081/realms/test"
kc:
  enabled: true
  base_path: "/kc"
  signature_secret: "test-secret"
  max_clock_skew_seconds: 30
  max_body_bytes: 1048576
bff:
  enabled: true
  base_path: "/bff"
staff:
  enabled: true
  base_path: "/staff"
cuss:
  api_url: "http://localhost:8082"
"#,
            )
            .unwrap()
        });

        let oidc_state = Arc::new(OidcState::new(
            config.oauth2.issuer.clone(),
            None,
            Duration::from_secs(3600),
            Duration::from_secs(3600),
            backend_auth::HttpClient::new_with_defaults().unwrap(),
        ));

        let signature_state = Arc::new(SignatureState {
            signature_secret: config.kc.signature_secret.clone(),
            max_clock_skew_seconds: config.kc.max_clock_skew_seconds,
            max_body_bytes: config.kc.max_body_bytes,
        });

        AppState {
            kyc: self.kyc.unwrap_or_else(|| Arc::new(MockKycRepo::new())),
            user: self.user.unwrap_or_else(|| Arc::new(MockUserRepo::new())),
            device: self.device.unwrap_or_else(|| Arc::new(MockDeviceRepo::new())),
            sms: self.sms.unwrap_or_else(|| Arc::new(MockSmsProvider::new())),
            notification_queue: self
                .notification_queue
                .unwrap_or_else(|| Arc::new(MockNotificationQueue::new())),
            provisioning_queue: self
                .provisioning_queue
                .unwrap_or_else(|| Arc::new(MockProvisioningQueue::new())),
            worker_http_client: self
                .worker_http_client
                .unwrap_or_else(|| Arc::new(reqwest::Client::new())),
            s3: self.s3.unwrap_or_else(|| Arc::new(MockFileStorage::new())),
            config,
            oidc_state,
            signature_state,
        }
    }
}

pub fn create_fake_jwt(user_id: &str) -> backend_auth::JwtToken {
    backend_auth::JwtToken::new(backend_auth::Claims {
        sub: user_id.to_string(),
        name: Some("Test User".to_string()),
        iss: "http://localhost:8081/realms/test".to_string(),
        exp: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp() as usize,
        preferred_username: Some("testuser".to_string()),
    })
}
