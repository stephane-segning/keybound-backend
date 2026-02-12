use crate::traits::*;
use backend_model::db;
use backend_model::{kc as kc_map, staff as staff_map};
use chrono::{DateTime, Utc};
use lru::LruCache;
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx_data::{IntoParams, QueryResult, Serial, dml, repo};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct PgRepository {
    pool: PgPool,
    resolve_user_by_phone_cache: Arc<Mutex<LruCache<String, Option<db::UserRow>>>>,
}

impl PgRepository {
    pub fn new(pool: PgPool) -> Self {
        let capacity = NonZeroUsize::new(50_000).expect("non-zero LRU capacity");
        let resolve_user_by_phone_cache = Arc::new(Mutex::new(LruCache::new(capacity)));

        Self {
            pool,
            resolve_user_by_phone_cache,
        }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    fn phone_cache_key(realm: &str, phone: &str) -> String {
        format!("{realm}:{phone}")
    }
}

#[repo]
trait PgSqlRepo {
    #[dml(file = "queries/bff/list_kyc_documents.sql", unchecked)]
    async fn list_kyc_documents_db(
        &self,
        external_id: String,
        params: impl IntoParams,
    ) -> sqlx_data::Result<Serial<db::KycDocumentRow>>;

    #[dml(file = "queries/staff/list_kyc_submissions.sql", unchecked)]
    async fn list_kyc_submissions_db(
        &self,
        params: impl IntoParams,
    ) -> sqlx_data::Result<Serial<db::KycProfileRow>>;

