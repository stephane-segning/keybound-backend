use crate::traits::*;
use backend_core::{Error, async_trait};
use backend_model::db;
use chrono::{DateTime, Utc};
use diesel::dsl::count_star;
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel_async::AsyncPgConnection;
use diesel_async::RunQueryDsl;
use diesel_async::pooled_connection::deadpool::Pool;
use serde_json::Value;

#[derive(Clone)]
pub struct FlowRepository {
    pub(crate) pool: Pool<AsyncPgConnection>,
}

impl FlowRepository {
    pub fn new(pool: Pool<AsyncPgConnection>) -> Self {
        Self { pool }
    }

    async fn get_conn(
        &self,
    ) -> RepoResult<diesel_async::pooled_connection::deadpool::Object<AsyncPgConnection>> {
        self.pool
            .get()
            .await
            .map_err(|error| Error::DieselPool(error.to_string()))
    }
}

#[async_trait]
impl FlowRepo for FlowRepository {
    async fn create_session(
        &self,
        input: FlowSessionCreateInput,
    ) -> RepoResult<db::FlowSessionRow> {
        use backend_model::schema::flow_session;

        let mut conn = self.get_conn().await?;
        let now = Utc::now();
        let row = db::FlowSessionRow {
            id: input.id,
            human_id: input.human_id,
            user_id: input.user_id,
            session_type: input.session_type,
            status: input.status,
            context: input.context,
            created_at: now,
            updated_at: now,
            completed_at: None,
        };

        match diesel::insert_into(flow_session::table)
            .values(&row)
            .get_result::<db::FlowSessionRow>(&mut conn)
            .await
        {
            Ok(created) => Ok(created),
            Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
                flow_session::table
                    .filter(flow_session::human_id.eq(&row.human_id))
                    .select(db::FlowSessionRow::as_select())
                    .first::<db::FlowSessionRow>(&mut conn)
                    .await
                    .optional()
                    .map_err(Error::from)?
                    .ok_or_else(|| {
                        Error::conflict("FLOW_SESSION_CONFLICT", "Session already exists")
                    })
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn get_session(&self, session_id: &str) -> RepoResult<Option<db::FlowSessionRow>> {
        use backend_model::schema::flow_session;

        let mut conn = self.get_conn().await?;
        flow_session::table
            .filter(flow_session::id.eq(session_id))
            .select(db::FlowSessionRow::as_select())
            .first::<db::FlowSessionRow>(&mut conn)
            .await
            .optional()
            .map_err(Into::into)
    }

    async fn list_sessions(
        &self,
        filter: FlowSessionFilter,
    ) -> RepoResult<(Vec<db::FlowSessionRow>, i64)> {
        use backend_model::schema::flow_session;

        let filter = filter.normalized();
        let mut conn = self.get_conn().await?;

        let mut count_query = flow_session::table.into_boxed();
        let mut rows_query = flow_session::table.into_boxed();

        if let Some(user_id) = filter.user_id.as_ref() {
            count_query = count_query.filter(flow_session::user_id.eq(user_id));
            rows_query = rows_query.filter(flow_session::user_id.eq(user_id));
        }
        if let Some(session_type) = filter.session_type.as_ref() {
            count_query = count_query.filter(flow_session::session_type.eq(session_type));
            rows_query = rows_query.filter(flow_session::session_type.eq(session_type));
        }
        if let Some(status) = filter.status.as_ref() {
            count_query = count_query.filter(flow_session::status.eq(status));
            rows_query = rows_query.filter(flow_session::status.eq(status));
        }

        let total = count_query
            .select(count_star())
            .get_result::<i64>(&mut conn)
            .await
            .map_err(Error::from)?;

        let rows = rows_query
            .order(flow_session::updated_at.desc())
            .limit(i64::from(filter.limit))
            .offset(filter.offset())
            .select(db::FlowSessionRow::as_select())
            .load::<db::FlowSessionRow>(&mut conn)
            .await
            .map_err(Error::from)?;

        Ok((rows, total))
    }

    async fn update_session_status(
        &self,
        session_id: &str,
        status: &str,
        completed_at: Option<DateTime<Utc>>,
    ) -> RepoResult<()> {
        use backend_model::schema::flow_session;

        let mut conn = self.get_conn().await?;
        diesel::update(flow_session::table.filter(flow_session::id.eq(session_id)))
            .set((
                flow_session::status.eq(status),
                flow_session::updated_at.eq(Utc::now()),
                flow_session::completed_at.eq(completed_at),
            ))
            .execute(&mut conn)
            .await
            .map_err(Error::from)?;
        Ok(())
    }

    async fn update_session_context(&self, session_id: &str, context: Value) -> RepoResult<()> {
        use backend_model::schema::flow_session;

        let mut conn = self.get_conn().await?;
        diesel::update(flow_session::table.filter(flow_session::id.eq(session_id)))
            .set((
                flow_session::context.eq(context),
                flow_session::updated_at.eq(Utc::now()),
            ))
            .execute(&mut conn)
            .await
            .map_err(Error::from)?;
        Ok(())
    }

    async fn create_flow(&self, input: FlowInstanceCreateInput) -> RepoResult<db::FlowInstanceRow> {
        use backend_model::schema::flow_instance;

        let mut conn = self.get_conn().await?;
        let now = Utc::now();
        let row = db::FlowInstanceRow {
            id: input.id,
            human_id: input.human_id,
            session_id: input.session_id,
            flow_type: input.flow_type,
            status: input.status,
            current_step: input.current_step,
            step_ids: input.step_ids,
            context: input.context,
            created_at: now,
            updated_at: now,
        };

        match diesel::insert_into(flow_instance::table)
            .values(&row)
            .get_result::<db::FlowInstanceRow>(&mut conn)
            .await
        {
            Ok(created) => Ok(created),
            Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
                flow_instance::table
                    .filter(flow_instance::human_id.eq(&row.human_id))
                    .select(db::FlowInstanceRow::as_select())
                    .first::<db::FlowInstanceRow>(&mut conn)
                    .await
                    .optional()
                    .map_err(Error::from)?
                    .ok_or_else(|| Error::conflict("FLOW_INSTANCE_CONFLICT", "Flow already exists"))
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn get_flow(&self, flow_id: &str) -> RepoResult<Option<db::FlowInstanceRow>> {
        use backend_model::schema::flow_instance;

        let mut conn = self.get_conn().await?;
        flow_instance::table
            .filter(flow_instance::id.eq(flow_id))
            .select(db::FlowInstanceRow::as_select())
            .first::<db::FlowInstanceRow>(&mut conn)
            .await
            .optional()
            .map_err(Into::into)
    }

    async fn list_flows_for_session(
        &self,
        session_id: &str,
    ) -> RepoResult<Vec<db::FlowInstanceRow>> {
        use backend_model::schema::flow_instance;

        let mut conn = self.get_conn().await?;
        flow_instance::table
            .filter(flow_instance::session_id.eq(session_id))
            .order(flow_instance::created_at.asc())
            .select(db::FlowInstanceRow::as_select())
            .load::<db::FlowInstanceRow>(&mut conn)
            .await
            .map_err(Into::into)
    }

    async fn update_flow(
        &self,
        flow_id: &str,
        status: Option<String>,
        current_step: Option<Option<String>>,
        step_ids: Option<Value>,
        context: Option<Value>,
    ) -> RepoResult<db::FlowInstanceRow> {
        use backend_model::schema::flow_instance;

        let mut conn = self.get_conn().await?;
        let current = flow_instance::table
            .filter(flow_instance::id.eq(flow_id))
            .select(db::FlowInstanceRow::as_select())
            .first::<db::FlowInstanceRow>(&mut conn)
            .await
            .optional()
            .map_err(Error::from)?
            .ok_or_else(|| Error::not_found("FLOW_NOT_FOUND", "Flow not found"))?;

        let updated = db::FlowInstanceRow {
            status: status.unwrap_or_else(|| current.status.clone()),
            current_step: current_step.unwrap_or_else(|| current.current_step.clone()),
            step_ids: step_ids.unwrap_or_else(|| current.step_ids.clone()),
            context: context.unwrap_or_else(|| current.context.clone()),
            updated_at: Utc::now(),
            ..current
        };

        diesel::update(flow_instance::table.filter(flow_instance::id.eq(flow_id)))
            .set((
                flow_instance::status.eq(&updated.status),
                flow_instance::current_step.eq(&updated.current_step),
                flow_instance::step_ids.eq(&updated.step_ids),
                flow_instance::context.eq(&updated.context),
                flow_instance::updated_at.eq(updated.updated_at),
            ))
            .get_result::<db::FlowInstanceRow>(&mut conn)
            .await
            .map_err(Into::into)
    }

    async fn create_step(&self, input: FlowStepCreateInput) -> RepoResult<db::FlowStepRow> {
        use backend_model::schema::flow_step;

        let mut conn = self.get_conn().await?;
        let now = Utc::now();
        let row = db::FlowStepRow {
            id: input.id,
            human_id: input.human_id,
            flow_id: input.flow_id,
            step_type: input.step_type,
            actor: input.actor,
            status: input.status,
            attempt_no: input.attempt_no,
            input: input.input,
            output: input.output,
            error: input.error,
            next_retry_at: input.next_retry_at,
            created_at: now,
            updated_at: now,
            finished_at: input.finished_at,
        };

        match diesel::insert_into(flow_step::table)
            .values(&row)
            .get_result::<db::FlowStepRow>(&mut conn)
            .await
        {
            Ok(created) => Ok(created),
            Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
                flow_step::table
                    .filter(flow_step::human_id.eq(&row.human_id))
                    .select(db::FlowStepRow::as_select())
                    .first::<db::FlowStepRow>(&mut conn)
                    .await
                    .optional()
                    .map_err(Error::from)?
                    .ok_or_else(|| Error::conflict("FLOW_STEP_CONFLICT", "Step already exists"))
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn get_step(&self, step_id: &str) -> RepoResult<Option<db::FlowStepRow>> {
        use backend_model::schema::flow_step;

        let mut conn = self.get_conn().await?;
        flow_step::table
            .filter(flow_step::id.eq(step_id))
            .select(db::FlowStepRow::as_select())
            .first::<db::FlowStepRow>(&mut conn)
            .await
            .optional()
            .map_err(Into::into)
    }

    async fn list_steps_for_flow(&self, flow_id: &str) -> RepoResult<Vec<db::FlowStepRow>> {
        use backend_model::schema::flow_step;

        let mut conn = self.get_conn().await?;
        flow_step::table
            .filter(flow_step::flow_id.eq(flow_id))
            .order(flow_step::created_at.asc())
            .select(db::FlowStepRow::as_select())
            .load::<db::FlowStepRow>(&mut conn)
            .await
            .map_err(Into::into)
    }

    async fn patch_step(&self, step_id: &str, patch: FlowStepPatch) -> RepoResult<db::FlowStepRow> {
        use backend_model::schema::flow_step;

        let mut conn = self.get_conn().await?;
        let current = flow_step::table
            .filter(flow_step::id.eq(step_id))
            .select(db::FlowStepRow::as_select())
            .first::<db::FlowStepRow>(&mut conn)
            .await
            .optional()
            .map_err(Error::from)?
            .ok_or_else(|| Error::not_found("FLOW_STEP_NOT_FOUND", "Step not found"))?;

        let updated = db::FlowStepRow {
            status: patch.status.unwrap_or_else(|| current.status.clone()),
            input: patch.input.unwrap_or_else(|| current.input.clone()),
            output: patch.output.unwrap_or_else(|| current.output.clone()),
            error: patch.error.unwrap_or_else(|| current.error.clone()),
            next_retry_at: patch.next_retry_at.unwrap_or(current.next_retry_at),
            finished_at: patch.finished_at.unwrap_or(current.finished_at),
            updated_at: Utc::now(),
            ..current
        };

        diesel::update(flow_step::table.filter(flow_step::id.eq(step_id)))
            .set((
                flow_step::status.eq(&updated.status),
                flow_step::input.eq(&updated.input),
                flow_step::output.eq(&updated.output),
                flow_step::error.eq(&updated.error),
                flow_step::next_retry_at.eq(&updated.next_retry_at),
                flow_step::finished_at.eq(&updated.finished_at),
                flow_step::updated_at.eq(updated.updated_at),
            ))
            .get_result::<db::FlowStepRow>(&mut conn)
            .await
            .map_err(Into::into)
    }

    async fn deactivate_signing_keys(&self) -> RepoResult<usize> {
        use backend_model::schema::signing_key;

        let mut conn = self.get_conn().await?;
        diesel::update(signing_key::table.filter(signing_key::is_active.eq(true)))
            .set(signing_key::is_active.eq(false))
            .execute(&mut conn)
            .await
            .map_err(Into::into)
    }

    async fn create_signing_key(
        &self,
        input: SigningKeyCreateInput,
    ) -> RepoResult<db::SigningKeyRow> {
        use backend_model::schema::signing_key;

        let mut conn = self.get_conn().await?;
        let row = db::SigningKeyRow {
            kid: input.kid,
            private_key_pem: input.private_key_pem,
            public_key_jwk: input.public_key_jwk,
            algorithm: input.algorithm,
            created_at: Utc::now(),
            expires_at: input.expires_at,
            is_active: input.is_active,
        };

        diesel::insert_into(signing_key::table)
            .values(&row)
            .get_result::<db::SigningKeyRow>(&mut conn)
            .await
            .map_err(Into::into)
    }

    async fn get_active_signing_key(&self) -> RepoResult<Option<db::SigningKeyRow>> {
        use backend_model::schema::signing_key;

        let mut conn = self.get_conn().await?;
        signing_key::table
            .filter(signing_key::is_active.eq(true))
            .order(signing_key::created_at.desc())
            .select(db::SigningKeyRow::as_select())
            .first::<db::SigningKeyRow>(&mut conn)
            .await
            .optional()
            .map_err(Into::into)
    }

    async fn list_active_signing_keys(&self) -> RepoResult<Vec<db::SigningKeyRow>> {
        use backend_model::schema::signing_key;

        let mut conn = self.get_conn().await?;
        signing_key::table
            .filter(signing_key::is_active.eq(true))
            .order(signing_key::created_at.desc())
            .select(db::SigningKeyRow::as_select())
            .load::<db::SigningKeyRow>(&mut conn)
            .await
            .map_err(Into::into)
    }
}
