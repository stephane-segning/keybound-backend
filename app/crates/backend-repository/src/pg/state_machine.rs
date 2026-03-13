use crate::traits::*;
use backend_core::{Error, async_trait};
use backend_model::db;
use chrono::{DateTime, Utc};
use diesel::dsl::{count_star, max};
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel_async::AsyncPgConnection;
use diesel_async::RunQueryDsl;
use diesel_async::pooled_connection::deadpool::Pool;
use serde_json::Value;
use tracing::{debug, info, instrument};

const STAFF_REALM: &str = "staff";

#[derive(Clone)]
pub struct StateMachineRepository {
    pub(crate) pool: Pool<AsyncPgConnection>,
}

impl StateMachineRepository {
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

    fn build_contact_full_name(full_name: Option<String>, username: String) -> String {
        if let Some(full) = full_name {
            let trimmed = full.trim().to_owned();
            if !trimmed.is_empty() {
                return trimmed;
            }
        }
        username
    }
}

#[async_trait]
impl StateMachineRepo for StateMachineRepository {
    #[instrument(skip(self, input))]
    async fn create_instance(&self, input: SmInstanceCreateInput) -> RepoResult<db::SmInstanceRow> {
        use backend_model::schema::sm_instance;

        let mut conn = self.get_conn().await?;
        let now = Utc::now();
        debug!(instance_id = %input.id, kind = %input.kind, "creating state machine instance built");

        let row = db::SmInstanceRow {
            id: input.id,
            kind: input.kind,
            user_id: input.user_id,
            idempotency_key: input.idempotency_key,
            status: input.status,
            context: input.context,
            created_at: now,
            updated_at: now,
            completed_at: None,
        };

        let insert_result = diesel::insert_into(sm_instance::table)
            .values(&row)
            .get_result::<db::SmInstanceRow>(&mut conn)
            .await;

        match insert_result {
            Ok(created) => Ok(created),
            Err(err @ DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
                let existing = sm_instance::table
                    .filter(sm_instance::idempotency_key.eq(&row.idempotency_key))
                    .select(db::SmInstanceRow::as_select())
                    .first::<db::SmInstanceRow>(&mut conn)
                    .await
                    .optional()
                    .map_err(Error::from)?;

                if let Some(existing) = existing {
                    Ok(existing)
                } else {
                    Err(Error::from(err))
                }
            }
            Err(err) => Err(Error::from(err)),
        }
    }

    #[instrument(skip(self))]
    async fn get_instance(&self, instance_id: &str) -> RepoResult<Option<db::SmInstanceRow>> {
        use backend_model::schema::sm_instance;

        debug!(instance_id = instance_id, "fetching state machine instance");
        let mut conn = self.get_conn().await?;
        sm_instance::table
            .filter(sm_instance::id.eq(instance_id))
            .select(db::SmInstanceRow::as_select())
            .first::<db::SmInstanceRow>(&mut conn)
            .await
            .optional()
            .map_err(Error::from)
    }

    #[instrument(skip(self))]
    async fn get_instance_by_idempotency_key(
        &self,
        idempotency_key_val: &str,
    ) -> RepoResult<Option<db::SmInstanceRow>> {
        use backend_model::schema::sm_instance;

        debug!(
            idempotency_key = idempotency_key_val,
            "fetching instance by idempotency key"
        );
        let mut conn = self.get_conn().await?;
        sm_instance::table
            .filter(sm_instance::idempotency_key.eq(idempotency_key_val))
            .select(db::SmInstanceRow::as_select())
            .first::<db::SmInstanceRow>(&mut conn)
            .await
            .optional()
            .map_err(Error::from)
    }

