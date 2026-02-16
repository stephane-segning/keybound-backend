use crate::traits::*;
use backend_model::{db, kc as kc_map};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use sqlx::PgPool;

#[derive(Clone)]
pub struct UserRepository {
    pub(crate) pool: PgPool,
    pub(crate) diesel_pool: deadpool_diesel::Pool<diesel_async::AsyncPgConnection>,
}

impl UserRepository {
    pub fn new(
        pool: PgPool,
        diesel_pool: deadpool_diesel::Pool<diesel_async::AsyncPgConnection>,
    ) -> Self {
        Self { pool, diesel_pool }
    }

    async fn get_conn(
        &self,
    ) -> RepoResult<deadpool_diesel::Object<diesel_async::AsyncPgConnection>> {
        self.diesel_pool
            .get()
            .await
            .map_err(|e| backend_core::Error::DieselPool(e.to_string()))
    }
}

impl UserRepo for UserRepository {
    async fn create_user(&self, req: &kc_map::UserUpsert) -> RepoResult<db::UserRow> {
        use backend_model::schema::app_user::dsl::*;

        let user_id_val = backend_id::user_id()?;
        let mut conn = self.get_conn().await?;

        let new_user = db::UserRow {
            id: user_id_val,
            email: req.email.clone(),
            email_verified: req.email_verified.unwrap_or(false),
            phone_number: req.attributes.as_ref().and_then(|a| a.get("phone_number").cloned()),
            fineract_customer_id: None,
            disabled: !req.enabled.unwrap_or(true),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        diesel::insert_into(app_user)
            .values(&new_user)
            .get_result(&mut conn)
            .await
            .map_err(Into::into)
    }

    async fn get_user(&self, user_id_val: &str) -> RepoResult<Option<db::UserRow>> {
        use backend_model::schema::app_user::dsl::*;

        let mut conn = self.get_conn().await?;

        app_user
            .filter(id.eq(user_id_val))
            .first::<db::UserRow>(&mut conn)
            .await
            .optional()
            .map_err(Into::into)
    }

    async fn update_user(
        &self,
        user_id_val: &str,
        req: &kc_map::UserUpsert,
    ) -> RepoResult<Option<db::UserRow>> {
        use backend_model::schema::app_user::dsl::*;

        let mut conn = self.get_conn().await?;

        diesel::update(app_user.filter(id.eq(user_id_val)))
            .set((
                email.eq(req.email.clone()),
                email_verified.eq(req.email_verified.unwrap_or(false)),
                disabled.eq(!req.enabled.unwrap_or(true)),
                updated_at.eq(chrono::Utc::now()),
            ))
            .get_result::<db::UserRow>(&mut conn)
            .await
            .optional()
            .map_err(Into::into)
    }

    async fn delete_user(&self, user_id_val: &str) -> RepoResult<u64> {
        use backend_model::schema::app_user::dsl::*;

        let mut conn = self.get_conn().await?;

        diesel::delete(app_user.filter(id.eq(user_id_val)))
            .execute(&mut conn)
            .await
            .map(|n| n as u64)
            .map_err(Into::into)
    }

    async fn search_users(&self, req: &kc_map::UserSearch) -> RepoResult<Vec<db::UserRow>> {
        use backend_model::schema::app_user::dsl::*;

        let mut conn = self.get_conn().await?;
        let mut query = app_user.into_boxed();

        if let Some(ref search_val) = req.search {
            let pattern = format!("%{}%", search_val);
            query = query.filter(
                email
                    .ilike(pattern.clone())
                    .or(id.ilike(pattern)),
            );
        }

        if let Some(ref email_val) = req.email {
            query = query.filter(email.eq(email_val));
        }

        if let Some(enabled_val) = req.enabled {
            query = query.filter(disabled.eq(!enabled_val));
        }

        if let Some(email_verified_val) = req.email_verified {
            query = query.filter(email_verified.eq(email_verified_val));
        }

        let limit_val = req.max_results.unwrap_or(50).clamp(1, 200) as i64;
        let offset_val = req.first_result.unwrap_or(0).max(0) as i64;

        query
            .order(created_at.desc())
            .limit(limit_val)
            .offset(offset_val)
            .load::<db::UserRow>(&mut conn)
            .await
            .map_err(Into::into)
    }

    async fn resolve_user_by_phone(
        &self,
        _realm: &str,
        phone: &str,
    ) -> RepoResult<Option<db::UserRow>> {
        use backend_model::schema::app_user::dsl::*;

        let mut conn = self.get_conn().await?;

        app_user
            .filter(phone_number.eq(phone))
            .first::<db::UserRow>(&mut conn)
            .await
            .optional()
            .map_err(Into::into)
    }

    async fn resolve_or_create_user_by_phone(
        &self,
        realm: &str,
        phone: &str,
    ) -> RepoResult<(db::UserRow, bool)> {
        if let Some(user) = self.resolve_user_by_phone(realm, phone).await? {
            return Ok((user, false));
        }

        use backend_model::schema::app_user::dsl::*;
        let user_id_val = backend_id::user_id()?;
        let mut conn = self.get_conn().await?;

        let new_user = db::UserRow {
            id: user_id_val,
            email: None,
            email_verified: false,
            phone_number: Some(phone.to_owned()),
            fineract_customer_id: None,
            disabled: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let user = diesel::insert_into(app_user)
            .values(&new_user)
            .get_result(&mut conn)
            .await?;

        Ok((user, true))
    }
}
