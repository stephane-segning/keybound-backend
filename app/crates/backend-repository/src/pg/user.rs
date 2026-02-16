use crate::traits::*;
use backend_model::{db, kc as kc_map};
use serde_json::Value;
use sqlx::PgPool;
use sqlx_data::{QueryResult, dml, repo};

#[repo]
pub trait PgUserRepo {
    #[dml(file = "queries/user/create.sql", unchecked)]
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

    #[dml(file = "queries/user/get.sql", unchecked)]
    async fn get_user_db(&self, user_id: String) -> sqlx_data::Result<Option<db::UserRow>>;

    #[dml(file = "queries/user/update.sql", unchecked)]
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

    #[dml(file = "queries/user/delete.sql", unchecked)]
    async fn delete_user_db(&self, user_id: String) -> sqlx_data::Result<QueryResult>;

    #[dml(file = "queries/user/search.sql", unchecked)]
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

    #[dml(file = "queries/user/resolve_by_phone.sql", unchecked)]
    async fn resolve_user_by_phone_db(
        &self,
        realm: String,
        phone: String,
    ) -> sqlx_data::Result<Option<db::UserRow>>;

    #[dml(file = "queries/user/create_by_phone.sql", unchecked)]
    async fn create_user_by_phone_db(
        &self,
        user_id: String,
        realm: String,
        phone: String,
        attributes: Value,
    ) -> sqlx_data::Result<db::UserRow>;
}

#[derive(Clone)]
pub struct UserRepository {
    pub(crate) pool: PgPool,
}

impl PgUserRepo for UserRepository {
    fn get_pool(&self) -> &sqlx_data::Pool {
        &self.pool
    }
}

impl UserRepo for UserRepository {
    async fn create_user(&self, req: &kc_map::UserUpsert) -> RepoResult<db::UserRow> {
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

    async fn get_user(&self, user_id: &str) -> RepoResult<Option<db::UserRow>> {
        let row = self.get_user_db(user_id.to_owned()).await?;
        Ok(row)
    }

    async fn update_user(
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

    async fn delete_user(&self, user_id: &str) -> RepoResult<u64> {
        let res = self.delete_user_db(user_id.to_owned()).await?;
        Ok(res.rows_affected())
    }

    async fn search_users(&self, req: &kc_map::UserSearch) -> RepoResult<Vec<db::UserRow>> {
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

    async fn resolve_user_by_phone(
        &self,
        realm: &str,
        phone: &str,
    ) -> RepoResult<Option<db::UserRow>> {
        let user = self
            .resolve_user_by_phone_db(realm.to_owned(), phone.to_owned())
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
        let user = self
            .create_user_by_phone_db(user_id, realm.to_owned(), phone.to_owned(), attributes_json)
            .await?;

        Ok((user, true))
    }
}

impl UserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}