    #[instrument(skip(self, filter))]
    async fn list_instances(
        &self,
        filter: SmInstanceFilter,
    ) -> RepoResult<(Vec<db::SmInstanceRow>, i64)> {
        use backend_model::schema::{app_user, sm_instance};

        let filter = filter.normalized();
        debug!(?filter, "listing state machine instances");
        let mut conn = self.get_conn().await?;

        let mut count_query = sm_instance::table.into_boxed();
        let mut rows_query = sm_instance::table.into_boxed();

        if let Some(kind) = filter.kind.as_ref() {
            count_query = count_query.filter(sm_instance::kind.eq(kind));
            rows_query = rows_query.filter(sm_instance::kind.eq(kind));
        }
        if let Some(status) = filter.status.as_ref() {
            count_query = count_query.filter(sm_instance::status.eq(status));
            rows_query = rows_query.filter(sm_instance::status.eq(status));
        }
        if let Some(user_id) = filter.user_id.as_ref() {
            count_query = count_query.filter(sm_instance::user_id.eq(user_id));
            rows_query = rows_query.filter(sm_instance::user_id.eq(user_id));
        }
        if let Some(phone_number) = filter.phone_number.as_ref() {
            let user_ids = app_user::table
                .filter(app_user::phone_number.eq(phone_number))
                .select(app_user::user_id)
                .load::<String>(&mut conn)
                .await
                .map_err(Error::from)?;

            if user_ids.is_empty() {
                return Ok((Vec::new(), 0));
            }

            let user_ids_nullable = user_ids
                .into_iter()
                .map(Some)
                .collect::<Vec<Option<String>>>();
            count_query =
                count_query.filter(sm_instance::user_id.eq_any(user_ids_nullable.clone()));
            rows_query = rows_query.filter(sm_instance::user_id.eq_any(user_ids_nullable));
        }
        if let Some(from) = filter.created_from {
            count_query = count_query.filter(sm_instance::created_at.ge(from));
            rows_query = rows_query.filter(sm_instance::created_at.ge(from));
        }
        if let Some(to) = filter.created_to {
            count_query = count_query.filter(sm_instance::created_at.le(to));
            rows_query = rows_query.filter(sm_instance::created_at.le(to));
        }

        let total = count_query
            .select(count_star())
            .get_result::<i64>(&mut conn)
            .await
            .map_err(Error::from)?;

        let rows = rows_query
            .order(sm_instance::updated_at.desc())
            .limit(i64::from(filter.limit))
            .offset(filter.offset())
            .select(db::SmInstanceRow::as_select())
            .load::<db::SmInstanceRow>(&mut conn)
            .await
            .map_err(Error::from)?;

        Ok((rows, total))
    }

    #[instrument(skip(self))]
    async fn update_instance_status(
        &self,
        instance_id: &str,
        status: &str,
        completed_at: Option<DateTime<Utc>>,
    ) -> RepoResult<()> {
        use backend_model::schema::sm_instance;

        debug!(instance_id = instance_id, status = status, completed_at = ?completed_at, "updating state machine instance status");
        let mut conn = self.get_conn().await?;
        let now = Utc::now();

        diesel::update(sm_instance::table.filter(sm_instance::id.eq(instance_id)))
            .set((
                sm_instance::status.eq(status),
                sm_instance::updated_at.eq(now),
                sm_instance::completed_at.eq(completed_at),
            ))
            .execute(&mut conn)
            .await
            .map_err(Error::from)?;

        Ok(())
    }

    #[instrument(skip(self))]
    async fn update_instance_context(&self, instance_id: &str, context: Value) -> RepoResult<()> {
        use backend_model::schema::sm_instance;

        debug!(
            instance_id = instance_id,
            "updating state machine instance context"
        );
        let mut conn = self.get_conn().await?;
        let now = Utc::now();

        diesel::update(sm_instance::table.filter(sm_instance::id.eq(instance_id)))
            .set((
                sm_instance::context.eq(context),
                sm_instance::updated_at.eq(now),
            ))
            .execute(&mut conn)
            .await
            .map_err(Error::from)?;

        Ok(())
    }

    #[instrument(skip(self, input))]
    async fn append_event(&self, input: SmEventCreateInput) -> RepoResult<db::SmEventRow> {
        use backend_model::schema::sm_event;

        let mut conn = self.get_conn().await?;
        let now = Utc::now();
        debug!(event_id = %input.id, instance_id = %input.instance_id, kind = %input.kind, "appending state machine event");

        let row = db::SmEventRow {
            id: input.id,
            instance_id: input.instance_id,
            kind: input.kind,
            actor_type: input.actor_type,
            actor_id: input.actor_id,
            payload: input.payload,
            created_at: now,
        };

        diesel::insert_into(sm_event::table)
            .values(&row)
            .get_result::<db::SmEventRow>(&mut conn)
            .await
            .map_err(Error::from)
    }

