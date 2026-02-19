use crate::traits::*;
use backend_core::Error;
use backend_model::db;
use chrono::{DateTime, Utc};
use diesel::dsl::count_star;
use diesel::prelude::*;
use diesel::upsert::excluded;
use diesel_async::AsyncConnection;
use diesel_async::AsyncPgConnection;
use diesel_async::RunQueryDsl;
use diesel_async::pooled_connection::deadpool::Pool;
use serde_json::{Value, json};

const IDENTITY_STEP_TYPE: &str = "IDENTITY";
const REVIEW_STATUS_PENDING: &str = "PENDING";
const REVIEW_STATUS_DONE: &str = "DONE";

#[derive(Clone)]
pub struct KycRepository {
    pub(crate) pool: Pool<AsyncPgConnection>,
}

impl KycRepository {
    pub fn new(pool: Pool<AsyncPgConnection>) -> Self {
        Self { pool }
    }

    async fn get_conn(
        &self,
    ) -> RepoResult<diesel_async::pooled_connection::deadpool::Object<AsyncPgConnection>> {
        self.pool
            .get()
            .await
            .map_err(|e| backend_core::Error::DieselPool(e.to_string()))
    }

    fn required_identity_assets() -> [&'static str; 4] {
        ["SELFIE_CLOSEUP", "SELFIE_WITH_ID", "ID_FRONT", "ID_BACK"]
    }

    async fn load_review_evidence(
        conn: &mut AsyncPgConnection,
        review_step_id: &str,
    ) -> RepoResult<Vec<KycReviewEvidenceRow>> {
        use backend_model::schema::kyc_evidence;

        let evidence_rows = kyc_evidence::table
            .filter(kyc_evidence::step_id.eq(review_step_id))
            .order(kyc_evidence::created_at.asc())
            .select((kyc_evidence::asset_type, kyc_evidence::evidence_id))
            .load::<(String, String)>(conn)
            .await
            .map_err(Error::from)?;

        Ok(evidence_rows
            .into_iter()
            .map(|(asset_type, evidence_id)| KycReviewEvidenceRow {
                asset_type,
                evidence_id,
            })
            .collect())
    }

    fn file_name_from_key(object_key: &str) -> String {
        object_key
            .rsplit('/')
            .next()
            .filter(|segment| !segment.is_empty())
            .unwrap_or(object_key)
            .to_string()
    }
}

impl KycRepo for KycRepository {
    async fn start_or_resume_session(
        &self,
        user_id_val: &str,
    ) -> RepoResult<(db::KycSessionRow, Vec<String>)> {
        use backend_model::schema::{kyc_session, kyc_step};

        let mut conn = self.get_conn().await?;

        conn.transaction::<_, Error, _>(|conn| {
            Box::pin(async move {
                let mut session = kyc_session::table
                    .filter(kyc_session::user_id.eq(user_id_val))
                    .select(db::KycSessionRow::as_select())
                    .first::<db::KycSessionRow>(conn)
                    .await
                    .optional()?
                    .unwrap_or(db::KycSessionRow {
                        id: backend_id::kyc_session_id()?,
                        user_id: user_id_val.to_string(),
                        status: "OPEN".to_string(),
                        created_at: Utc::now(),
                        updated_at: Utc::now(),
                    });

                if session.created_at == session.updated_at {
                    diesel::insert_into(kyc_session::table)
                        .values(&session)
                        .execute(conn)
                        .await?;
                } else {
                    session.updated_at = Utc::now();
                    diesel::update(kyc_session::table.filter(kyc_session::id.eq(&session.id)))
                        .set(kyc_session::updated_at.eq(session.updated_at))
                        .execute(conn)
                        .await?;
                }

                let step_ids = kyc_step::table
                    .filter(kyc_step::session_id.eq(&session.id))
                    .order(kyc_step::created_at.asc())
                    .select(kyc_step::id)
                    .load::<String>(conn)
                    .await?;

                Ok((session, step_ids))
            })
        })
        .await
    }

    async fn create_step(&self, input: KycStepCreateInput) -> RepoResult<db::KycStepRow> {
        use backend_model::schema::{kyc_session, kyc_step};

        let mut conn = self.get_conn().await?;

        conn.transaction::<_, Error, _>(|conn| {
            Box::pin(async move {
                let exists = kyc_session::table
                    .filter(kyc_session::id.eq(&input.session_id))
                    .filter(kyc_session::user_id.eq(&input.user_id))
                    .select(count_star())
                    .get_result::<i64>(conn)
                    .await?
                    > 0;

                if !exists {
                    return Err(Error::not_found(
                        "SESSION_NOT_FOUND",
                        "Session not found for user",
                    ));
                }

                let now = Utc::now();
                let row = db::KycStepRow {
                    id: backend_id::kyc_step_id()?,
                    session_id: input.session_id,
                    user_id: input.user_id,
                    step_type: input.step_type,
                    status: "IN_PROGRESS".to_string(),
                    data: json!({}),
                    policy: input.policy,
                    created_at: now,
                    updated_at: now,
                    submitted_at: None,
                };

                diesel::insert_into(kyc_step::table)
                    .values(&row)
                    .get_result::<db::KycStepRow>(conn)
                    .await
                    .map_err(Error::from)
            })
        })
        .await
    }

