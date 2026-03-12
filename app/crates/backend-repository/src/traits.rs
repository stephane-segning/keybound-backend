//! Repository traits defining database operation contracts.
//!
//! These traits abstract the database layer, enabling:
//! - Clean separation between business logic and persistence
//! - Easier testing with mock implementations
//! - Type-safe queries through Diesel DSL

use chrono::{DateTime, Utc};
use serde_json::Value;

/// Result type alias for repository operations.
pub type RepoResult<T> = backend_core::Result<T>;

/// Pagination filter for list queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageFilter {
    pub page: i32,
    pub limit: i32,
}

impl PageFilter {
    /// Returns normalized filter with page >= 1 and limit between 1 and 100.
    pub fn normalized(self) -> Self {
        Self {
            page: self.page.max(1),
            limit: self.limit.clamp(1, 100),
        }
    }

    /// Calculates the offset for SQL LIMIT/OFFSET pagination.
    pub fn offset(&self) -> i64 {
        i64::from((self.page - 1) * self.limit)
    }
}

/// Filter criteria for state machine instance queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmInstanceFilter {
    pub kind: Option<String>,
    pub status: Option<String>,
    pub user_id: Option<String>,
    pub phone_number: Option<String>,
    pub created_from: Option<DateTime<Utc>>,
    pub created_to: Option<DateTime<Utc>>,
    pub page: i32,
    pub limit: i32,
}

impl SmInstanceFilter {
    /// Returns normalized filter with trimmed strings and valid pagination.
    pub fn normalized(self) -> Self {
        let page = self.page.max(1);
        let limit = self.limit.clamp(1, 100);
        let kind = self
            .kind
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty());
        let status = self
            .status
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty());
        let user_id = self
            .user_id
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty());
        let phone_number = self
            .phone_number
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty());

        Self {
            kind,
            status,
            user_id,
            phone_number,
            created_from: self.created_from,
            created_to: self.created_to,
            page,
            limit,
        }
    }

    /// Calculates the offset for SQL LIMIT/OFFSET pagination.
    pub fn offset(&self) -> i64 {
        i64::from((self.page - 1) * self.limit)
    }
}

/// Input for creating a new state machine instance.
#[derive(Debug, Clone)]
pub struct SmInstanceCreateInput {
    pub id: String,
    pub kind: String,
    pub user_id: Option<String>,
    pub idempotency_key: String,
    pub status: String,
    pub context: Value,
}

/// Input for creating a state machine event.
#[derive(Debug, Clone)]
pub struct SmEventCreateInput {
    pub id: String,
    pub instance_id: String,
    pub kind: String,
    pub actor_type: String,
    pub actor_id: Option<String>,
    pub payload: Value,
}

/// Input for creating a step attempt record.
#[derive(Debug, Clone)]
pub struct SmStepAttemptCreateInput {
    pub id: String,
    pub instance_id: String,
    pub step_name: String,
    pub attempt_no: i32,
    pub status: String,
    pub external_ref: Option<String>,
    pub input: Value,
    pub output: Option<Value>,
    pub error: Option<Value>,
    pub queued_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub next_retry_at: Option<DateTime<Utc>>,
}

/// Partial update for step attempt records.
#[derive(Debug, Clone)]
pub struct SmStepAttemptPatch {
    pub status: Option<String>,
    pub output: Option<Option<Value>>,
    pub error: Option<Option<Value>>,
    pub queued_at: Option<Option<DateTime<Utc>>>,
    pub started_at: Option<Option<DateTime<Utc>>>,
    pub finished_at: Option<Option<DateTime<Utc>>>,
    pub next_retry_at: Option<Option<DateTime<Utc>>>,
}

/// Repository trait for state machine persistence operations.
#[backend_core::async_trait]
pub trait StateMachineRepo: Send + Sync {
    /// Creates a new state machine instance.
    async fn create_instance(
        &self,
        input: SmInstanceCreateInput,
    ) -> RepoResult<backend_model::db::SmInstanceRow>;

    async fn get_instance(
        &self,
        instance_id: &str,
    ) -> RepoResult<Option<backend_model::db::SmInstanceRow>>;

    /// Finds an instance by its idempotency key for deduplication.
    async fn get_instance_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> RepoResult<Option<backend_model::db::SmInstanceRow>>;

    /// Lists instances with filtering and pagination.
    /// Returns (rows, total_count).
    async fn list_instances(
        &self,
        filter: SmInstanceFilter,
    ) -> RepoResult<(Vec<backend_model::db::SmInstanceRow>, i64)>;

    /// Updates instance status and optionally sets completed_at.
    async fn update_instance_status(
        &self,
        instance_id: &str,
        status: &str,
        completed_at: Option<DateTime<Utc>>,
    ) -> RepoResult<()>;

    /// Updates the context JSON of an instance.
    async fn update_instance_context(&self, instance_id: &str, context: Value) -> RepoResult<()>;

    /// Appends an event to the instance's event history.
    async fn append_event(
        &self,
        input: SmEventCreateInput,
    ) -> RepoResult<backend_model::db::SmEventRow>;

    /// Lists all events for an instance in chronological order.
    async fn list_events(
        &self,
        instance_id: &str,
    ) -> RepoResult<Vec<backend_model::db::SmEventRow>>;