    #[instrument(skip(self))]
    async fn list_events(&self, instance_id_val: &str) -> RepoResult<Vec<db::SmEventRow>> {
        use backend_model::schema::sm_event;

        debug!(
            instance_id = instance_id_val,
            "listing state machine events"
        );
        let mut conn = self.get_conn().await?;
        sm_event::table
            .filter(sm_event::instance_id.eq(instance_id_val))
            .order(sm_event::created_at.asc())
            .select(db::SmEventRow::as_select())
            .load::<db::SmEventRow>(&mut conn)
            .await
            .map_err(Error::from)
    }

    #[instrument(skip(self, input))]
    async fn create_step_attempt(
        &self,
        input: SmStepAttemptCreateInput,
    ) -> RepoResult<db::SmStepAttemptRow> {
        use backend_model::schema::sm_step_attempt;

        let mut conn = self.get_conn().await?;
        debug!(attempt_id = %input.id, instance_id = %input.instance_id, step_name = %input.step_name, attempt_no = input.attempt_no, "creating state machine step attempt");

        let row = db::SmStepAttemptRow {
            id: input.id,
            instance_id: input.instance_id,
            step_name: input.step_name,
            attempt_no: input.attempt_no,
            status: input.status,
            external_ref: input.external_ref,
            input: input.input,
            output: input.output,
            error: input.error,
            queued_at: input.queued_at,
            started_at: input.started_at,
            finished_at: input.finished_at,
            next_retry_at: input.next_retry_at,
        };

        diesel::insert_into(sm_step_attempt::table)
            .values(&row)
            .get_result::<db::SmStepAttemptRow>(&mut conn)
            .await
            .map_err(Error::from)
    }

    #[instrument(skip(self, patch))]
    async fn patch_step_attempt(
        &self,
        attempt_id: &str,
        patch: SmStepAttemptPatch,
    ) -> RepoResult<db::SmStepAttemptRow> {
        use backend_model::schema::sm_step_attempt;

        debug!(attempt_id = attempt_id, patch = ?patch, "patching state machine step attempt");
        let mut conn = self.get_conn().await?;
        let current = sm_step_attempt::table
            .filter(sm_step_attempt::id.eq(attempt_id))
            .select(db::SmStepAttemptRow::as_select())
            .first::<db::SmStepAttemptRow>(&mut conn)
            .await
            .optional()
            .map_err(Error::from)?
            .ok_or_else(|| Error::not_found("SM_ATTEMPT_NOT_FOUND", "Step attempt not found"))?;

        let updated = db::SmStepAttemptRow {
            status: patch.status.unwrap_or(current.status),
            output: patch.output.unwrap_or(current.output),
            error: patch.error.unwrap_or(current.error),
            queued_at: patch.queued_at.unwrap_or(current.queued_at),
            started_at: patch.started_at.unwrap_or(current.started_at),
            finished_at: patch.finished_at.unwrap_or(current.finished_at),
            next_retry_at: patch.next_retry_at.unwrap_or(current.next_retry_at),
            ..current
        };

        diesel::update(sm_step_attempt::table.filter(sm_step_attempt::id.eq(attempt_id)))
            .set((
                sm_step_attempt::status.eq(&updated.status),
                sm_step_attempt::output.eq(&updated.output),
                sm_step_attempt::error.eq(&updated.error),
                sm_step_attempt::queued_at.eq(&updated.queued_at),
                sm_step_attempt::started_at.eq(&updated.started_at),
                sm_step_attempt::finished_at.eq(&updated.finished_at),
                sm_step_attempt::next_retry_at.eq(&updated.next_retry_at),
            ))
            .get_result::<db::SmStepAttemptRow>(&mut conn)
            .await
            .map_err(Error::from)
    }