    async fn get_step(&self, step_id_val: &str) -> RepoResult<Option<db::KycStepRow>> {
        use backend_model::schema::kyc_step;

        let mut conn = self.get_conn().await?;

        kyc_step::table
            .filter(kyc_step::id.eq(step_id_val))
            .select(db::KycStepRow::as_select())
            .first::<db::KycStepRow>(&mut conn)
            .await
            .optional()
            .map_err(Error::from)
    }

    async fn count_recent_otp_challenges(
        &self,
        step_id_val: &str,
        since: DateTime<Utc>,
    ) -> RepoResult<i64> {
        use backend_model::schema::kyc_otp_challenge;

        let mut conn = self.get_conn().await?;

        kyc_otp_challenge::table
            .filter(kyc_otp_challenge::step_id.eq(step_id_val))
            .filter(kyc_otp_challenge::created_at.ge(since))
            .select(count_star())
            .get_result::<i64>(&mut conn)
            .await
            .map_err(Error::from)
    }

    async fn create_otp_challenge(
        &self,
        input: OtpChallengeCreateInput,
    ) -> RepoResult<db::KycOtpChallengeRow> {
        use backend_model::schema::kyc_otp_challenge;

        let mut conn = self.get_conn().await?;

        let row = db::KycOtpChallengeRow {
            otp_ref: backend_id::kyc_otp_ref()?,
            step_id: input.step_id,
            msisdn: input.msisdn,
            channel: input.channel,
            otp_hash: input.otp_hash,
            expires_at: input.expires_at,
            tries_left: input.tries_left,
            created_at: Utc::now(),
            verified_at: None,
        };

        diesel::insert_into(kyc_otp_challenge::table)
            .values(&row)
            .get_result::<db::KycOtpChallengeRow>(&mut conn)
            .await
            .map_err(Error::from)
    }

    async fn get_otp_challenge(
        &self,
        step_id_val: &str,
        otp_ref_val: &str,
    ) -> RepoResult<Option<db::KycOtpChallengeRow>> {
        use backend_model::schema::kyc_otp_challenge;

        let mut conn = self.get_conn().await?;

        kyc_otp_challenge::table
            .filter(kyc_otp_challenge::step_id.eq(step_id_val))
            .filter(kyc_otp_challenge::otp_ref.eq(otp_ref_val))
            .select(db::KycOtpChallengeRow::as_select())
            .first::<db::KycOtpChallengeRow>(&mut conn)
            .await
            .optional()
            .map_err(Error::from)
    }

    async fn mark_otp_verified(&self, step_id_val: &str, otp_ref_val: &str) -> RepoResult<()> {
        use backend_model::schema::kyc_otp_challenge;

        let mut conn = self.get_conn().await?;

        diesel::update(
            kyc_otp_challenge::table
                .filter(kyc_otp_challenge::step_id.eq(step_id_val))
                .filter(kyc_otp_challenge::otp_ref.eq(otp_ref_val)),
        )
        .set(kyc_otp_challenge::verified_at.eq(Utc::now()))
        .execute(&mut conn)
        .await
        .map(|_| ())
        .map_err(Error::from)
    }

    async fn decrement_otp_tries(&self, step_id_val: &str, otp_ref_val: &str) -> RepoResult<i32> {
        use backend_model::schema::kyc_otp_challenge;

        let mut conn = self.get_conn().await?;

        conn.transaction::<_, Error, _>(|conn| {
            Box::pin(async move {
                let current = kyc_otp_challenge::table
                    .filter(kyc_otp_challenge::step_id.eq(step_id_val))
                    .filter(kyc_otp_challenge::otp_ref.eq(otp_ref_val))
                    .select(kyc_otp_challenge::tries_left)
                    .first::<i32>(conn)
                    .await?;

                let next = (current - 1).max(0);

                diesel::update(
                    kyc_otp_challenge::table
                        .filter(kyc_otp_challenge::step_id.eq(step_id_val))
                        .filter(kyc_otp_challenge::otp_ref.eq(otp_ref_val)),
                )
                .set(kyc_otp_challenge::tries_left.eq(next))
                .execute(conn)
                .await?;

                Ok(next)
            })
        })
        .await
    }

    async fn count_recent_magic_challenges(
        &self,
        step_id_val: &str,
        since: DateTime<Utc>,
    ) -> RepoResult<i64> {
        use backend_model::schema::kyc_magic_email_challenge;

        let mut conn = self.get_conn().await?;

        kyc_magic_email_challenge::table
            .filter(kyc_magic_email_challenge::step_id.eq(step_id_val))
            .filter(kyc_magic_email_challenge::created_at.ge(since))
            .select(count_star())
            .get_result::<i64>(&mut conn)
            .await
            .map_err(Error::from)
    }