    #[dml(
        "INSERT INTO kyc_profiles (external_id) VALUES ($1) ON CONFLICT (external_id) DO NOTHING",
        unchecked
    )]
    async fn ensure_kyc_profile_db(&self, external_id: String) -> sqlx_data::Result<QueryResult>;

    #[dml(
        r#"
        INSERT INTO kyc_documents (
          id, external_id, document_type, file_name, mime_type, content_length,
          s3_bucket, s3_key, presigned_expires_at
        )
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        RETURNING
          id,
          external_id,
          document_type,
          status::text as status,
          uploaded_at,
          rejection_reason,
          file_name,
          mime_type,
          content_length,
          s3_bucket,
          s3_key,
          presigned_expires_at,
          created_at,
          updated_at
        "#,
        unchecked
    )]
    async fn insert_kyc_document_intent_db(
        &self,
        id: String,
        external_id: String,
        document_type: String,
        file_name: String,
        mime_type: String,
        content_length: i64,
        s3_bucket: String,
        s3_key: String,
        presigned_expires_at: DateTime<Utc>,
    ) -> sqlx_data::Result<db::KycDocumentRow>;

    #[dml(
        r#"
        SELECT
          external_id,
          first_name,
          last_name,
          email,
          phone_number,
          date_of_birth,
          nationality,
          kyc_tier,
          kyc_status::text as kyc_status,
          submitted_at,
          reviewed_at,
          reviewed_by,
          rejection_reason,
          review_notes,
          created_at,
          updated_at
        FROM kyc_profiles
        WHERE external_id = $1
        "#,
        unchecked
    )]
    async fn get_kyc_profile_db(&self, external_id: String) -> sqlx_data::Result<Option<db::KycProfileRow>>;

    #[dml(
        "SELECT kyc_tier FROM kyc_profiles WHERE external_id = $1",
        unchecked
    )]
    async fn get_kyc_tier_db(&self, external_id: String) -> sqlx_data::Result<Option<i32>>;

    #[dml(
        r#"
        UPDATE kyc_profiles
        SET
          kyc_status = 'APPROVED',
          kyc_tier = $2,
          reviewed_at = now(),
          reviewed_by = 'staff',
          review_notes = $3,
          updated_at = now()
        WHERE external_id = $1
        "#,
        unchecked
    )]
    async fn update_kyc_approved_db(
        &self,
        external_id: String,
        new_tier: i32,
        notes: Option<String>,
    ) -> sqlx_data::Result<QueryResult>;

    #[dml(
        r#"
        UPDATE kyc_profiles
        SET
          kyc_status = 'REJECTED',
          reviewed_at = now(),
          reviewed_by = 'staff',
          rejection_reason = $2,
          review_notes = $3,
          updated_at = now()
        WHERE external_id = $1
        "#,
        unchecked
    )]
    async fn update_kyc_rejected_db(
        &self,
        external_id: String,
        reason: String,
        notes: Option<String>,
    ) -> sqlx_data::Result<QueryResult>;

    #[dml(
        r#"
        UPDATE kyc_profiles
        SET
          kyc_status = 'NEEDS_INFO',
          reviewed_at = now(),
          reviewed_by = 'staff',
          review_notes = $2,
          updated_at = now()
        WHERE external_id = $1
        "#,
        unchecked
    )]
    async fn update_kyc_request_info_db(
        &self,
        external_id: String,
        message: String,
    ) -> sqlx_data::Result<QueryResult>;

    #[dml(
        r#"
        UPDATE kyc_profiles
        SET
          first_name = COALESCE($2, first_name),
          last_name = COALESCE($3, last_name),
          email = COALESCE($4, email),
          phone_number = COALESCE($5, phone_number),
          date_of_birth = COALESCE($6, date_of_birth),
          nationality = COALESCE($7, nationality),
          updated_at = now()
        WHERE external_id = $1
        RETURNING
          external_id,
          first_name,
          last_name,
          email,
          phone_number,
          date_of_birth,
          nationality,
          kyc_tier,
          kyc_status::text as kyc_status,
          submitted_at,
          reviewed_at,
          reviewed_by,
          rejection_reason,
          review_notes,
          created_at,
          updated_at
        "#,
        unchecked
    )]
    async fn patch_kyc_information_db(
        &self,
        external_id: String,
        first_name: Option<String>,
        last_name: Option<String>,
        email: Option<String>,
        phone_number: Option<String>,
        date_of_birth: Option<String>,
        nationality: Option<String>,
    ) -> sqlx_data::Result<Option<db::KycProfileRow>>;

    #[dml(
        r#"
        INSERT INTO users (
          user_id, realm, username, first_name, last_name, email, enabled, email_verified, attributes
        )
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        RETURNING
          user_id, realm, username, first_name, last_name, email, enabled, email_verified,
          attributes, created_at, updated_at
        "#,
        unchecked
    )]
    async fn create_user_db(
        &self,
        user_id: String,
        realm: String,
        username: String,
        first_name: Option<String>,
        last_name: Option<String>,
        email: Option<String>,
        enabled: bool,
        email_verified: bool,
        attributes: Option<Value>,
    ) -> sqlx_data::Result<db::UserRow>;

    #[dml(
        r#"
        SELECT
          user_id, realm, username, first_name, last_name, email, enabled, email_verified,
          attributes, created_at, updated_at
        FROM users
        WHERE user_id = $1
        "#,
        unchecked
    )]
    async fn get_user_db(&self, user_id: String) -> sqlx_data::Result<Option<db::UserRow>>;

    #[dml(
        r#"
        UPDATE users
        SET
          realm = $2,
          username = $3,
          first_name = $4,
          last_name = $5,
          email = $6,
          enabled = $7,
          email_verified = $8,
          attributes = $9,
          updated_at = now()
        WHERE user_id = $1
        RETURNING
          user_id, realm, username, first_name, last_name, email, enabled, email_verified,
          attributes, created_at, updated_at
        "#,
        unchecked
    )]
    async fn update_user_db(
        &self,
        user_id: String,
        realm: String,
        username: String,
        first_name: Option<String>,
        last_name: Option<String>,
        email: Option<String>,
        enabled: bool,
        email_verified: bool,
        attributes: Option<Value>,
    ) -> sqlx_data::Result<Option<db::UserRow>>;

    #[dml("DELETE FROM users WHERE user_id = $1", unchecked)]
    async fn delete_user_db(&self, user_id: String) -> sqlx_data::Result<QueryResult>;

    #[dml(
        r#"
        SELECT
          user_id, realm, username, first_name, last_name, email, enabled, email_verified,
          attributes, created_at, updated_at
        FROM users
        WHERE realm = $1
          AND ($2::text IS NULL OR (
            username ILIKE ('%' || $2 || '%') OR
            email ILIKE ('%' || $2 || '%') OR
            first_name ILIKE ('%' || $2 || '%') OR
            last_name ILIKE ('%' || $2 || '%')
          ))
          AND ($3::text IS NULL OR username = $3)
          AND ($4::text IS NULL OR email = $4)
          AND ($5::boolean IS NULL OR enabled = $5)
          AND ($6::boolean IS NULL OR email_verified = $6)
        ORDER BY created_at DESC
        LIMIT $7
        OFFSET $8
        "#,
        unchecked
    )]
    async fn search_users_db(
        &self,
        realm: String,
        search: Option<String>,
        username: Option<String>,
        email: Option<String>,
        enabled: Option<bool>,
        email_verified: Option<bool>,
        limit: i32,
        offset: i32,
    ) -> sqlx_data::Result<Vec<db::UserRow>>;

    #[dml(
        r#"
        SELECT
          id,
          realm,
          client_id,
          user_id,
          user_hint,
          device_id,
          jkt,
          status::text as status,
          public_jwk,
          attributes,
          proof,
          label,
          created_at,
          last_seen_at
        FROM devices
        WHERE ($1::text IS NULL OR device_id = $1)
          AND ($2::text IS NULL OR jkt = $2)
        LIMIT 1
        "#,
        unchecked
    )]
    async fn lookup_device_db(
        &self,
        device_id: Option<String>,
        jkt: Option<String>,
    ) -> sqlx_data::Result<Option<db::DeviceRow>>;

    #[dml(
        r#"
        SELECT
          id,
          realm,
          client_id,
          user_id,
          user_hint,
          device_id,
          jkt,
          status::text as status,
          public_jwk,
          attributes,
          proof,
          label,
          created_at,
          last_seen_at
        FROM devices
        WHERE user_id = $1
          AND ($2 OR status = 'ACTIVE')
        ORDER BY created_at DESC
        "#,
        unchecked
    )]
    async fn list_user_devices_db(
        &self,
        user_id: String,
        include_revoked: bool,
    ) -> sqlx_data::Result<Vec<db::DeviceRow>>;

    #[dml(
        r#"
        SELECT
          id,
          realm,
          client_id,
          user_id,
          user_hint,
          device_id,
          jkt,
          status::text as status,
          public_jwk,
          attributes,
          proof,
          label,
          created_at,
          last_seen_at
        FROM devices
        WHERE user_id = $1 AND device_id = $2
        "#,
        unchecked
    )]
    async fn get_user_device_db(
        &self,
        user_id: String,
        device_id: String,
    ) -> sqlx_data::Result<Option<db::DeviceRow>>;

    #[dml(
        r#"
        UPDATE devices
        SET status = $2::device_status
        WHERE id = $1
        RETURNING
          id,
          realm,
          client_id,
          user_id,
          user_hint,
          device_id,
          jkt,
          status::text as status,
          public_jwk,
          attributes,
          proof,
          label,
          created_at,
          last_seen_at
        "#,
        unchecked
    )]
    async fn update_device_status_db(&self, record_id: String, status: String)
        -> sqlx_data::Result<db::DeviceRow>;

    #[dml(
        "SELECT id, user_id FROM devices WHERE device_id = $1 OR jkt = $2 LIMIT 1",
        unchecked
    )]
    async fn find_device_binding_db(
        &self,
        device_id: String,
        jkt: String,
    ) -> sqlx_data::Result<Option<(String, String)>>;

    #[dml(
        r#"
        INSERT INTO devices (
          id, realm, client_id, user_id, user_hint, device_id, jkt, public_jwk, attributes, proof
        )
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
        RETURNING id
        "#,
        unchecked
    )]
    async fn bind_device_db(
        &self,
        id: String,
        realm: String,
        client_id: String,
        user_id: String,
        user_hint: Option<String>,
        device_id: String,
        jkt: String,
        public_jwk: Value,
        attributes: Option<Value>,
        proof: Option<Value>,
    ) -> sqlx_data::Result<String>;

    #[dml(
        r#"
        INSERT INTO approvals (
          request_id, realm, client_id, user_id, device_id, jkt, public_jwk,
          platform, model, app_version, reason, expires_at, context, idempotency_key
        )
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
        RETURNING request_id, status::text as status, expires_at
        "#,
        unchecked
    )]
    async fn create_approval_db(
        &self,
        request_id: String,
        realm: String,
        client_id: String,
        user_id: String,
        device_id: String,
        jkt: String,
        public_jwk: Option<Value>,
        platform: Option<String>,
        model: Option<String>,
        app_version: Option<String>,
        reason: Option<String>,
        expires_at: Option<DateTime<Utc>>,
        context: Option<Value>,
        idempotency_key: Option<String>,
    ) -> sqlx_data::Result<(String, String, Option<DateTime<Utc>>)>;

    #[dml(
        r#"
        SELECT
          request_id,
          realm,
          client_id,
          user_id,
          device_id,
          jkt,
          public_jwk,
          platform,
          model,
          app_version,
          reason,
          expires_at,
          context,
          idempotency_key,
          status::text as status,
          created_at,
          decided_at,
          decided_by_device_id,
          message
        FROM approvals
        WHERE request_id = $1
        "#,
        unchecked
    )]
    async fn get_approval_db(&self, request_id: String) -> sqlx_data::Result<Option<db::ApprovalRow>>;

    #[dml(
        r#"
        SELECT
          request_id,
          realm,
          client_id,
          user_id,
          device_id,
          jkt,
          public_jwk,
          platform,
          model,
          app_version,
          reason,
          expires_at,
          context,
          idempotency_key,
          status::text as status,
          created_at,
          decided_at,
          decided_by_device_id,
          message
        FROM approvals
        WHERE user_id = $1
          AND ($2::text[] IS NULL OR status::text = ANY($2))
        ORDER BY created_at DESC
        "#,
        unchecked
    )]
    async fn list_user_approvals_db(
        &self,
        user_id: String,
        statuses: Option<Vec<String>>,
    ) -> sqlx_data::Result<Vec<db::ApprovalRow>>;

    #[dml(
        r#"
        UPDATE approvals
        SET
          status = $2::approval_status,
          decided_at = now(),
          decided_by_device_id = $3,
          message = $4
        WHERE request_id = $1
        RETURNING
          request_id,
          realm,
          client_id,
          user_id,
          device_id,
          jkt,
          public_jwk,
          platform,
          model,
          app_version,
          reason,
          expires_at,
          context,
          idempotency_key,
          status::text as status,
          created_at,
          decided_at,
          decided_by_device_id,
          message
        "#,
        unchecked
    )]
    async fn decide_approval_db(
        &self,
        request_id: String,
        status: String,
        decided_by_device_id: Option<String>,
        message: Option<String>,
    ) -> sqlx_data::Result<Option<db::ApprovalRow>>;

    #[dml("DELETE FROM approvals WHERE request_id = $1", unchecked)]
    async fn cancel_approval_db(&self, request_id: String) -> sqlx_data::Result<QueryResult>;

    #[dml(
        r#"
        SELECT
          user_id, realm, username, first_name, last_name, email, enabled, email_verified,
          attributes, created_at, updated_at
        FROM users
        WHERE realm = $1 AND username = $2
        "#,
        unchecked
    )]
    async fn resolve_user_by_phone_db(
        &self,
        realm: String,
        phone: String,
    ) -> sqlx_data::Result<Option<db::UserRow>>;

    #[dml(
        r#"
        INSERT INTO users (user_id, realm, username, enabled, email_verified, attributes)
        VALUES ($1,$2,$3,TRUE,FALSE,$4)
        RETURNING
          user_id, realm, username, first_name, last_name, email, enabled, email_verified,
          attributes, created_at, updated_at
        "#,
        unchecked
    )]
    async fn create_user_by_phone_db(
        &self,
        user_id: String,
        realm: String,
        phone: String,
        attributes: Value,
    ) -> sqlx_data::Result<db::UserRow>;

    #[dml(
        "SELECT COUNT(*)::int8 FROM devices WHERE user_id = $1",
        unchecked
    )]
    async fn count_user_devices_db(&self, user_id: String) -> sqlx_data::Result<i64>;

    #[dml(
        r#"
        INSERT INTO sms_messages (
          id, realm, client_id, user_id, phone_number, hash, otp_sha256, ttl_seconds,
          max_attempts, next_retry_at, metadata
        )
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,now(),$10)
        "#,
        unchecked
    )]
    async fn queue_sms_db(
        &self,
        id: String,
        realm: String,
        client_id: String,
        user_id: Option<String>,
        phone_number: String,
        hash: String,
        otp_sha256: Vec<u8>,
        ttl_seconds: i32,
        max_attempts: i32,
        metadata: Value,
    ) -> sqlx_data::Result<QueryResult>;

    #[dml(
        r#"
        SELECT
          id,
          realm,
          client_id,
          user_id,
          phone_number,
          hash,
          otp_sha256,
          ttl_seconds,
          status::text as status,
          attempt_count,
          max_attempts,
          next_retry_at,
          last_error,
          sns_message_id,
          session_id,
          trace_id,
          metadata,
          created_at,
          sent_at,
          confirmed_at
        FROM sms_messages
        WHERE hash = $1
        "#,
        unchecked
    )]
    async fn get_sms_by_hash_db(&self, hash: String) -> sqlx_data::Result<Option<db::SmsMessageRow>>;

    #[dml(
        "UPDATE sms_messages SET confirmed_at = now() WHERE hash = $1",
        unchecked
    )]
    async fn mark_sms_confirmed_db(&self, hash: String) -> sqlx_data::Result<QueryResult>;

    #[dml(
        r#"
        SELECT
          id,
          realm,
          client_id,
          user_id,
          phone_number,
          hash,
          otp_sha256,
          ttl_seconds,
          status::text as status,
          attempt_count,
          max_attempts,
          next_retry_at,
          last_error,
          sns_message_id,
          session_id,
          trace_id,
          metadata,
          created_at,
          sent_at,
          confirmed_at
        FROM sms_messages
        WHERE status::text IN ('PENDING', 'FAILED')
          AND (next_retry_at IS NULL OR next_retry_at <= now())
          AND attempt_count < max_attempts
        ORDER BY created_at ASC
        LIMIT $1
        "#,
        unchecked
    )]
    async fn list_retryable_sms_db(&self, limit: i64) -> sqlx_data::Result<Vec<db::SmsMessageRow>>;

    #[dml(
        r#"
        UPDATE sms_messages
        SET
          status = 'SENT',
          attempt_count = attempt_count + 1,
          sns_message_id = $2,
          sent_at = now(),
          last_error = NULL,
          next_retry_at = NULL
        WHERE id = $1
        "#,
        unchecked
    )]
    async fn mark_sms_sent_db(
        &self,
        id: String,
        sns_message_id: Option<String>,
    ) -> sqlx_data::Result<QueryResult>;

    #[dml(
        r#"
        UPDATE sms_messages
        SET
          status = $2::sms_status,
          attempt_count = attempt_count + 1,
          last_error = $3,
          next_retry_at = $4
        WHERE id = $1
        "#,
        unchecked
    )]
    async fn mark_sms_failed_db(
        &self,
        id: String,
        status: String,
        error: String,
        next_retry_at: Option<DateTime<Utc>>,
    ) -> sqlx_data::Result<QueryResult>;

    #[dml(
        "UPDATE sms_messages SET status = 'GAVE_UP', last_error = $2 WHERE id = $1",
        unchecked
    )]
    async fn mark_sms_gave_up_db(&self, id: String, reason: String) -> sqlx_data::Result<QueryResult>;
}

