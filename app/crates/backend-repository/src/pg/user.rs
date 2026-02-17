use crate::traits::*;
use backend_model::{db, kc as kc_map};
use diesel::prelude::*;
use diesel_async::AsyncPgConnection;
use diesel_async::RunQueryDsl;
use diesel_async::pooled_connection::deadpool::Pool;

#[derive(Clone)]
pub struct UserRepository {
    pub(crate) pool: Pool<AsyncPgConnection>,
}

impl UserRepository {
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
}

impl UserRepo for UserRepository {
    async fn create_user(&self, req: &kc_map::UserUpsert) -> RepoResult<db::UserRow> {
        use backend_model::schema::app_user::dsl::*;

        let user_id_val = backend_id::user_id()?;
        let mut conn = self.get_conn().await?;

        let attributes_json = req
            .attributes
            .clone()
            .map(|m| serde_json::to_value(m).unwrap_or_default());

        let new_user = db::UserRow {
            user_id: user_id_val,
            realm: req.realm.clone(),
            username: req.username.clone(),
            first_name: req.first_name.clone(),
            last_name: req.last_name.clone(),
            email: req.email.clone(),
            email_verified: req.email_verified.unwrap_or(false),
            phone_number: req
                .attributes
                .as_ref()
                .and_then(|a| a.get("phone_number").cloned()),
            fineract_customer_id: None,
            disabled: !req.enabled.unwrap_or(true),
            attributes: attributes_json,
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
            .filter(user_id.eq(user_id_val))
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

        let attributes_json = req
            .attributes
            .clone()
            .map(|m| serde_json::to_value(m).unwrap_or_default());

        diesel::update(app_user.filter(user_id.eq(user_id_val)))
            .set((
                realm.eq(req.realm.clone()),
                username.eq(req.username.clone()),
                first_name.eq(req.first_name.clone()),
                last_name.eq(req.last_name.clone()),
                email.eq(req.email.clone()),
                email_verified.eq(req.email_verified.unwrap_or(false)),
                disabled.eq(!req.enabled.unwrap_or(true)),
                attributes.eq(attributes_json),
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

        diesel::delete(app_user.filter(user_id.eq(user_id_val)))
            .execute(&mut conn)
            .await
            .map(|n| n as u64)
            .map_err(Into::into)
    }

    async fn search_users(&self, req: &kc_map::UserSearch) -> RepoResult<Vec<db::UserRow>> {
        use backend_model::schema::app_user::dsl::*;

        let mut conn = self.get_conn().await?;
        let mut query = app_user.into_boxed();

        query = query.filter(realm.eq(req.realm.clone()));

        if let Some(ref search_val) = req.search {
            let pattern = format!("%{}%", search_val);
            query = query.filter(
                email
                    .ilike(pattern.clone())
                    .or(username.ilike(pattern.clone()))
                    .or(first_name.ilike(pattern.clone()))
                    .or(last_name.ilike(pattern)),
            );
        }

        if let Some(ref username_val) = req.username {
            query = query.filter(username.eq(username_val));
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
        realm_val: &str,
        phone: &str,
    ) -> RepoResult<Option<db::UserRow>> {
        use backend_model::schema::app_user::dsl::*;

        let mut conn = self.get_conn().await?;

        app_user
            .filter(realm.eq(realm_val))
            .filter(username.eq(phone))
            .first::<db::UserRow>(&mut conn)
            .await
            .optional()
            .map_err(Into::into)
    }

    async fn resolve_or_create_user_by_phone(
        &self,
        realm_val: &str,
        phone: &str,
    ) -> RepoResult<(db::UserRow, bool)> {
        if let Some(user) = self.resolve_user_by_phone(realm_val, phone).await? {
            return Ok((user, false));
        }

        use backend_model::schema::app_user::dsl::*;
        let user_id_val = backend_id::user_id()?;
        let mut conn = self.get_conn().await?;

        let attributes_json = serde_json::json!({ "phone_number": phone });

        let new_user = db::UserRow {
            user_id: user_id_val,
            realm: realm_val.to_owned(),
            username: phone.to_owned(),
            email: None,
            email_verified: false,
            phone_number: Some(phone.to_owned()),
            first_name: None,
            last_name: None,
            fineract_customer_id: None,
            disabled: false,
            attributes: Some(attributes_json),
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