    #[instrument(skip(self))]
    async fn claim_step_attempt(
        &self,
        attempt_id_val: &str,
    ) -> RepoResult<Option<db::SmStepAttemptRow>> {
        use backend_model::schema::sm_step_attempt;

        debug!(
            attempt_id = attempt_id_val,
            "claiming state machine step attempt"
        );
        let mut conn = self.get_conn().await?;
        let now = Utc::now();

        diesel::update(
            sm_step_attempt::table
                .filter(sm_step_attempt::id.eq(attempt_id_val))
                .filter(sm_step_attempt::status.eq("QUEUED")),
        )
        .set((
            sm_step_attempt::status.eq("RUNNING"),
            sm_step_attempt::started_at.eq(Some(now)),
            sm_step_attempt::finished_at.eq::<Option<DateTime<Utc>>>(None),
            sm_step_attempt::error.eq::<Option<Value>>(None),
        ))
        .get_result::<db::SmStepAttemptRow>(&mut conn)
        .await
        .optional()
        .map_err(Error::from)
    }

    #[instrument(skip(self))]
    async fn list_step_attempts(
        &self,
        instance_id_val: &str,
    ) -> RepoResult<Vec<db::SmStepAttemptRow>> {
        use backend_model::schema::sm_step_attempt;

        debug!(
            instance_id = instance_id_val,
            "listing state machine step attempts"
        );
        let mut conn = self.get_conn().await?;
        sm_step_attempt::table
            .filter(sm_step_attempt::instance_id.eq(instance_id_val))
            .order((
                sm_step_attempt::step_name.asc(),
                sm_step_attempt::attempt_no.asc(),
            ))
            .select(db::SmStepAttemptRow::as_select())
            .load::<db::SmStepAttemptRow>(&mut conn)
            .await
            .map_err(Error::from)
    }

    #[instrument(skip(self))]
    async fn get_latest_step_attempt(
        &self,
        instance_id_val: &str,
        step_name_val: &str,
    ) -> RepoResult<Option<db::SmStepAttemptRow>> {
        use backend_model::schema::sm_step_attempt;

        debug!(
            instance_id = instance_id_val,
            step_name = step_name_val,
            "fetching latest state machine step attempt"
        );
        let mut conn = self.get_conn().await?;
        sm_step_attempt::table
            .filter(sm_step_attempt::instance_id.eq(instance_id_val))
            .filter(sm_step_attempt::step_name.eq(step_name_val))
            .order(sm_step_attempt::attempt_no.desc())
            .select(db::SmStepAttemptRow::as_select())
            .first::<db::SmStepAttemptRow>(&mut conn)
            .await
            .optional()
            .map_err(Error::from)
    }

    #[instrument(skip(self))]
    async fn get_step_attempt_by_external_ref(
        &self,
        instance_id_val: &str,
        step_name_val: &str,
        external_ref_val: &str,
    ) -> RepoResult<Option<db::SmStepAttemptRow>> {
        use backend_model::schema::sm_step_attempt;

        debug!(
            instance_id = instance_id_val,
            step_name = step_name_val,
            external_ref = external_ref_val,
            "fetching state machine step attempt by external ref"
        );
        let mut conn = self.get_conn().await?;
        sm_step_attempt::table
            .filter(sm_step_attempt::instance_id.eq(instance_id_val))
            .filter(sm_step_attempt::step_name.eq(step_name_val))
            .filter(sm_step_attempt::external_ref.eq(external_ref_val))
            .order(sm_step_attempt::attempt_no.desc())
            .select(db::SmStepAttemptRow::as_select())
            .first::<db::SmStepAttemptRow>(&mut conn)
            .await
            .optional()
            .map_err(Error::from)
    }

