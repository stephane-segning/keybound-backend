use crate::traits::*;
use backend_model::db;
use backend_model::{kc as kc_map, staff as staff_map};
use sqlx::{PgPool, Postgres, QueryBuilder};

#[derive(Clone)]
pub struct PgRepository {
    pool: PgPool,
}

impl PgRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

fn map_insert_error(err: sqlx::Error) -> RepoError {
    let sqlx::Error::Database(db_err) = &err else {
        return RepoError::Sqlx(err);
    };
    if db_err.code().as_deref() == Some("23505") {
        RepoError::Conflict
    } else {
        RepoError::Sqlx(err)
    }
}

impl BffRepo for PgRepository {
    async fn ensure_kyc_profile(&self, external_id: &str) -> RepoResult<()> {
        sqlx::query(
            "INSERT INTO kyc_profiles (external_id) VALUES ($1) ON CONFLICT (external_id) DO NOTHING",
        )
        .bind(external_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn insert_kyc_document_intent(&self, input: KycDocumentInsert) -> RepoResult<db::KycDocumentRow> {
        let id = backend_id::kyc_document_id()?;
        let row = sqlx::query_as(
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
        )
        .bind(id)
        .bind(input.external_id)
        .bind(input.document_type)
        .bind(input.file_name)
        .bind(input.mime_type)
        .bind(input.content_length)
        .bind(input.s3_bucket)
        .bind(input.s3_key)
        .bind(input.presigned_expires_at)
        .fetch_one(&self.pool)
        .await?;

        Ok(row)
    }

    async fn get_kyc_profile(&self, external_id: &str) -> RepoResult<Option<db::KycProfileRow>> {
        let row = sqlx::query_as(
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
        )
        .bind(external_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    async fn list_kyc_documents(&self, external_id: &str) -> RepoResult<Vec<db::KycDocumentRow>> {
        let rows = sqlx::query_as(
            r#"
            SELECT
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
            FROM kyc_documents
            WHERE external_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(external_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    async fn get_kyc_tier(&self, external_id: &str) -> RepoResult<Option<i32>> {
        let row: Option<(i32,)> =
            sqlx::query_as("SELECT kyc_tier FROM kyc_profiles WHERE external_id = $1")
                .bind(external_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(tier,)| tier))
    }
}

impl StaffRepo for PgRepository {
    async fn list_kyc_submissions(&self, query: KycSubmissionsQuery) -> RepoResult<KycSubmissionsPage> {
        let mut count_qb: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT COUNT(*)::int4 FROM kyc_profiles WHERE 1=1");
        if let Some(status) = &query.status {
            count_qb.push(" AND kyc_status::text = ");
            count_qb.push_bind(status.clone());
        }
        if let Some(search) = &query.search {
            let like = format!("%{search}%");
            count_qb.push(" AND (external_id ILIKE ");
            count_qb.push_bind(like.clone());
            count_qb.push(" OR email ILIKE ");
            count_qb.push_bind(like.clone());
            count_qb.push(" OR phone_number ILIKE ");
            count_qb.push_bind(like);
            count_qb.push(")");
        }
        let total: i32 = count_qb.build_query_scalar().fetch_one(&self.pool).await?;

        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
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
            WHERE 1=1
            "#,
        );
        if let Some(status) = &query.status {
            qb.push(" AND kyc_status::text = ");
            qb.push_bind(status.clone());
        }
        if let Some(search) = &query.search {
            let like = format!("%{search}%");
            qb.push(" AND (external_id ILIKE ");
            qb.push_bind(like.clone());
            qb.push(" OR email ILIKE ");
            qb.push_bind(like.clone());
            qb.push(" OR phone_number ILIKE ");
            qb.push_bind(like);
            qb.push(")");
        }
        qb.push(" ORDER BY submitted_at DESC NULLS LAST, created_at DESC");
        qb.push(" LIMIT ");
        qb.push_bind(query.limit);
        qb.push(" OFFSET ");
        qb.push_bind((query.page - 1) * query.limit);

        let items: Vec<db::KycProfileRow> = qb.build_query_as().fetch_all(&self.pool).await?;
        Ok(KycSubmissionsPage { total, items })
    }

    async fn get_kyc_submission(&self, external_id: &str) -> RepoResult<Option<db::KycProfileRow>> {
        BffRepo::get_kyc_profile(self, external_id).await
    }

    async fn update_kyc_approved(&self, external_id: &str, req: &staff_map::KycApprovalRequest) -> RepoResult<bool> {
        let updated = sqlx::query(
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
        )
        .bind(external_id)
        .bind(req.new_tier as i32)
        .bind(req.notes.clone())
        .execute(&self.pool)
        .await?
        .rows_affected()
            > 0;

        Ok(updated)
    }

    async fn update_kyc_rejected(&self, external_id: &str, req: &staff_map::KycRejectionRequest) -> RepoResult<bool> {
        let updated = sqlx::query(
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
        )
        .bind(external_id)
        .bind(req.reason.clone())
        .bind(req.notes.clone())
        .execute(&self.pool)
        .await?
        .rows_affected()
            > 0;

        Ok(updated)
    }

    async fn update_kyc_request_info(
        &self,
        external_id: &str,
        req: &staff_map::KycRequestInfoRequest,
    ) -> RepoResult<bool> {
        let updated = sqlx::query(
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
        )
        .bind(external_id)
        .bind(req.message.clone())
        .execute(&self.pool)
        .await?
        .rows_affected()
            > 0;

        Ok(updated)
    }
}

impl KcRepo for PgRepository {
    async fn create_user(&self, req: &kc_map::UserUpsert) -> RepoResult<db::UserRow> {
        let user_id = backend_id::user_id()?;
        let attributes_json = req
            .attributes
            .clone()
            .map(|m| serde_json::to_value(m).unwrap_or_default());

        let row = sqlx::query_as::<_, db::UserRow>(
            r#"
            INSERT INTO users (
              user_id, realm, username, first_name, last_name, email, enabled, email_verified, attributes
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
            RETURNING
              user_id, realm, username, first_name, last_name, email, enabled, email_verified,
              attributes, created_at, updated_at
            "#,
        )
        .bind(user_id)
        .bind(req.realm.clone())
        .bind(req.username.clone())
        .bind(req.first_name.clone())
        .bind(req.last_name.clone())
        .bind(req.email.clone())
        .bind(req.enabled.unwrap_or(true))
        .bind(req.email_verified.unwrap_or(false))
        .bind(attributes_json)
        .fetch_one(&self.pool)
        .await
        .map_err(map_insert_error)?;

        Ok(row)
    }

    async fn get_user(&self, user_id: &str) -> RepoResult<Option<db::UserRow>> {
        let row = sqlx::query_as(
            r#"
            SELECT
              user_id, realm, username, first_name, last_name, email, enabled, email_verified,
              attributes, created_at, updated_at
            FROM users
            WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn update_user(&self, user_id: &str, req: &kc_map::UserUpsert) -> RepoResult<Option<db::UserRow>> {
        let attributes_json = req
            .attributes
            .clone()
            .map(|m| serde_json::to_value(m).unwrap_or_default());

        let row = sqlx::query_as(
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
        )
        .bind(user_id)
        .bind(req.realm.clone())
        .bind(req.username.clone())
        .bind(req.first_name.clone())
        .bind(req.last_name.clone())
        .bind(req.email.clone())
        .bind(req.enabled.unwrap_or(true))
        .bind(req.email_verified.unwrap_or(false))
        .bind(attributes_json)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn delete_user(&self, user_id: &str) -> RepoResult<u64> {
        let affected = sqlx::query("DELETE FROM users WHERE user_id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(affected)
    }

    async fn search_users(&self, req: &kc_map::UserSearch) -> RepoResult<Vec<db::UserRow>> {
        let max_results = req.max_results.unwrap_or(50).clamp(1, 200);
        let first_result = req.first_result.unwrap_or(0).max(0);

        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"
            SELECT
              user_id, realm, username, first_name, last_name, email, enabled, email_verified,
              attributes, created_at, updated_at
            FROM users
            WHERE realm =
            "#,
        );
        qb.push_bind(req.realm.clone());

        if let Some(search) = &req.search {
            let like = format!("%{search}%");
            qb.push(" AND (username ILIKE ");
            qb.push_bind(like.clone());
            qb.push(" OR email ILIKE ");
            qb.push_bind(like.clone());
            qb.push(" OR first_name ILIKE ");
            qb.push_bind(like.clone());
            qb.push(" OR last_name ILIKE ");
            qb.push_bind(like);
            qb.push(")");
        }
        if let Some(username) = &req.username {
            qb.push(" AND username = ");
            qb.push_bind(username.clone());
        }
        if let Some(email) = &req.email {
            qb.push(" AND email = ");
            qb.push_bind(email.clone());
        }
        if let Some(enabled) = req.enabled {
            qb.push(" AND enabled = ");
            qb.push_bind(enabled);
        }
        if let Some(email_verified) = req.email_verified {
            qb.push(" AND email_verified = ");
            qb.push_bind(email_verified);
        }

        qb.push(" ORDER BY created_at DESC");
        qb.push(" LIMIT ");
        qb.push_bind(max_results);
        qb.push(" OFFSET ");
        qb.push_bind(first_result);

        let users = qb.build_query_as().fetch_all(&self.pool).await?;
        Ok(users)
    }

    async fn lookup_device(&self, req: &kc_map::DeviceLookupRequest) -> RepoResult<Option<db::DeviceRow>> {
        let row = sqlx::query_as(
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
        )
        .bind(req.device_id.clone())
        .bind(req.jkt.clone())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn list_user_devices(&self, user_id: &str, include_revoked: bool) -> RepoResult<Vec<db::DeviceRow>> {
        let query = if include_revoked {
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
            ORDER BY created_at DESC
            "#
        } else {
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
              AND status = 'ACTIVE'
            ORDER BY created_at DESC
            "#
        };
        let rows = sqlx::query_as(query).bind(user_id).fetch_all(&self.pool).await?;
        Ok(rows)
    }

    async fn get_user_device(&self, user_id: &str, device_id: &str) -> RepoResult<Option<db::DeviceRow>> {
        let row = sqlx::query_as(
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
        )
        .bind(user_id)
        .bind(device_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn update_device_status(&self, record_id: &str, status: &str) -> RepoResult<db::DeviceRow> {
        let row = sqlx::query_as(
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
        )
        .bind(record_id)
        .bind(status)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    async fn find_device_binding(&self, device_id: &str, jkt: &str) -> RepoResult<Option<(String, String)>> {
        let row = sqlx::query_as(
            "SELECT id, user_id FROM devices WHERE device_id = $1 OR jkt = $2 LIMIT 1",
        )
        .bind(device_id)
        .bind(jkt)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn bind_device(&self, req: &kc_map::EnrollmentBindRequest) -> RepoResult<String> {
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

        let (id,): (String,) = sqlx::query_as(
            r#"
            INSERT INTO devices (
              id, realm, client_id, user_id, user_hint, device_id, jkt, public_jwk, attributes, proof
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
            RETURNING id
            "#,
        )
        .bind(record_id)
        .bind(req.realm.clone())
        .bind(req.client_id.clone())
        .bind(req.user_id.clone())
        .bind(req.user_hint.clone())
        .bind(req.device_id.clone())
        .bind(req.jkt.clone())
        .bind(public_jwk)
        .bind(attributes_json)
        .bind(proof)
        .fetch_one(&self.pool)
        .await
        .map_err(map_insert_error)?;
        Ok(id)
    }

    async fn create_approval(
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

        let created = sqlx::query_as(
            r#"
            INSERT INTO approvals (
              request_id, realm, client_id, user_id, device_id, jkt, public_jwk,
              platform, model, app_version, reason, expires_at, context, idempotency_key
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
            RETURNING request_id, status::text, expires_at
            "#,
        )
        .bind(request_id)
        .bind(req.realm.clone())
        .bind(req.client_id.clone())
        .bind(req.user_id.clone())
        .bind(req.new_device.device_id.clone())
        .bind(req.new_device.jkt.clone())
        .bind(public_jwk)
        .bind(req.new_device.platform.clone())
        .bind(req.new_device.model.clone())
        .bind(req.new_device.app_version.clone())
        .bind(req.reason.clone())
        .bind(req.expires_at)
        .bind(ctx_json)
        .bind(idempotency_key)
        .fetch_one(&self.pool)
        .await
        .map_err(map_insert_error)
        .map(|(request_id, status, expires_at)| ApprovalCreated {
            request_id,
            status,
            expires_at,
        })?;

        Ok(created)
    }

    async fn get_approval(&self, request_id: &str) -> RepoResult<Option<db::ApprovalRow>> {
        let row = sqlx::query_as(
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
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn list_user_approvals(
        &self,
        user_id: &str,
        statuses: Option<Vec<String>>,
    ) -> RepoResult<Vec<db::ApprovalRow>> {
        let rows = sqlx::query_as(
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
        )
        .bind(user_id)
        .bind(statuses)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn decide_approval(
        &self,
        request_id: &str,
        req: &kc_map::ApprovalDecisionRequest,
    ) -> RepoResult<Option<db::ApprovalRow>> {
        let new_status = match req.decision.to_string().as_str() {
            "APPROVE" => "APPROVED",
            "DENY" => "DENIED",
            _ => "DENIED",
        };
        let row = sqlx::query_as(
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
        )
        .bind(request_id)
        .bind(new_status)
        .bind(req.decided_by_device_id.clone())
        .bind(req.message.clone())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn cancel_approval(&self, request_id: &str) -> RepoResult<u64> {
        let affected = sqlx::query("DELETE FROM approvals WHERE request_id = $1")
            .bind(request_id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(affected)
    }

    async fn resolve_user_by_phone(&self, realm: &str, phone: &str) -> RepoResult<Option<db::UserRow>> {
        let user = sqlx::query_as(
            r#"
            SELECT
              user_id, realm, username, first_name, last_name, email, enabled, email_verified,
              attributes, created_at, updated_at
            FROM users
            WHERE realm = $1 AND username = $2
            "#,
        )
        .bind(realm)
        .bind(phone)
        .fetch_optional(&self.pool)
        .await?;
        Ok(user)
    }

    async fn resolve_or_create_user_by_phone(
        &self,
        realm: &str,
        phone: &str,
    ) -> RepoResult<(db::UserRow, bool)> {
        if let Some(user) = self.resolve_user_by_phone(realm, phone).await? {
            return Ok((user, false));
        }

        let user_id = backend_id::user_id()?;
        let attributes_json = serde_json::json!({ "phone_number": phone });
        let user = sqlx::query_as::<_, db::UserRow>(
            r#"
            INSERT INTO users (user_id, realm, username, enabled, email_verified, attributes)
            VALUES ($1,$2,$3,TRUE,FALSE,$4)
            RETURNING
              user_id, realm, username, first_name, last_name, email, enabled, email_verified,
              attributes, created_at, updated_at
            "#,
        )
        .bind(user_id)
        .bind(realm)
        .bind(phone)
        .bind(attributes_json)
        .fetch_one(&self.pool)
        .await
        .map_err(map_insert_error)?;

        Ok((user, true))
    }

    async fn count_user_devices(&self, user_id: &str) -> RepoResult<i64> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM devices WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(count)
    }

    async fn queue_sms(&self, sms: SmsPendingInsert) -> RepoResult<SmsQueued> {
        let hash = backend_id::sms_hash()?;
        sqlx::query(
            r#"
            INSERT INTO sms_messages (
              id, realm, client_id, user_id, phone_number, hash, otp_sha256, ttl_seconds,
              max_attempts, next_retry_at, metadata
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,now(),$10)
            "#,
        )
        .bind(hash.clone())
        .bind(sms.realm)
        .bind(sms.client_id)
        .bind(sms.user_id)
        .bind(sms.phone_number)
        .bind(hash.clone())
        .bind(sms.otp_sha256)
        .bind(sms.ttl_seconds)
        .bind(sms.max_attempts)
        .bind(sms.metadata)
        .execute(&self.pool)
        .await
        .map_err(map_insert_error)?;

        Ok(SmsQueued {
            hash,
            ttl_seconds: sms.ttl_seconds,
            status: "PENDING".to_owned(),
        })
    }

    async fn get_sms_by_hash(&self, hash: &str) -> RepoResult<Option<db::SmsMessageRow>> {
        let row = sqlx::query_as(
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
        )
        .bind(hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn mark_sms_confirmed(&self, hash: &str) -> RepoResult<()> {
        sqlx::query("UPDATE sms_messages SET confirmed_at = now() WHERE hash = $1")
            .bind(hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

impl SmsRetryRepo for PgRepository {
    async fn list_retryable_sms(&self, limit: i64) -> RepoResult<Vec<db::SmsMessageRow>> {
        let rows = sqlx::query_as(
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
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn mark_sms_sent(&self, id: &str, sns_message_id: Option<String>) -> RepoResult<()> {
        sqlx::query(
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
        )
        .bind(id)
        .bind(sns_message_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn mark_sms_failed(&self, update: SmsPublishFailure) -> RepoResult<()> {
        sqlx::query(
            r#"
            UPDATE sms_messages
            SET
              status = $2::sms_status,
              attempt_count = attempt_count + 1,
              last_error = $3,
              next_retry_at = $4
            WHERE id = $1
            "#,
        )
        .bind(update.id)
        .bind(if update.gave_up { "GAVE_UP" } else { "FAILED" })
        .bind(update.error)
        .bind(update.next_retry_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn mark_sms_gave_up(&self, id: &str, reason: &str) -> RepoResult<()> {
        sqlx::query("UPDATE sms_messages SET status = 'GAVE_UP', last_error = $2 WHERE id = $1")
            .bind(id)
            .bind(reason)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
