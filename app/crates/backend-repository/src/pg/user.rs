use crate::traits::*;
use backend_core::async_trait;
use backend_model::{db, kc as kc_map};
use diesel::PgJsonbExpressionMethods;
use diesel::prelude::*;
use diesel::upsert::excluded;
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

fn normalize_full_name(name: Option<String>) -> Option<String> {
    name.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    })
}

fn normalize_search_field(value: Option<&String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    })
}

#[async_trait]
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
            full_name: normalize_full_name(req.first_name.clone()),
            email: req.email.clone(),
            email_verified: req.email_verified.unwrap_or(false),
            phone_number: req
                .attributes
                .as_ref()
                .and_then(|a| a.get("phone_number").cloned()),
            fineract_customer_id: None,
            disabled: !req.enabled.unwrap_or(true),
            attributes: attributes_json,
            metadata: serde_json::json!({}),
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
                full_name.eq(normalize_full_name(req.first_name.clone())),
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

        let exact = req.exact.unwrap_or(false);
        let search_filter = normalize_search_field(req.search.as_ref());
        let username_filter = normalize_search_field(req.username.as_ref());
        let first_name_filter = normalize_search_field(req.first_name.as_ref());
        let last_name_filter = normalize_search_field(req.last_name.as_ref());
        let email_filter = normalize_search_field(req.email.as_ref());

        let attribute_filters: Vec<(String, String)> = req
            .attributes
            .as_ref()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|(key, value)| {
                        let key = normalize_search_field(Some(key))?;
                        let value = normalize_search_field(Some(value))?;
                        Some((key, value))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let has_identity_filter = search_filter.is_some()
            || username_filter.is_some()
            || first_name_filter.is_some()
            || last_name_filter.is_some()
            || email_filter.is_some()
            || !attribute_filters.is_empty();

        // Defensive guard for KC federation lookups:
        // if the request contains only realm (no identity filters), do not return
        // arbitrary users from that realm.
        if !has_identity_filter {
            return Ok(vec![]);
        }

        if let Some(search_val) = search_filter {
            if exact {
                query = query.filter(
                    email
                        .eq(search_val.clone())
                        .or(username.eq(search_val.clone()))
                        .or(full_name.eq(search_val)),
                );
            } else {
                let pattern = format!("%{}%", search_val);
                query = query.filter(
                    email
                        .ilike(pattern.clone())
                        .or(username.ilike(pattern.clone()))
                        .or(full_name.ilike(pattern)),
                );
            }
        }

        if let Some(username_val) = username_filter {
            if exact {
                query = query.filter(username.eq(username_val));
            } else {
                query = query.filter(username.ilike(format!("%{}%", username_val)));
            }
        }

        if let Some(email_val) = email_filter {
            if exact {
                query = query.filter(email.eq(email_val));
            } else {
                query = query.filter(email.ilike(format!("%{}%", email_val)));
            }
        }

        if exact {
            match (first_name_filter.clone(), last_name_filter.clone()) {
                (Some(first_name_val), Some(last_name_val)) => {
                    query = query.filter(full_name.eq(format!("{first_name_val} {last_name_val}")));
                }
                (Some(first_name_val), None) => {
                    query = query.filter(full_name.eq(first_name_val));
                }
                (None, Some(last_name_val)) => {
                    query = query.filter(full_name.eq(last_name_val));
                }
                (None, None) => {}
            }
        } else {
            if let Some(first_name_val) = first_name_filter.clone() {
                query = query.filter(full_name.ilike(format!("%{}%", first_name_val)));
            }
            if let Some(last_name_val) = last_name_filter.clone() {
                query = query.filter(full_name.ilike(format!("%{}%", last_name_val)));
            }
        }

        for (key, value) in attribute_filters {
            let contains_value = serde_json::json!({ key: value });
            query = query.filter(attributes.contains(contains_value));
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
            full_name: None,
            fineract_customer_id: None,
            disabled: false,
            attributes: Some(attributes_json),
            metadata: serde_json::json!({}),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let user = diesel::insert_into(app_user)
            .values(&new_user)
            .get_result(&mut conn)
            .await?;

        Ok((user, true))
    }

    async fn upsert_user_data(&self, input: UserDataUpsertInput) -> RepoResult<db::UserDataRow> {
        use backend_model::schema::app_user_data::dsl::*;

        let mut conn = self.get_conn().await?;
        let now = chrono::Utc::now();

        diesel::insert_into(app_user_data)
            .values((
                user_id.eq(input.user_id),
                name.eq(input.name),
                data_type.eq(input.data_type),
                content.eq(input.content),
                eager_fetch.eq(input.eager_fetch),
                created_at.eq(now),
                updated_at.eq(now),
            ))
            .on_conflict((user_id, name, data_type))
            .do_update()
            .set((
                content.eq(excluded(content)),
                eager_fetch.eq(excluded(eager_fetch)),
                updated_at.eq(now),
            ))
            .get_result::<db::UserDataRow>(&mut conn)
            .await
            .map_err(Into::into)
    }

    async fn list_user_data(
        &self,
        user_id_val: &str,
        eager_fetch_only: bool,
    ) -> RepoResult<Vec<db::UserDataRow>> {
        use backend_model::schema::app_user_data::dsl::*;

        let mut conn = self.get_conn().await?;
        let mut query = app_user_data.filter(user_id.eq(user_id_val)).into_boxed();
        if eager_fetch_only {
            query = query.filter(eager_fetch.eq(true));
        }

        query
            .order((name.asc(), data_type.asc()))
            .load::<db::UserDataRow>(&mut conn)
            .await
            .map_err(Into::into)
    }

    async fn update_metadata(&self, user_id_val: &str, metadata_patch: serde_json::Value) -> RepoResult<()> {
        use backend_model::schema::app_user::dsl::*;

        let mut conn = self.get_conn().await?;

        // Note: Ideally we'd do a JSON merge update in SQL, but for simplicity we fetch and patch.
        if let Some(mut user) = app_user
            .filter(user_id.eq(user_id_val))
            .first::<db::UserRow>(&mut conn)
            .await
            .optional()
            .map_err(Into::<backend_core::Error>::into)?
        {
            if let (Some(base_obj), Some(patch_obj)) = (user.metadata.as_object_mut(), metadata_patch.as_object()) {
                for (k, v) in patch_obj {
                    if v.is_null() {
                        base_obj.remove(k);
                    } else {
                        base_obj.insert(k.clone(), v.clone());
                    }
                }
                
                diesel::update(app_user.filter(user_id.eq(user_id_val)))
                    .set((
                        metadata.eq(user.metadata),
                        updated_at.eq(chrono::Utc::now()),
                    ))
                    .execute(&mut conn)
                    .await
                    .map_err(Into::<backend_core::Error>::into)?;
            }
        }

        Ok(())
    }
}
