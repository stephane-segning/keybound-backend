use crate::file_storage::{EncryptionMode, FileStorage, PresignedUpload};
use crate::state::AppState;
use crate::state_machine::jobs::StateMachineStepJob;
use crate::state_machine::queue::StateMachineQueue;
use crate::worker::{NotificationJob, NotificationQueue, ProvisioningQueue, WorkerHttpClient};
use backend_auth::{OidcState, SignatureState};
use backend_core::async_trait;
use backend_core::{Config, Error};
use backend_repository::{
    DeviceRepo, RepoResult, SmEventCreateInput, SmInstanceCreateInput, SmInstanceFilter,
    SmStepAttemptCreateInput, SmStepAttemptPatch, StateMachineRepo, UserRepo,
};
use bytes::Bytes;
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
        async fn enqueue_fineract_provisioning(&self, _user_id: &str) -> backend_core::Result<()>;
    }
}

mock! {
    pub StateMachineQueue {}
    #[async_trait]
    impl StateMachineQueue for StateMachineQueue {
        async fn enqueue(&self, job: StateMachineStepJob) -> backend_core::Result<()>;
    }
}

mock! {
    pub FileStorage {}
    #[async_trait]
    impl FileStorage for FileStorage {
        async fn head_object(&self, bucket: &str, key: &str) -> std::result::Result<(), Error>;

        async fn upload(
            &self,
            bucket: &str,
            key: &str,
            mime_type: &str,
            encryption: EncryptionMode,
            body: Bytes,
        ) -> std::result::Result<(), Error>;

        async fn upload_presigned(
            &self,
            bucket: &str,
            key: &str,
            mime_type: &str,
            encryption: EncryptionMode,
            expires_in: Duration,
        ) -> std::result::Result<PresignedUpload, Error>;

        async fn download(&self, bucket: &str, key: &str) -> std::result::Result<Bytes, Error>;

        async fn download_presigned(
            &self,
            bucket: &str,
            key: &str,
            expires_in: Duration,
            content_disposition: Option<String>,
        ) -> std::result::Result<String, Error>;
    }
}

mock! {
    pub StateMachineRepo {}
    #[async_trait]
    impl StateMachineRepo for StateMachineRepo {
        async fn create_instance(
            &self,
            input: SmInstanceCreateInput,
        ) -> RepoResult<backend_model::db::SmInstanceRow>;

        async fn get_instance(
            &self,
            instance_id: &str,
        ) -> RepoResult<Option<backend_model::db::SmInstanceRow>>;

        async fn get_instance_by_idempotency_key(
            &self,
            idempotency_key: &str,
        ) -> RepoResult<Option<backend_model::db::SmInstanceRow>>;

        async fn list_instances(
            &self,
            filter: SmInstanceFilter,
        ) -> RepoResult<(Vec<backend_model::db::SmInstanceRow>, i64)>;

        async fn update_instance_status(
            &self,
            instance_id: &str,
            status: &str,
            completed_at: Option<chrono::DateTime<chrono::Utc>>,
        ) -> RepoResult<()>;

        async fn update_instance_context(
            &self,
            instance_id: &str,
            context: serde_json::Value,
        ) -> RepoResult<()>;

        async fn append_event(
            &self,
            input: SmEventCreateInput,
        ) -> RepoResult<backend_model::db::SmEventRow>;

        async fn list_events(
            &self,
            instance_id: &str,
        ) -> RepoResult<Vec<backend_model::db::SmEventRow>>;

        async fn create_step_attempt(
            &self,
            input: SmStepAttemptCreateInput,
        ) -> RepoResult<backend_model::db::SmStepAttemptRow>;

        async fn patch_step_attempt(
            &self,
            attempt_id: &str,
            patch: SmStepAttemptPatch,
        ) -> RepoResult<backend_model::db::SmStepAttemptRow>;

        async fn claim_step_attempt(
            &self,
            attempt_id: &str,
        ) -> RepoResult<Option<backend_model::db::SmStepAttemptRow>>;

        async fn list_step_attempts(
            &self,
            instance_id: &str,
        ) -> RepoResult<Vec<backend_model::db::SmStepAttemptRow>>;

        async fn get_latest_step_attempt(
            &self,
            instance_id: &str,
            step_name: &str,
        ) -> RepoResult<Option<backend_model::db::SmStepAttemptRow>>;

        async fn get_step_attempt_by_external_ref(
            &self,
            instance_id: &str,
            step_name: &str,
            external_ref: &str,
        ) -> RepoResult<Option<backend_model::db::SmStepAttemptRow>>;

        async fn cancel_other_attempts_for_step(
            &self,
            instance_id: &str,
            step_name: &str,
            keep_attempt_id: &str,
        ) -> RepoResult<()>;

        async fn next_attempt_no(
            &self,
            instance_id: &str,
            step_name: &str,
        ) -> RepoResult<i32>;

        async fn select_deposit_staff_contact(
            &self,
            user_id: &str,
        ) -> RepoResult<(String, String, String)>;
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

#[derive(Default)]
pub struct TestAppStateBuilder {
    pub sm: Option<Arc<dyn StateMachineRepo>>,
    pub user: Option<Arc<dyn UserRepo>>,
    pub device: Option<Arc<dyn DeviceRepo>>,
    pub sm_queue: Option<Arc<dyn StateMachineQueue>>,
    pub notification_queue: Option<Arc<dyn NotificationQueue>>,
    pub provisioning_queue: Option<Arc<dyn ProvisioningQueue>>,
    pub worker_http_client: Option<Arc<dyn WorkerHttpClient>>,
    pub s3: Option<Arc<dyn FileStorage>>,
    pub config: Option<Config>,
}

impl TestAppStateBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_sm(mut self, sm: Arc<dyn StateMachineRepo>) -> Self {
        self.sm = Some(sm);
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

    pub fn with_sm_queue(mut self, q: Arc<dyn StateMachineQueue>) -> Self {
        self.sm_queue = Some(q);
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

    pub fn build(self) -> AppState {
        let config = self.config.unwrap_or_else(|| {
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
            sm: self
                .sm
                .unwrap_or_else(|| Arc::new(MockStateMachineRepo::new())),
            user: self.user.unwrap_or_else(|| Arc::new(MockUserRepo::new())),
            device: self
                .device
                .unwrap_or_else(|| Arc::new(MockDeviceRepo::new())),
            sm_queue: self
                .sm_queue
                .unwrap_or_else(|| Arc::new(MockStateMachineQueue::new())),
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
    let claims = backend_auth::Claims {
        sub: user_id.to_owned(),
        name: None,
        iss: "http://localhost/test".to_owned(),
        exp: usize::MAX,
        preferred_username: None,
    };
    backend_auth::JwtToken::new(claims)
}