    #[instrument(skip(self))]
    async fn cancel_other_attempts_for_step(
        &self,
        instance_id_val: &str,
        step_name_val: &str,
        keep_attempt_id: &str,
    ) -> RepoResult<()> {
        use backend_model::schema::sm_step_attempt;

        debug!(
            instance_id = instance_id_val,
            step_name = step_name_val,
            keep_attempt_id = keep_attempt_id,
            "cancelling other state machine attempts"
        );
        let mut conn = self.get_conn().await?;
        let now = Utc::now();
        diesel::update(
            sm_step_attempt::table
                .filter(sm_step_attempt::instance_id.eq(instance_id_val))
                .filter(sm_step_attempt::step_name.eq(step_name_val))
                .filter(sm_step_attempt::id.ne(keep_attempt_id))
                .filter(sm_step_attempt::status.ne("CANCELLED")),
        )
        .set((
            sm_step_attempt::status.eq("CANCELLED"),
            sm_step_attempt::finished_at.eq(Some(now)),
        ))
        .execute(&mut conn)
        .await
        .map_err(Error::from)?;

        Ok(())
    }

    #[instrument(skip(self))]
    async fn next_attempt_no(&self, instance_id_val: &str, step_name_val: &str) -> RepoResult<i32> {
        use backend_model::schema::sm_step_attempt;

        debug!(
            instance_id = instance_id_val,
            step_name = step_name_val,
            "calculating next attempt number"
        );
        let mut conn = self.get_conn().await?;
        let current_max = sm_step_attempt::table
            .filter(sm_step_attempt::instance_id.eq(instance_id_val))
            .filter(sm_step_attempt::step_name.eq(step_name_val))
            .select(max(sm_step_attempt::attempt_no))
            .get_result::<Option<i32>>(&mut conn)
            .await
            .map_err(Error::from)?;

        Ok(current_max.unwrap_or(0) + 1)
    }

    #[instrument(skip(self))]
    async fn select_deposit_staff_contact(
        &self,
        user_id_val: &str,
    ) -> RepoResult<(String, String, String)> {
        type CandidateRow = (String, Option<String>, String, Option<String>);

        #[instrument(skip(conn))]
        async fn fetch_candidate(
            conn: &mut AsyncPgConnection,
            user_id_val: &str,
            prefer_staff_realm: bool,
            exclude_user: bool,
        ) -> Result<Option<CandidateRow>, Error> {
            use backend_model::schema::app_user;

            let mut query = app_user::table
                .filter(app_user::disabled.eq(false))
                .filter(app_user::phone_number.is_not_null())
                .into_boxed();

            if prefer_staff_realm {
                query = query.filter(app_user::realm.eq(STAFF_REALM));
            }
            if exclude_user {
                query = query.filter(app_user::user_id.ne(user_id_val));
            }

            debug!(prefer_staff_realm, exclude_user, "fetching staff candidate");
            query
                .order(app_user::created_at.asc())
                .select((
                    app_user::user_id,
                    app_user::full_name,
                    app_user::username,
                    app_user::phone_number,
                ))
                .first::<CandidateRow>(conn)
                .await
                .optional()
                .map_err(Error::from)
        }

        debug!(user_id = user_id_val, "selecting deposit staff contact");
        let mut conn = self.get_conn().await?;

        let preferred = fetch_candidate(&mut conn, user_id_val, true, true).await?;
        let fallback = if preferred.is_none() {
            fetch_candidate(&mut conn, user_id_val, false, true).await?
        } else {
            None
        };
        let requester_fallback = if preferred.is_none() && fallback.is_none() {
            fetch_candidate(&mut conn, user_id_val, false, false).await?
        } else {
            None
        };

        let Some((staff_id, full_name, username, phone_number)) =
            preferred.or(fallback).or(requester_fallback)
        else {
            info!("No preferred staff found");
            return Err(Error::bad_request(
                "STAFF_CONTACT_NOT_AVAILABLE",
                "No active staff contact with phone number is available",
            ));
        };

        let staff_phone_number = phone_number.ok_or_else(|| {
            Error::internal(
                "STAFF_CONTACT_INVALID",
                "Staff contact is missing a phone number",
            )
        })?;

        let contact_name = StateMachineRepository::build_contact_full_name(full_name, username);
        debug!(staff_id = %staff_id, contact_name = %contact_name, "selected deposit staff contact");
        Ok((staff_id, contact_name, staff_phone_number))
    }
}