    async fn create_magic_challenge(
        &self,
        input: MagicChallengeCreateInput,
    ) -> RepoResult<db::KycMagicEmailChallengeRow> {
        use backend_model::schema::kyc_magic_email_challenge;

        let mut conn = self.get_conn().await?;

        let row = db::KycMagicEmailChallengeRow {
            token_ref: backend_id::kyc_magic_ref()?,
            step_id: input.step_id,
            email: input.email,
            token_hash: input.token_hash,
            expires_at: input.expires_at,
            created_at: Utc::now(),
            verified_at: None,
        };

        diesel::insert_into(kyc_magic_email_challenge::table)
            .values(&row)
            .get_result::<db::KycMagicEmailChallengeRow>(&mut conn)
            .await
            .map_err(Error::from)
    }

    async fn get_magic_challenge(
        &self,
        token_ref_val: &str,
    ) -> RepoResult<Option<db::KycMagicEmailChallengeRow>> {
        use backend_model::schema::kyc_magic_email_challenge;

        let mut conn = self.get_conn().await?;

        kyc_magic_email_challenge::table
            .filter(kyc_magic_email_challenge::token_ref.eq(token_ref_val))
            .select(db::KycMagicEmailChallengeRow::as_select())
            .first::<db::KycMagicEmailChallengeRow>(&mut conn)
            .await
            .optional()
            .map_err(Error::from)
    }

    async fn mark_magic_verified(&self, token_ref_val: &str) -> RepoResult<()> {
        use backend_model::schema::kyc_magic_email_challenge;

        let mut conn = self.get_conn().await?;

        diesel::update(
            kyc_magic_email_challenge::table
                .filter(kyc_magic_email_challenge::token_ref.eq(token_ref_val)),
        )
        .set(kyc_magic_email_challenge::verified_at.eq(Utc::now()))
        .execute(&mut conn)
        .await
        .map(|_| ())
        .map_err(Error::from)
    }

    async fn update_step_status(&self, step_id_val: &str, status_val: &str) -> RepoResult<()> {
        use backend_model::schema::kyc_step;

        let mut conn = self.get_conn().await?;

        diesel::update(kyc_step::table.filter(kyc_step::id.eq(step_id_val)))
            .set((
                kyc_step::status.eq(status_val),
                kyc_step::updated_at.eq(Utc::now()),
            ))
            .execute(&mut conn)
            .await
            .map(|_| ())
            .map_err(Error::from)
    }

    async fn create_upload_intent(
        &self,
        input: UploadIntentCreateInput,
    ) -> RepoResult<db::KycUploadRow> {
        use backend_model::schema::kyc_upload;

        let mut conn = self.get_conn().await?;

        let row = db::KycUploadRow {
            upload_id: backend_id::kyc_upload_id()?,
            step_id: input.step_id,
            user_id: input.user_id,
            purpose: input.purpose,
            asset_type: input.asset_type,
            mime: input.mime,
            size_bytes: input.size_bytes,
            bucket: input.bucket,
            object_key: input.object_key,
            method: input.method,
            url: input.url,
            headers: input.headers,
            multipart: input.multipart,
            expires_at: input.expires_at,
            created_at: Utc::now(),
            completed_at: None,
            etag: None,
            computed_sha256: None,
        };

        diesel::insert_into(kyc_upload::table)
            .values(&row)
            .get_result::<db::KycUploadRow>(&mut conn)
            .await
            .map_err(Error::from)
    }

