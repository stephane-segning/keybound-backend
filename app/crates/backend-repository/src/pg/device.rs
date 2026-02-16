use crate::traits::*;
use backend_model::{db, kc as kc_map};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use diesel_async::pooled_connection::deadpool::Pool;
use diesel_async::AsyncPgConnection;
use sqlx::PgPool;

#[derive(Clone)]
pub struct DeviceRepository {
    pub(crate) pool: PgPool,
    pub(crate) diesel_pool: Pool<AsyncPgConnection>,
}

impl DeviceRepository {
    pub fn new(pool: PgPool, diesel_pool: Pool<AsyncPgConnection>) -> Self {
        Self { pool, diesel_pool }
    }

    async fn get_conn(
        &self,
    ) -> RepoResult<diesel_async::pooled_connection::deadpool::Object<AsyncPgConnection>> {
        self.diesel_pool
            .get()
            .await
            .map_err(|e| backend_core::Error::DieselPool(e.to_string()))
    }
}

impl DeviceRepo for DeviceRepository {
    async fn lookup_device(
        &self,
        req: &kc_map::DeviceLookupRequest,
    ) -> RepoResult<Option<db::DeviceRow>> {
        use backend_model::schema::device::dsl::*;

        let mut conn = self.get_conn().await?;
        let mut query = device.into_boxed();

        if let Some(device_id_val) = &req.device_id {
            query = query.filter(device_id.eq(device_id_val));
        }

        if let Some(jkt_val) = &req.jkt {
            query = query.filter(jkt.eq(jkt_val));
        }

        query
            .first::<db::DeviceRow>(&mut conn)
            .await
            .optional()
            .map_err(Into::into)
    }

    async fn list_user_devices(
        &self,
        user_id_val: &str,
        include_revoked: bool,
    ) -> RepoResult<Vec<db::DeviceRow>> {
        use backend_model::schema::device::dsl::*;

        let mut conn = self.get_conn().await?;
        let mut query = device.filter(user_id.eq(user_id_val)).into_boxed();

        if !include_revoked {
            query = query.filter(status.eq("ACTIVE"));
        }

        query
            .load::<db::DeviceRow>(&mut conn)
            .await
            .map_err(Into::into)
    }

    async fn get_user_device(
        &self,
        user_id_val: &str,
        device_id_val: &str,
    ) -> RepoResult<Option<db::DeviceRow>> {
        use backend_model::schema::device::dsl::*;

        let mut conn = self.get_conn().await?;

        device
            .filter(user_id.eq(user_id_val))
            .filter(device_id.eq(device_id_val))
            .first::<db::DeviceRow>(&mut conn)
            .await
            .optional()
            .map_err(Into::into)
    }

    async fn update_device_status(
        &self,
        device_id_val: &str,
        status_val: &str,
    ) -> RepoResult<db::DeviceRow> {
        use backend_model::schema::device::dsl::*;

        let mut conn = self.get_conn().await?;

        diesel::update(device.filter(device_id.eq(device_id_val)))
            .set(status.eq(status_val))
            .get_result::<db::DeviceRow>(&mut conn)
            .await
            .map_err(Into::into)
    }

    async fn find_device_binding(
        &self,
        device_id_val: &str,
        jkt_val: &str,
    ) -> RepoResult<Option<(String, String)>> {
        use backend_model::schema::device::dsl::*;

        let mut conn = self.get_conn().await?;

        device
            .filter(device_id.eq(device_id_val))
            .filter(jkt.eq(jkt_val))
            .select((user_id, device_id))
            .first::<(String, String)>(&mut conn)
            .await
            .optional()
            .map_err(Into::into)
    }

    async fn bind_device(&self, req: &kc_map::EnrollmentBindRequest) -> RepoResult<String> {
        use backend_model::schema::device::dsl::*;

        let mut conn = self.get_conn().await?;

        let public_jwk_str = serde_json::to_string(&req.public_jwk).unwrap_or_default();

        let new_device = db::DeviceRow {
            device_id: req.device_id.clone(),
            user_id: req.user_id.clone(),
            jkt: req.jkt.clone(),
            public_jwk: public_jwk_str,
            status: "ACTIVE".to_string(),
            label: None, // Label is not provided in EnrollmentBindRequest
            created_at: chrono::Utc::now(),
            last_seen_at: Some(chrono::Utc::now()),
        };

        diesel::insert_into(device)
            .values(&new_device)
            .returning(device_id)
            .get_result::<String>(&mut conn)
            .await
            .map_err(Into::into)
    }

    async fn count_user_devices(&self, user_id_val: &str) -> RepoResult<i64> {
        use backend_model::schema::device::dsl::*;

        let mut conn = self.get_conn().await?;

        device
            .filter(user_id.eq(user_id_val))
            .count()
            .get_result::<i64>(&mut conn)
            .await
            .map_err(Into::into)
    }
}