impl PgSqlRepo for PgRepository {
    fn get_pool(&self) -> &sqlx_data::Pool {
        &self.pool
    }
}

impl PgRepository {
    pub async fn ensure_kyc_profile(&self, external_id: &str) -> RepoResult<()> {
        self.ensure_kyc_profile_db(external_id.to_owned()).await?;
        Ok(())
    }

    pub async fn insert_kyc_document_intent(
        &self,
        input: KycDocumentInsert,
    ) -> RepoResult<db::KycDocumentRow> {
        let id = backend_id::kyc_document_id()?;

        let row = self
            .insert_kyc_document_intent_db(
                id,
                input.external_id,
                input.document_type,
                input.file_name,
                input.mime_type,
                input.content_length,
                input.s3_bucket,
                input.s3_key,
                input.presigned_expires_at,
            )
            .await?;
        Ok(row)
    }

    pub async fn get_kyc_profile(&self, external_id: &str) -> RepoResult<Option<db::KycProfileRow>> {
        let row = self.get_kyc_profile_db(external_id.to_owned()).await?;
        Ok(row)
    }

    pub async fn list_kyc_documents(
        &self,
        external_id: String,
        params: impl IntoParams + Send,
    ) -> RepoResult<Serial<db::KycDocumentRow>> {
        let rows = self.list_kyc_documents_db(external_id, params).await?;
        Ok(rows)
    }