    async fn complete_upload_and_register_evidence(
        &self,
        input: UploadCompleteInput,
    ) -> RepoResult<UploadCompleteResult> {
        use backend_model::schema::{kyc_evidence, kyc_review_queue, kyc_step, kyc_upload};

        let mut conn = self.get_conn().await?;

        conn.transaction::<_, Error, _>(|conn| {
            Box::pin(async move {
                let upload = kyc_upload::table
                    .filter(kyc_upload::upload_id.eq(&input.upload_id))
                    .for_update()
                    .select(db::KycUploadRow::as_select())
                    .first::<db::KycUploadRow>(conn)
                    .await
                    .optional()?
                    .ok_or_else(|| {
                        Error::not_found("UPLOAD_NOT_FOUND", "Upload intent not found")
                    })?;

                if upload.bucket != input.bucket || upload.object_key != input.object_key {
                    return Err(Error::bad_request(
                        "UPLOAD_MISMATCH",
                        "Upload completion bucket/key do not match upload intent",
                    ));
                }

                if upload.user_id != input.user_id {
                    return Err(Error::unauthorized(
                        "Upload intent does not belong to authenticated user",
                    ));
                }

                if upload.completed_at.is_none() {
                    diesel::update(
                        kyc_upload::table.filter(kyc_upload::upload_id.eq(&input.upload_id)),
                    )
                    .set((
                        kyc_upload::completed_at.eq(Utc::now()),
                        kyc_upload::etag.eq(input.etag.clone()),
                        kyc_upload::computed_sha256.eq(input.computed_sha256.clone()),
                    ))
                    .execute(conn)
                    .await?;
                }

                let evidence = kyc_evidence::table
                    .filter(kyc_evidence::step_id.eq(&upload.step_id))
                    .filter(kyc_evidence::bucket.eq(&upload.bucket))
                    .filter(kyc_evidence::object_key.eq(&upload.object_key))
                    .select(db::KycEvidenceRow::as_select())
                    .first::<db::KycEvidenceRow>(conn)
                    .await
                    .optional()?
                    .map_or_else(
                        || {
                            Ok::<_, Error>(db::KycEvidenceRow {
                                evidence_id: backend_id::kyc_evidence_id()?,
                                step_id: upload.step_id.clone(),
                                asset_type: upload.asset_type.clone(),
                                bucket: upload.bucket.clone(),
                                object_key: upload.object_key.clone(),
                                sha256: input.computed_sha256.clone(),
                                created_at: Utc::now(),
                            })
                        },
                        Ok,
                    )?;

                let inserted = diesel::insert_into(kyc_evidence::table)
                    .values(&evidence)
                    .on_conflict_do_nothing()
                    .execute(conn)
                    .await?;

                let evidence = if inserted > 0 {
                    evidence
                } else {
                    kyc_evidence::table
                        .filter(kyc_evidence::step_id.eq(&upload.step_id))
                        .filter(kyc_evidence::bucket.eq(&upload.bucket))
                        .filter(kyc_evidence::object_key.eq(&upload.object_key))
                        .select(db::KycEvidenceRow::as_select())
                        .first::<db::KycEvidenceRow>(conn)
                        .await?
                };

                let step = kyc_step::table
                    .filter(kyc_step::id.eq(&upload.step_id))
                    .select(db::KycStepRow::as_select())
                    .first::<db::KycStepRow>(conn)
                    .await?;

                let mut moved_to_pending_review = false;

                if step.step_type == IDENTITY_STEP_TYPE {
                    let present_assets = kyc_evidence::table
                        .filter(kyc_evidence::step_id.eq(&step.id))
                        .select(kyc_evidence::asset_type)
                        .distinct()
                        .load::<String>(conn)
                        .await?;

                    let has_all_assets =
                        Self::required_identity_assets()
                            .iter()
                            .all(|required_asset| {
                                present_assets.iter().any(|asset| asset == required_asset)
                            });

                    if has_all_assets {
                        let now = Utc::now();
                        diesel::update(kyc_step::table.filter(kyc_step::id.eq(&step.id)))
                            .set((
                                kyc_step::status.eq("PENDING_REVIEW"),
                                kyc_step::updated_at.eq(now),
                                kyc_step::submitted_at.eq(Some(now)),
                            ))
                            .execute(conn)
                            .await?;

                        diesel::insert_into(kyc_review_queue::table)
                            .values((
                                kyc_review_queue::session_id.eq(step.session_id.clone()),
                                kyc_review_queue::step_id.eq(step.id.clone()),
                                kyc_review_queue::status.eq(REVIEW_STATUS_PENDING),
                                kyc_review_queue::priority.eq(100),
                                kyc_review_queue::created_at.eq(now),
                                kyc_review_queue::updated_at.eq(now),
                            ))
                            .on_conflict((kyc_review_queue::session_id, kyc_review_queue::step_id))
                            .do_update()
                            .set((
                                kyc_review_queue::status.eq(excluded(kyc_review_queue::status)),
                                kyc_review_queue::updated_at.eq(now),
                            ))
                            .execute(conn)
                            .await?;

                        moved_to_pending_review = true;
                    }
                }

                Ok(UploadCompleteResult {
                    evidence,
                    moved_to_pending_review,
                })
            })
        })
        .await
    }

    async fn list_staff_submissions(
        &self,
        filter: KycSubmissionFilter,
    ) -> RepoResult<(Vec<KycStaffSubmissionSummaryRow>, i64)> {
        use backend_model::schema::{app_user, kyc_step};

        let filter = filter.normalized();
        let mut conn = self.get_conn().await?;

        let mut count_query = kyc_step::table
            .inner_join(app_user::table.on(app_user::user_id.eq(kyc_step::user_id)))
            .filter(kyc_step::step_type.eq(IDENTITY_STEP_TYPE))
            .into_boxed();

        let mut rows_query = kyc_step::table
            .inner_join(app_user::table.on(app_user::user_id.eq(kyc_step::user_id)))
            .filter(kyc_step::step_type.eq(IDENTITY_STEP_TYPE))
            .select((
                kyc_step::id,
                app_user::user_id,
                app_user::first_name,
                app_user::last_name,
                app_user::email,
                app_user::phone_number,
                kyc_step::status,
                kyc_step::submitted_at,
            ))
            .into_boxed();

        if let Some(status_filter) = filter.status.as_ref() {
            count_query = count_query.filter(kyc_step::status.eq(status_filter));
            rows_query = rows_query.filter(kyc_step::status.eq(status_filter));
        }

        if let Some(search_filter) = filter.search.as_ref() {
            let pattern = format!("%{search_filter}%");
            let search_clause = app_user::first_name
                .ilike(pattern.clone())
                .or(app_user::last_name.ilike(pattern.clone()))
                .or(app_user::email.ilike(pattern));
            count_query = count_query.filter(search_clause.clone());
            rows_query = rows_query.filter(search_clause);
        }

        let total = count_query
            .select(count_star())
            .get_result::<i64>(&mut conn)
            .await
            .map_err(Error::from)?;

        let rows = rows_query
            .order(kyc_step::updated_at.desc())
            .limit(i64::from(filter.limit))
            .offset(filter.offset())
            .load::<(
                String,
                String,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
                String,
                Option<DateTime<Utc>>,
            )>(&mut conn)
            .await
            .map_err(Error::from)?;

        Ok((
            rows.into_iter()
                .map(
                    |(
                        submission_id,
                        user_id,
                        first_name,
                        last_name,
                        email,
                        phone_number,
                        status,
                        submitted_at,
                    )| KycStaffSubmissionSummaryRow {
                        submission_id,
                        user_id,
                        first_name,
                        last_name,
                        email,
                        phone_number,
                        status,
                        submitted_at,
                    },
                )
                .collect(),
            total,
        ))
    }