    /// Creates a new step attempt record.
    async fn create_step_attempt(
        &self,
        input: SmStepAttemptCreateInput,
    ) -> RepoResult<backend_model::db::SmStepAttemptRow>;

    /// Patches specific fields of a step attempt.
    async fn patch_step_attempt(
        &self,
        attempt_id: &str,
        patch: SmStepAttemptPatch,
    ) -> RepoResult<backend_model::db::SmStepAttemptRow>;

    /// Atomically claim a queued attempt for execution.
    /// Returns None if the attempt was not in QUEUED state (already running/finished/cancelled).
    async fn claim_step_attempt(
        &self,
        attempt_id: &str,
    ) -> RepoResult<Option<backend_model::db::SmStepAttemptRow>>;

    /// Lists all step attempts for an instance.
    async fn list_step_attempts(
        &self,
        instance_id: &str,
    ) -> RepoResult<Vec<backend_model::db::SmStepAttemptRow>>;

    /// Gets the most recent attempt for a step within an instance.
    async fn get_latest_step_attempt(
        &self,
        instance_id: &str,
        step_name: &str,
    ) -> RepoResult<Option<backend_model::db::SmStepAttemptRow>>;

    /// Finds a step attempt by its external reference (e.g., SMS ID).
    async fn get_step_attempt_by_external_ref(
        &self,
        instance_id: &str,
        step_name: &str,
        external_ref: &str,
    ) -> RepoResult<Option<backend_model::db::SmStepAttemptRow>>;

    /// Cancels all other attempts for a step (used when one succeeds).
    async fn cancel_other_attempts_for_step(
        &self,
        instance_id: &str,
        step_name: &str,
        keep_attempt_id: &str,
    ) -> RepoResult<()>;

    /// Gets the next attempt number for a step (1-indexed).
    async fn next_attempt_no(&self, instance_id: &str, step_name: &str) -> RepoResult<i32>;

    /// Retrieves staff contact info for deposit approvals.
    /// Returns (full_name, username, email).
    async fn select_deposit_staff_contact(
        &self,
        user_id: &str,
    ) -> RepoResult<(String, String, String)>;
}

/// Repository trait for user account operations.
#[backend_core::async_trait]
pub trait UserRepo: Send + Sync {
    /// Creates a new user from Keycloak upsert request.
    async fn create_user(
        &self,
        req: &backend_model::kc::UserUpsert,
    ) -> RepoResult<backend_model::db::UserRow>;
    
    /// Gets a user by ID.
    async fn get_user(&self, user_id: &str) -> RepoResult<Option<backend_model::db::UserRow>>;
    
    /// Updates a user from Keycloak upsert request.
    async fn update_user(
        &self,
        user_id: &str,
        req: &backend_model::kc::UserUpsert,
    ) -> RepoResult<Option<backend_model::db::UserRow>>;
    
    /// Deletes a user by ID. Returns rows deleted (0 or 1).
    async fn delete_user(&self, user_id: &str) -> RepoResult<u64>;
    
    /// Searches users by various criteria.
    async fn search_users(
        &self,
        req: &backend_model::kc::UserSearch,
    ) -> RepoResult<Vec<backend_model::db::UserRow>>;
    
    /// Finds a user by phone number within a realm.
    async fn resolve_user_by_phone(
        &self,
        realm: &str,
        phone: &str,
    ) -> RepoResult<Option<backend_model::db::UserRow>>;
    
    /// Finds or creates a user by phone number.
    /// Returns (user, created) where created is true if new user was created.
    async fn resolve_or_create_user_by_phone(
        &self,
        realm: &str,
        phone: &str,
    ) -> RepoResult<(backend_model::db::UserRow, bool)>;
}

/// Repository trait for device binding operations.
#[backend_core::async_trait]
pub trait DeviceRepo: Send + Sync {
    /// Looks up a device by ID and/or JKT. Updates last_seen_at on match.
    async fn lookup_device(
        &self,
        req: &backend_model::kc::DeviceLookupRequest,
    ) -> RepoResult<Option<backend_model::db::DeviceRow>>;
    
    /// Lists all devices for a user, optionally including revoked ones.
    async fn list_user_devices(
        &self,
        user_id: &str,
        include_revoked: bool,
    ) -> RepoResult<Vec<backend_model::db::DeviceRow>>;
    
    /// Gets a specific device for a user.
    async fn get_user_device(
        &self,
        user_id: &str,
        device_id: &str,
    ) -> RepoResult<Option<backend_model::db::DeviceRow>>;
    
    /// Updates device status (active, revoked, etc.).
    async fn update_device_status(
        &self,
        record_id: &str,
        status: &str,
    ) -> RepoResult<backend_model::db::DeviceRow>;
    
    /// Finds an existing device binding by device_id and JKT.
    /// Returns (user_id, device_record_id) if found.
    async fn find_device_binding(
        &self,
        device_id: &str,
        jkt: &str,
    ) -> RepoResult<Option<(String, String)>>;
    
    /// Binds a device to a user with public key.
    async fn bind_device(
        &self,
        req: &backend_model::kc::EnrollmentBindRequest,
    ) -> RepoResult<String>;
    
    /// Counts the number of devices for a user.
    async fn count_user_devices(&self, user_id: &str) -> RepoResult<i64>;
}