    pub async fn get_kyc_tier(&self, external_id: &str) -> RepoResult<Option<i32>> {
        let tier = self.get_kyc_tier_db(external_id.to_owned()).await?;
        Ok(tier)
    }

    pub async fn list_kyc_submissions(
        &self,
        params: impl IntoParams + Send,
    ) -> RepoResult<Serial<db::KycProfileRow>> {
        let rows = self.list_kyc_submissions_db(params).await?;
        Ok(rows)
    }

    pub async fn get_kyc_submission(&self, external_id: &str) -> RepoResult<Option<db::KycProfileRow>> {
        let row = self.get_kyc_profile_db(external_id.to_owned()).await?;
        Ok(row)
    }

    pub async fn update_kyc_approved(
        &self,
        external_id: &str,
        req: &staff_map::KycApprovalRequest,
    ) -> RepoResult<bool> {
        let res = self
            .update_kyc_approved_db(
                external_id.to_owned(),
                req.new_tier as i32,
                req.notes.clone(),
            )
            .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn update_kyc_rejected(
        &self,
        external_id: &str,
        req: &staff_map::KycRejectionRequest,
    ) -> RepoResult<bool> {
        let res = self
            .update_kyc_rejected_db(external_id.to_owned(), req.reason.clone(), req.notes.clone())
            .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn update_kyc_request_info(
        &self,
        external_id: &str,
        req: &staff_map::KycRequestInfoRequest,
    ) -> RepoResult<bool> {
        let res = self
            .update_kyc_request_info_db(external_id.to_owned(), req.message.clone())
            .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn patch_kyc_information(
        &self,
        external_id: &str,
        req: &backend_model::bff::KycInformationPatchRequest,
    ) -> RepoResult<Option<db::KycProfileRow>> {
        let row = self
            .patch_kyc_information_db(
                external_id.to_owned(),
                req.first_name.clone(),
                req.last_name.clone(),
                req.email.clone(),
                req.phone_number.clone(),
                req.date_of_birth.clone(),
                req.nationality.clone(),
            )
            .await?;
        Ok(row)
    }

    pub async fn create_user(&self, req: &kc_map::UserUpsert) -> RepoResult<db::UserRow> {
        let user_id = backend_id::user_id()?;
        let attributes_json = req
            .attributes
            .clone()
            .map(|m| serde_json::to_value(m).unwrap_or_default());

        let row = self
            .create_user_db(
                user_id,
                req.realm.clone(),
                req.username.clone(),
                req.first_name.clone(),
                req.last_name.clone(),
                req.email.clone(),
                req.enabled.unwrap_or(true),
                req.email_verified.unwrap_or(false),
                attributes_json,
            )
            .await?;
        Ok(row)
    }

    pub async fn get_user(&self, user_id: &str) -> RepoResult<Option<db::UserRow>> {
        let row = self.get_user_db(user_id.to_owned()).await?;
        Ok(row)
    }

    pub async fn update_user(
        &self,
        user_id: &str,
        req: &kc_map::UserUpsert,
    ) -> RepoResult<Option<db::UserRow>> {
        let attributes_json = req
            .attributes
            .clone()
            .map(|m| serde_json::to_value(m).unwrap_or_default());

        let row = self
            .update_user_db(
                user_id.to_owned(),
                req.realm.clone(),
                req.username.clone(),
                req.first_name.clone(),
                req.last_name.clone(),
                req.email.clone(),
                req.enabled.unwrap_or(true),
                req.email_verified.unwrap_or(false),
                attributes_json,
            )
            .await?;
        Ok(row)
    }

    pub async fn delete_user(&self, user_id: &str) -> RepoResult<u64> {
        let res = self.delete_user_db(user_id.to_owned()).await?;
        Ok(res.rows_affected())
    }

    pub async fn search_users(&self, req: &kc_map::UserSearch) -> RepoResult<Vec<db::UserRow>> {
        let max_results = req.max_results.unwrap_or(50).clamp(1, 200);
        let first_result = req.first_result.unwrap_or(0).max(0);

        let rows = self
            .search_users_db(
                req.realm.clone(),
                req.search.clone(),
                req.username.clone(),
                req.email.clone(),
                req.enabled,
                req.email_verified,
                max_results,
                first_result,
            )
            .await?;
        Ok(rows)
    }

    pub async fn lookup_device(
        &self,
        req: &kc_map::DeviceLookupRequest,
    ) -> RepoResult<Option<db::DeviceRow>> {
        let row = self.lookup_device_db(req.device_id.clone(), req.jkt.clone()).await?;
        Ok(row)
    }

    pub async fn list_user_devices(
        &self,
        user_id: &str,
        include_revoked: bool,
    ) -> RepoResult<Vec<db::DeviceRow>> {
        let rows = self
            .list_user_devices_db(user_id.to_owned(), include_revoked)
            .await?;
        Ok(rows)
    }

    pub async fn get_user_device(
        &self,
        user_id: &str,
        device_id: &str,
    ) -> RepoResult<Option<db::DeviceRow>> {
        let row = self
            .get_user_device_db(user_id.to_owned(), device_id.to_owned())
            .await?;
        Ok(row)
    }

    pub async fn update_device_status(
        &self,
        record_id: &str,
        status: &str,
    ) -> RepoResult<db::DeviceRow> {
        let row = self
            .update_device_status_db(record_id.to_owned(), status.to_owned())
            .await?;
        Ok(row)
    }

    pub async fn find_device_binding(
        &self,
        device_id: &str,
        jkt: &str,
    ) -> RepoResult<Option<(String, String)>> {
        let row = self
            .find_device_binding_db(device_id.to_owned(), jkt.to_owned())
            .await?;
        Ok(row)
    }

    pub async fn bind_device(&self, req: &kc_map::EnrollmentBindRequest) -> RepoResult<String> {
        let record_id = backend_id::device_id()?;
        let attributes_json = req
            .attributes
            .clone()
            .map(|m| serde_json::to_value(m).unwrap_or_default());
        let public_jwk = serde_json::to_value(req.public_jwk.clone()).unwrap_or_default();
        let proof = req
            .proof
            .clone()
            .map(|m| serde_json::to_value(m).unwrap_or_default());

        let id = self
            .bind_device_db(
                record_id,
                req.realm.clone(),
                req.client_id.clone(),
                req.user_id.clone(),
                req.user_hint.clone(),
                req.device_id.clone(),
                req.jkt.clone(),
                public_jwk,
                attributes_json,
                proof,
            )
            .await?;
        Ok(id)
    }

    pub async fn create_approval(
        &self,
        req: &kc_map::ApprovalCreateRequest,
        idempotency_key: Option<String>,
    ) -> RepoResult<ApprovalCreated> {
        let request_id = backend_id::approval_id()?;
        let public_jwk = req
            .new_device
            .public_jwk
            .clone()
            .map(|m| serde_json::to_value(m).unwrap_or_default());
        let ctx_json = req
            .context
            .clone()
            .map(|m| serde_json::to_value(m).unwrap_or_default());

        let (request_id, status, expires_at) = self
            .create_approval_db(
                request_id,
                req.realm.clone(),
                req.client_id.clone(),
                req.user_id.clone(),
                req.new_device.device_id.clone(),
                req.new_device.jkt.clone(),
                public_jwk,
                req.new_device.platform.clone(),
                req.new_device.model.clone(),
                req.new_device.app_version.clone(),
                req.reason.clone(),
                req.expires_at,
                ctx_json,
                idempotency_key,
            )
            .await?;

        Ok(ApprovalCreated {
            request_id,
            status,
            expires_at,
        })
    }

    pub async fn get_approval(&self, request_id: &str) -> RepoResult<Option<db::ApprovalRow>> {
        let row = self.get_approval_db(request_id.to_owned()).await?;
        Ok(row)
    }

    pub async fn list_user_approvals(
        &self,
        user_id: &str,
        statuses: Option<Vec<String>>,
    ) -> RepoResult<Vec<db::ApprovalRow>> {
        let rows = self.list_user_approvals_db(user_id.to_owned(), statuses).await?;
        Ok(rows)
    }

    pub async fn decide_approval(
        &self,
        request_id: &str,
        req: &kc_map::ApprovalDecisionRequest,
    ) -> RepoResult<Option<db::ApprovalRow>> {
        let status = match req.decision.to_string().as_str() {
            "APPROVE" => "APPROVED".to_owned(),
            "DENY" => "DENIED".to_owned(),
            _ => "DENIED".to_owned(),
        };

        let row = self
            .decide_approval_db(
                request_id.to_owned(),
                status,
                req.decided_by_device_id.clone(),
                req.message.clone(),
            )
            .await?;
        Ok(row)
    }

    pub async fn cancel_approval(&self, request_id: &str) -> RepoResult<u64> {
        let res = self.cancel_approval_db(request_id.to_owned()).await?;
        Ok(res.rows_affected())
    }

    pub async fn resolve_user_by_phone(
        &self,
        realm: &str,
        phone: &str,
    ) -> RepoResult<Option<db::UserRow>> {
        let cache_key = Self::phone_cache_key(realm, phone);
        {
            let mut cache = self
                .resolve_user_by_phone_cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(cached) = cache.get(&cache_key).cloned() {
                return Ok(cached);
            }
        }

        let user = self
            .resolve_user_by_phone_db(realm.to_owned(), phone.to_owned())
            .await?;

        {
            let mut cache = self
                .resolve_user_by_phone_cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            cache.put(cache_key, user.clone());
        }

        Ok(user)
    }

    pub async fn resolve_or_create_user_by_phone(
        &self,
        realm: &str,
        phone: &str,
    ) -> RepoResult<(db::UserRow, bool)> {
        if let Some(user) = self.resolve_user_by_phone(realm, phone).await? {
            return Ok((user, false));
        }

        let user_id = backend_id::user_id()?;
        let attributes_json = json!({ "phone_number": phone });
        let user = self
            .create_user_by_phone_db(
                user_id,
                realm.to_owned(),
                phone.to_owned(),
                attributes_json,
            )
            .await?;

        {
            let mut cache = self
                .resolve_user_by_phone_cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            cache.put(Self::phone_cache_key(realm, phone), Some(user.clone()));
        }

        Ok((user, true))
    }

    pub async fn count_user_devices(&self, user_id: &str) -> RepoResult<i64> {
        let count = self.count_user_devices_db(user_id.to_owned()).await?;
        Ok(count)
    }

    pub async fn queue_sms(&self, sms: SmsPendingInsert) -> RepoResult<SmsQueued> {
        let hash = backend_id::sms_hash()?;
        self.queue_sms_db(
            hash.clone(),
            sms.realm,
            sms.client_id,
            sms.user_id,
            sms.phone_number,
            hash.clone(),
            sms.otp_sha256,
            sms.ttl_seconds,
            sms.max_attempts,
            sms.metadata,
        )
        .await?;

        Ok(SmsQueued {
            hash,
            ttl_seconds: sms.ttl_seconds,
            status: "PENDING".to_owned(),
        })
    }

    pub async fn get_sms_by_hash(&self, hash: &str) -> RepoResult<Option<db::SmsMessageRow>> {
        let row = self.get_sms_by_hash_db(hash.to_owned()).await?;
        Ok(row)
    }

    pub async fn mark_sms_confirmed(&self, hash: &str) -> RepoResult<()> {
        self.mark_sms_confirmed_db(hash.to_owned()).await?;
        Ok(())
    }

    pub async fn list_retryable_sms(&self, limit: i64) -> RepoResult<Vec<db::SmsMessageRow>> {
        let rows = self.list_retryable_sms_db(limit).await?;
        Ok(rows)
    }

    pub async fn mark_sms_sent(&self, id: &str, sns_message_id: Option<String>) -> RepoResult<()> {
        self.mark_sms_sent_db(id.to_owned(), sns_message_id).await?;
        Ok(())
    }

    pub async fn mark_sms_failed(&self, update: SmsPublishFailure) -> RepoResult<()> {
        let status = if update.gave_up {
            "GAVE_UP".to_owned()
        } else {
            "FAILED".to_owned()
        };

        self.mark_sms_failed_db(update.id, status, update.error, update.next_retry_at)
            .await?;
        Ok(())
    }

    pub async fn mark_sms_gave_up(&self, id: &str, reason: &str) -> RepoResult<()> {
        self.mark_sms_gave_up_db(id.to_owned(), reason.to_owned())
            .await?;
        Ok(())
    }
}