    async fn get_staff_submission(
        &self,
        submission_id_val: &str,
    ) -> RepoResult<Option<KycStaffSubmissionDetailRow>> {
        use backend_model::schema::{app_user, kyc_review_decision, kyc_step};

        let mut conn = self.get_conn().await?;

        let row = kyc_step::table
            .inner_join(app_user::table.on(app_user::user_id.eq(kyc_step::user_id)))
            .filter(kyc_step::id.eq(submission_id_val))
            .filter(kyc_step::step_type.eq(IDENTITY_STEP_TYPE))
            .select((
                kyc_step::id,
                app_user::user_id,
                app_user::first_name,
                app_user::last_name,
                app_user::email,
                app_user::phone_number,
                app_user::attributes,
                kyc_step::status,
                kyc_step::submitted_at,
                kyc_step::data,
            ))
            .first::<(
                String,
                String,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<Value>,
                String,
                Option<DateTime<Utc>>,
                Value,
            )>(&mut conn)
            .await
            .optional()
            .map_err(Error::from)?;

        let Some((
            submission_id,
            user_id,
            first_name,
            last_name,
            email,
            phone_number,
            user_attributes,
            status,
            submitted_at,
            step_data,
        )) = row
        else {
            return Ok(None);
        };

        let latest_decision = kyc_review_decision::table
            .filter(kyc_review_decision::step_id.eq(submission_id_val))
            .order(kyc_review_decision::decided_at.desc())
            .select((
                kyc_review_decision::outcome,
                kyc_review_decision::reason_code,
                kyc_review_decision::comment,
                kyc_review_decision::decided_at,
                kyc_review_decision::reviewer_id,
            ))
            .first::<(
                String,
                String,
                Option<String>,
                DateTime<Utc>,
                Option<String>,
            )>(&mut conn)
            .await
            .optional()
            .map_err(Error::from)?;

        let mut date_of_birth = None;
        let mut nationality = None;

        if let Some(attrs) = user_attributes {
            date_of_birth = attrs
                .get("date_of_birth")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            nationality = attrs
                .get("nationality")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }

        if date_of_birth.is_none() {
            date_of_birth = step_data
                .get("date_of_birth")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }

        if nationality.is_none() {
            nationality = step_data
                .get("nationality")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }

        let rejection_reason_from_data = step_data
            .get("rejection_reason")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);

        let (reviewed_at, reviewed_by, rejection_reason, review_notes) =
            if let Some((outcome, reason_code, comment, decided_at, reviewer_id)) = latest_decision
            {
                let rejection_reason = if outcome == "REJECT" {
                    rejection_reason_from_data.or(Some(reason_code))
                } else {
                    None
                };

                (Some(decided_at), reviewer_id, rejection_reason, comment)
            } else {
                (None, None, rejection_reason_from_data, None)
            };

        Ok(Some(KycStaffSubmissionDetailRow {
            submission_id,
            user_id,
            first_name,
            last_name,
            email,
            phone_number,
            date_of_birth,
            nationality,
            status,
            submitted_at,
            reviewed_at,
            reviewed_by,
            rejection_reason,
            review_notes,
        }))
    }

    async fn list_staff_submission_documents(
        &self,
        submission_id_val: &str,
    ) -> RepoResult<Vec<KycStaffDocumentRow>> {
        use backend_model::schema::{kyc_evidence, kyc_upload};

        let mut conn = self.get_conn().await?;

        let evidence_rows = kyc_evidence::table
            .filter(kyc_evidence::step_id.eq(submission_id_val))
            .order(kyc_evidence::created_at.desc())
            .select((
                kyc_evidence::evidence_id,
                kyc_evidence::asset_type,
                kyc_evidence::bucket,
                kyc_evidence::object_key,
                kyc_evidence::created_at,
            ))
            .load::<(String, String, String, String, DateTime<Utc>)>(&mut conn)
            .await
            .map_err(Error::from)?;

        let mut result = Vec::with_capacity(evidence_rows.len());
        for (evidence_id, asset_type, bucket, object_key, created_at) in evidence_rows {
            let mime = kyc_upload::table
                .filter(kyc_upload::step_id.eq(submission_id_val))
                .filter(kyc_upload::bucket.eq(&bucket))
                .filter(kyc_upload::object_key.eq(&object_key))
                .order(kyc_upload::created_at.desc())
                .select(kyc_upload::mime)
                .first::<String>(&mut conn)
                .await
                .optional()
                .map_err(Error::from)?
                .unwrap_or_else(|| "application/octet-stream".to_string());

            result.push(KycStaffDocumentRow {
                id: evidence_id,
                submission_id: submission_id_val.to_string(),
                document_type: asset_type,
                file_name: Self::file_name_from_key(&object_key),
                mime_type: mime,
                bucket,
                object_key,
                uploaded_at: created_at,
            });
        }

        Ok(result)
    }

    async fn get_staff_submission_document(
        &self,
        submission_id_val: &str,
        document_id_val: &str,
    ) -> RepoResult<Option<KycStaffDocumentRow>> {
        let rows = self
            .list_staff_submission_documents(submission_id_val)
            .await?;
        Ok(rows.into_iter().find(|row| row.id == document_id_val))
    }

    async fn approve_submission(
        &self,
        submission_id_val: &str,
        reviewer_id: Option<&str>,
        notes: Option<&str>,
    ) -> RepoResult<bool> {
        use backend_model::schema::{kyc_review_decision, kyc_review_queue, kyc_step};

        let mut conn = self.get_conn().await?;

        conn.transaction::<_, Error, _>(|conn| {
            Box::pin(async move {
                let step = kyc_step::table
                    .filter(kyc_step::id.eq(submission_id_val))
                    .filter(kyc_step::step_type.eq(IDENTITY_STEP_TYPE))
                    .select(db::KycStepRow::as_select())
                    .first::<db::KycStepRow>(conn)
                    .await
                    .optional()?;

                let Some(step) = step else {
                    return Ok(false);
                };

                let now = Utc::now();
                diesel::update(kyc_step::table.filter(kyc_step::id.eq(&step.id)))
                    .set((
                        kyc_step::status.eq("VERIFIED"),
                        kyc_step::updated_at.eq(now),
                    ))
                    .execute(conn)
                    .await?;

                diesel::insert_into(kyc_review_decision::table)
                    .values((
                        kyc_review_decision::session_id.eq(step.session_id.clone()),
                        kyc_review_decision::step_id.eq(step.id.clone()),
                        kyc_review_decision::outcome.eq("APPROVE"),
                        kyc_review_decision::reason_code.eq("OK"),
                        kyc_review_decision::comment.eq(notes.map(ToOwned::to_owned)),
                        kyc_review_decision::decided_at.eq(now),
                        kyc_review_decision::reviewer_id.eq(reviewer_id.map(ToOwned::to_owned)),
                    ))
                    .execute(conn)
                    .await?;

                diesel::insert_into(kyc_review_queue::table)
                    .values((
                        kyc_review_queue::session_id.eq(step.session_id.clone()),
                        kyc_review_queue::step_id.eq(step.id.clone()),
                        kyc_review_queue::status.eq(REVIEW_STATUS_DONE),
                        kyc_review_queue::priority.eq(100),
                        kyc_review_queue::created_at.eq(now),
                        kyc_review_queue::updated_at.eq(now),
                    ))
                    .on_conflict((kyc_review_queue::session_id, kyc_review_queue::step_id))
                    .do_update()
                    .set((
                        kyc_review_queue::status.eq(REVIEW_STATUS_DONE),
                        kyc_review_queue::updated_at.eq(now),
                    ))
                    .execute(conn)
                    .await?;

                Ok(true)
            })
        })
        .await
    }

    async fn reject_submission(
        &self,
        submission_id_val: &str,
        reviewer_id: Option<&str>,
        reason: &str,
        notes: Option<&str>,
    ) -> RepoResult<bool> {
        use backend_model::schema::{kyc_review_decision, kyc_review_queue, kyc_step};

        let mut conn = self.get_conn().await?;

        conn.transaction::<_, Error, _>(|conn| {
            Box::pin(async move {
                let step = kyc_step::table
                    .filter(kyc_step::id.eq(submission_id_val))
                    .filter(kyc_step::step_type.eq(IDENTITY_STEP_TYPE))
                    .select(db::KycStepRow::as_select())
                    .first::<db::KycStepRow>(conn)
                    .await
                    .optional()?;

                let Some(step) = step else {
                    return Ok(false);
                };

                let now = Utc::now();
                let mut updated_data = step.data;
                updated_data["rejection_reason"] = Value::String(reason.to_string());

                diesel::update(kyc_step::table.filter(kyc_step::id.eq(&step.id)))
                    .set((
                        kyc_step::status.eq("REJECTED"),
                        kyc_step::updated_at.eq(now),
                        kyc_step::data.eq(updated_data),
                    ))
                    .execute(conn)
                    .await?;

                let merged_comment = if let Some(notes_val) = notes {
                    Some(format!("reason: {reason}\nnotes: {notes_val}"))
                } else {
                    Some(format!("reason: {reason}"))
                };

                diesel::insert_into(kyc_review_decision::table)
                    .values((
                        kyc_review_decision::session_id.eq(step.session_id.clone()),
                        kyc_review_decision::step_id.eq(step.id.clone()),
                        kyc_review_decision::outcome.eq("REJECT"),
                        kyc_review_decision::reason_code.eq("OTHER"),
                        kyc_review_decision::comment.eq(merged_comment),
                        kyc_review_decision::decided_at.eq(now),
                        kyc_review_decision::reviewer_id.eq(reviewer_id.map(ToOwned::to_owned)),
                    ))
                    .execute(conn)
                    .await?;

                diesel::insert_into(kyc_review_queue::table)
                    .values((
                        kyc_review_queue::session_id.eq(step.session_id.clone()),
                        kyc_review_queue::step_id.eq(step.id.clone()),
                        kyc_review_queue::status.eq(REVIEW_STATUS_DONE),
                        kyc_review_queue::priority.eq(100),
                        kyc_review_queue::created_at.eq(now),
                        kyc_review_queue::updated_at.eq(now),
                    ))
                    .on_conflict((kyc_review_queue::session_id, kyc_review_queue::step_id))
                    .do_update()
                    .set((
                        kyc_review_queue::status.eq(REVIEW_STATUS_DONE),
                        kyc_review_queue::updated_at.eq(now),
                    ))
                    .execute(conn)
                    .await?;

                Ok(true)
            })
        })
        .await
    }

    async fn request_submission_info(
        &self,
        submission_id_val: &str,
        message: &str,
    ) -> RepoResult<bool> {
        use backend_model::schema::{kyc_review_queue, kyc_step};

        let mut conn = self.get_conn().await?;

        conn.transaction::<_, Error, _>(|conn| {
            Box::pin(async move {
                let step = kyc_step::table
                    .filter(kyc_step::id.eq(submission_id_val))
                    .filter(kyc_step::step_type.eq(IDENTITY_STEP_TYPE))
                    .select(db::KycStepRow::as_select())
                    .first::<db::KycStepRow>(conn)
                    .await
                    .optional()?;

                let Some(step) = step else {
                    return Ok(false);
                };

                let now = Utc::now();
                let mut updated_data = step.data;
                updated_data["request_info_message"] = Value::String(message.to_string());

                diesel::update(kyc_step::table.filter(kyc_step::id.eq(&step.id)))
                    .set((
                        kyc_step::status.eq("IN_PROGRESS"),
                        kyc_step::updated_at.eq(now),
                        kyc_step::data.eq(updated_data),
                    ))
                    .execute(conn)
                    .await?;

                diesel::insert_into(kyc_review_queue::table)
                    .values((
                        kyc_review_queue::session_id.eq(step.session_id.clone()),
                        kyc_review_queue::step_id.eq(step.id.clone()),
                        kyc_review_queue::status.eq(REVIEW_STATUS_DONE),
                        kyc_review_queue::priority.eq(100),
                        kyc_review_queue::created_at.eq(now),
                        kyc_review_queue::updated_at.eq(now),
                    ))
                    .on_conflict((kyc_review_queue::session_id, kyc_review_queue::step_id))
                    .do_update()
                    .set((
                        kyc_review_queue::status.eq(REVIEW_STATUS_DONE),
                        kyc_review_queue::updated_at.eq(now),
                    ))
                    .execute(conn)
                    .await?;

                Ok(true)
            })
        })
        .await
    }

    async fn list_review_cases(
        &self,
        page: i32,
        limit: i32,
    ) -> RepoResult<(Vec<KycReviewCaseRow>, i64)> {
        use backend_model::schema::{app_user, kyc_review_queue, kyc_step};

        let page = page.max(1);
        let limit = limit.clamp(1, 100);
        let offset = i64::from((page - 1) * limit);

        let mut conn = self.get_conn().await?;

        let total = kyc_review_queue::table
            .inner_join(kyc_step::table.on(kyc_step::id.eq(kyc_review_queue::step_id)))
            .filter(kyc_review_queue::status.eq(REVIEW_STATUS_PENDING))
            .filter(kyc_step::step_type.eq(IDENTITY_STEP_TYPE))
            .select(count_star())
            .get_result::<i64>(&mut conn)
            .await
            .map_err(Error::from)?;

        let rows = kyc_review_queue::table
            .inner_join(kyc_step::table.on(kyc_step::id.eq(kyc_review_queue::step_id)))
            .inner_join(app_user::table.on(app_user::user_id.eq(kyc_step::user_id)))
            .filter(kyc_review_queue::status.eq(REVIEW_STATUS_PENDING))
            .filter(kyc_step::step_type.eq(IDENTITY_STEP_TYPE))
            .order((
                kyc_review_queue::priority.desc(),
                kyc_review_queue::created_at.asc(),
            ))
            .limit(i64::from(limit))
            .offset(offset)
            .select((
                kyc_step::id,
                kyc_step::user_id,
                kyc_step::status,
                kyc_step::submitted_at,
                kyc_step::updated_at,
                app_user::first_name,
                app_user::last_name,
            ))
            .load::<(
                String,
                String,
                String,
                Option<DateTime<Utc>>,
                DateTime<Utc>,
                Option<String>,
                Option<String>,
            )>(&mut conn)
            .await
            .map_err(Error::from)?;

        let mut result = Vec::with_capacity(rows.len());
        for (step_id, user_id, status, submitted_at, updated_at, first_name, last_name) in rows {
            let evidence = Self::load_review_evidence(&mut conn, &step_id).await?;
            result.push(KycReviewCaseRow {
                case_id: step_id.clone(),
                user_id,
                step_id,
                status,
                submitted_at: submitted_at.unwrap_or(updated_at),
                first_name: first_name.unwrap_or_default(),
                middle_name: None,
                last_name: last_name.unwrap_or_default(),
                evidence,
            });
        }

        Ok((result, total))
    }

    async fn get_review_case(&self, case_id_val: &str) -> RepoResult<Option<KycReviewCaseRow>> {
        use backend_model::schema::{app_user, kyc_step};

        let mut conn = self.get_conn().await?;

        let row = kyc_step::table
            .inner_join(app_user::table.on(app_user::user_id.eq(kyc_step::user_id)))
            .filter(kyc_step::id.eq(case_id_val))
            .filter(kyc_step::step_type.eq(IDENTITY_STEP_TYPE))
            .select((
                kyc_step::id,
                kyc_step::user_id,
                kyc_step::status,
                kyc_step::submitted_at,
                kyc_step::updated_at,
                app_user::first_name,
                app_user::last_name,
            ))
            .first::<(
                String,
                String,
                String,
                Option<DateTime<Utc>>,
                DateTime<Utc>,
                Option<String>,
                Option<String>,
            )>(&mut conn)
            .await
            .optional()
            .map_err(Error::from)?;

        let Some((step_id, user_id, status, submitted_at, updated_at, first_name, last_name)) = row
        else {
            return Ok(None);
        };

        let evidence = Self::load_review_evidence(&mut conn, &step_id).await?;

        Ok(Some(KycReviewCaseRow {
            case_id: step_id.clone(),
            user_id,
            step_id,
            status,
            submitted_at: submitted_at.unwrap_or(updated_at),
            first_name: first_name.unwrap_or_default(),
            middle_name: None,
            last_name: last_name.unwrap_or_default(),
            evidence,
        }))
    }

    async fn decide_review_case(
        &self,
        case_id_val: &str,
        outcome: &str,
        reason_code: &str,
        comment: Option<&str>,
        reviewer_id: Option<&str>,
    ) -> RepoResult<Option<KycReviewDecisionRecord>> {
        use backend_model::schema::{kyc_review_decision, kyc_review_queue, kyc_step};

        let mut conn = self.get_conn().await?;

        conn.transaction::<_, Error, _>(|conn| {
            Box::pin(async move {
                let step = kyc_step::table
                    .filter(kyc_step::id.eq(case_id_val))
                    .filter(kyc_step::step_type.eq(IDENTITY_STEP_TYPE))
                    .select(db::KycStepRow::as_select())
                    .first::<db::KycStepRow>(conn)
                    .await
                    .optional()?;

                let Some(step) = step else {
                    return Ok(None);
                };

                let new_step_status = if outcome == "APPROVE" {
                    "VERIFIED"
                } else {
                    "REJECTED"
                };

                let now = Utc::now();
                diesel::update(kyc_step::table.filter(kyc_step::id.eq(&step.id)))
                    .set((
                        kyc_step::status.eq(new_step_status),
                        kyc_step::updated_at.eq(now),
                    ))
                    .execute(conn)
                    .await?;

                diesel::insert_into(kyc_review_decision::table)
                    .values((
                        kyc_review_decision::session_id.eq(step.session_id.clone()),
                        kyc_review_decision::step_id.eq(step.id.clone()),
                        kyc_review_decision::outcome.eq(outcome),
                        kyc_review_decision::reason_code.eq(reason_code),
                        kyc_review_decision::comment.eq(comment.map(ToOwned::to_owned)),
                        kyc_review_decision::decided_at.eq(now),
                        kyc_review_decision::reviewer_id.eq(reviewer_id.map(ToOwned::to_owned)),
                    ))
                    .execute(conn)
                    .await?;

                diesel::insert_into(kyc_review_queue::table)
                    .values((
                        kyc_review_queue::session_id.eq(step.session_id.clone()),
                        kyc_review_queue::step_id.eq(step.id.clone()),
                        kyc_review_queue::status.eq(REVIEW_STATUS_DONE),
                        kyc_review_queue::priority.eq(100),
                        kyc_review_queue::created_at.eq(now),
                        kyc_review_queue::updated_at.eq(now),
                    ))
                    .on_conflict((kyc_review_queue::session_id, kyc_review_queue::step_id))
                    .do_update()
                    .set((
                        kyc_review_queue::status.eq(REVIEW_STATUS_DONE),
                        kyc_review_queue::updated_at.eq(now),
                    ))
                    .execute(conn)
                    .await?;

                Ok(Some(KycReviewDecisionRecord {
                    case_id: step.id,
                    decision: outcome.to_string(),
                    decided_at: now,
                }))
            })
        })
        .await
    }
}
