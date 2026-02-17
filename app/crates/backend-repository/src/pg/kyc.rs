use crate::traits::*;
use backend_model::{db, staff as staff_map};
use chrono::Utc;
use diesel::prelude::*;
use diesel_async::AsyncPgConnection;
use diesel_async::pooled_connection::deadpool::Pool;
use diesel_async::{AsyncConnection, RunQueryDsl};

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

    fn calculate_tier(&self, documents: &[db::KycDocumentRow]) -> i32 {
        let has_identity = documents.iter().any(|d| d.doc_type == "Identity");
        let has_address = documents.iter().any(|d| d.doc_type == "Address");

        if has_identity && has_address {
            2
        } else if has_identity {
            1
        } else {
            0
        }
    }
}

impl KycRepo for KycRepository {
    async fn ensure_kyc_profile(&self, external_id_val: &str) -> RepoResult<()> {
        use backend_model::schema::{kyc_case, kyc_submission};

        let mut conn = self.get_conn().await?;

        conn.transaction::<_, backend_core::Error, _>(|conn| {
            Box::pin(async move {
                let case_id: Option<String> = kyc_case::table
                    .filter(kyc_case::user_id.eq(external_id_val))
                    .select(kyc_case::id)
                    .first::<String>(conn)
                    .await
                    .optional()?;

                if let Some(cid) = case_id {
                    let exists = kyc_submission::table
                        .filter(kyc_submission::kyc_case_id.eq(&cid))
                        .select(diesel::dsl::count_star())
                        .get_result::<i64>(conn)
                        .await?
                        > 0;

                    if !exists {
                        let sub_id = backend_id::kyc_submission_id()?;
                        diesel::insert_into(kyc_submission::table)
                            .values((
                                kyc_submission::id.eq(sub_id),
                                kyc_submission::kyc_case_id.eq(cid),
                                kyc_submission::version.eq(1),
                                kyc_submission::status.eq("DRAFT"),
                                kyc_submission::provisioning_status.eq("PENDING"),
                                kyc_submission::created_at.eq(Utc::now()),
                                kyc_submission::updated_at.eq(Utc::now()),
                            ))
                            .execute(conn)
                            .await?;
                    }
                }
                Ok(())
            })
        })
        .await?;

        Ok(())
    }

    async fn insert_kyc_document_intent(
        &self,
        input: KycDocumentInsert,
    ) -> RepoResult<db::KycDocumentRow> {
        use backend_model::schema::{kyc_case, kyc_document, kyc_submission};

        let mut conn = self.get_conn().await?;
        let doc_id = backend_id::kyc_document_id()?;

        let row = conn
            .transaction::<_, backend_core::Error, _>(|conn| {
                Box::pin(async move {
                    let sub_id: String = kyc_submission::table
                        .inner_join(
                            kyc_case::table.on(kyc_case::id.eq(kyc_submission::kyc_case_id)),
                        )
                        .filter(kyc_case::user_id.eq(&input.external_id))
                        .filter(kyc_submission::status.eq("DRAFT"))
                        .select(kyc_submission::id)
                        .first::<String>(conn)
                        .await?;

                    diesel::insert_into(kyc_document::table)
                        .values((
                            kyc_document::id.eq(doc_id),
                            kyc_document::submission_id.eq(sub_id),
                            kyc_document::doc_type.eq(input.document_type),
                            kyc_document::s3_bucket.eq(input.s3_bucket),
                            kyc_document::s3_key.eq(input.s3_key),
                            kyc_document::file_name.eq(input.file_name),
                            kyc_document::mime_type.eq(input.mime_type),
                            kyc_document::size_bytes.eq(input.content_length),
                            kyc_document::sha256.eq(""), // Placeholder as per original intent
                            kyc_document::status.eq("PENDING"),
                            kyc_document::uploaded_at.eq(Utc::now()),
                        ))
                        .get_result::<db::KycDocumentRow>(conn)
                        .await
                        .map_err(|e| backend_core::Error::Diesel(e))
                })
            })
            .await?;

        Ok(row)
    }

    async fn get_kyc_profile(
        &self,
        external_id_val: &str,
    ) -> RepoResult<Option<db::KycSubmissionRow>> {
        use backend_model::schema::{kyc_case, kyc_submission};

        let mut conn = self.get_conn().await?;

        kyc_submission::table
            .inner_join(kyc_case::table.on(kyc_case::id.eq(kyc_submission::kyc_case_id)))
            .filter(kyc_case::user_id.eq(external_id_val))
            .filter(kyc_submission::status.eq("DRAFT"))
            .select(db::KycSubmissionRow::as_select())
            .first::<db::KycSubmissionRow>(&mut conn)
            .await
            .optional()
            .map_err(|e| backend_core::Error::Diesel(e))
    }

    async fn list_kyc_documents(
        &self,
        external_id_val: String,
    ) -> RepoResult<Vec<db::KycDocumentRow>> {
        use backend_model::schema::{kyc_case, kyc_document, kyc_submission};

        let mut conn = self.get_conn().await?;

        let rows = kyc_document::table
            .inner_join(
                kyc_submission::table.on(kyc_submission::id.eq(kyc_document::submission_id)),
            )
            .inner_join(kyc_case::table.on(kyc_case::id.eq(kyc_submission::kyc_case_id)))
            .filter(kyc_case::user_id.eq(external_id_val))
            .select(db::KycDocumentRow::as_select())
            .load::<db::KycDocumentRow>(&mut conn)
            .await
            .map_err(|e| backend_core::Error::Diesel(e))?;

        Ok(rows)
    }

    async fn get_kyc_document(
        &self,
        external_id_val: &str,
        document_id_val: &str,
    ) -> RepoResult<Option<db::KycDocumentRow>> {
        use backend_model::schema::{kyc_case, kyc_document, kyc_submission};

        let mut conn = self.get_conn().await?;

        kyc_document::table
            .inner_join(
                kyc_submission::table.on(kyc_submission::id.eq(kyc_document::submission_id)),
            )
            .inner_join(kyc_case::table.on(kyc_case::id.eq(kyc_submission::kyc_case_id)))
            .filter(kyc_case::user_id.eq(external_id_val))
            .filter(kyc_document::id.eq(document_id_val))
            .select(db::KycDocumentRow::as_select())
            .first::<db::KycDocumentRow>(&mut conn)
            .await
            .optional()
            .map_err(|e| backend_core::Error::Diesel(e))
    }

    async fn get_kyc_tier(&self, external_id_val: &str) -> RepoResult<Option<i32>> {
        let docs = self.list_kyc_documents(external_id_val.to_string()).await?;
        if docs.is_empty() {
            return Ok(None);
        }
        Ok(Some(self.calculate_tier(&docs)))
    }

    async fn list_kyc_submissions(&self) -> RepoResult<Vec<db::KycSubmissionRow>> {
        use backend_model::schema::kyc_submission;

        let mut conn = self.get_conn().await?;

        let rows = kyc_submission::table
            .select(db::KycSubmissionRow::as_select())
            .load::<db::KycSubmissionRow>(&mut conn)
            .await
            .map_err(|e| backend_core::Error::Diesel(e))?;

        Ok(rows)
    }

    async fn get_kyc_submission(
        &self,
        external_id_val: &str,
    ) -> RepoResult<Option<db::KycSubmissionRow>> {
        self.get_kyc_profile(external_id_val).await
    }

    async fn update_kyc_approved(
        &self,
        external_id_val: &str,
        req: &staff_map::KycApprovalRequest,
    ) -> RepoResult<bool> {
        use backend_model::schema::{kyc_case, kyc_submission};

        let mut conn = self.get_conn().await?;

        let res = conn
            .transaction::<_, backend_core::Error, _>(|conn| {
                Box::pin(async move {
                    let sub_id: Option<String> = kyc_submission::table
                        .inner_join(
                            kyc_case::table.on(kyc_case::id.eq(kyc_submission::kyc_case_id)),
                        )
                        .filter(kyc_case::user_id.eq(external_id_val))
                        .filter(kyc_submission::status.eq("SUBMITTED"))
                        .select(kyc_submission::id)
                        .first::<String>(conn)
                        .await
                        .optional()?;

                    if let Some(sid) = sub_id {
                        diesel::update(kyc_submission::table.filter(kyc_submission::id.eq(&sid)))
                            .set((
                                kyc_submission::status.eq("APPROVED"),
                                kyc_submission::decided_at.eq(Utc::now()),
                                kyc_submission::review_notes.eq(req.notes.clone()),
                                kyc_submission::updated_at.eq(Utc::now()),
                            ))
                            .execute(conn)
                            .await?;

                        diesel::update(
                            kyc_case::table.filter(kyc_case::user_id.eq(external_id_val)),
                        )
                        .set((
                            kyc_case::updated_at.eq(Utc::now()),
                        ))
                        .execute(conn)
                        .await?;

                        Ok(true)
                    } else {
                        Ok(false)
                    }
                })
            })
            .await?;

        Ok(res)
    }

    async fn update_kyc_rejected(
        &self,
        external_id_val: &str,
        req: &staff_map::KycRejectionRequest,
    ) -> RepoResult<bool> {
        use backend_model::schema::{kyc_case, kyc_submission};

        let mut conn = self.get_conn().await?;

        let res = conn
            .transaction::<_, backend_core::Error, _>(|conn| {
                Box::pin(async move {
                    let sub_id: Option<String> = kyc_submission::table
                        .inner_join(
                            kyc_case::table.on(kyc_case::id.eq(kyc_submission::kyc_case_id)),
                        )
                        .filter(kyc_case::user_id.eq(external_id_val))
                        .filter(kyc_submission::status.eq("SUBMITTED"))
                        .select(kyc_submission::id)
                        .first::<String>(conn)
                        .await
                        .optional()?;

                    if let Some(sid) = sub_id {
                        diesel::update(kyc_submission::table.filter(kyc_submission::id.eq(&sid)))
                            .set((
                                kyc_submission::status.eq("REJECTED"),
                                kyc_submission::decided_at.eq(Utc::now()),
                                kyc_submission::rejection_reason.eq(req.reason.clone()),
                                kyc_submission::review_notes.eq(req.notes.clone()),
                                kyc_submission::updated_at.eq(Utc::now()),
                            ))
                            .execute(conn)
                            .await?;

                        Ok(true)
                    } else {
                        Ok(false)
                    }
                })
            })
            .await?;

        Ok(res)
    }

    async fn update_kyc_request_info(
        &self,
        external_id_val: &str,
        req: &staff_map::KycRequestInfoRequest,
    ) -> RepoResult<bool> {
        use backend_model::schema::{kyc_case, kyc_submission};

        let mut conn = self.get_conn().await?;

        let res = conn
            .transaction::<_, backend_core::Error, _>(|conn| {
                Box::pin(async move {
                    let sub_id: Option<String> = kyc_submission::table
                        .inner_join(
                            kyc_case::table.on(kyc_case::id.eq(kyc_submission::kyc_case_id)),
                        )
                        .filter(kyc_case::user_id.eq(external_id_val))
                        .filter(kyc_submission::status.eq("SUBMITTED"))
                        .select(kyc_submission::id)
                        .first::<String>(conn)
                        .await
                        .optional()?;

                    if let Some(sid) = sub_id {
                        diesel::update(kyc_submission::table.filter(kyc_submission::id.eq(&sid)))
                            .set((
                                kyc_submission::status.eq("DRAFT"),
                                kyc_submission::review_notes.eq(req.message.clone()),
                                kyc_submission::updated_at.eq(Utc::now()),
                            ))
                            .execute(conn)
                            .await?;

                        Ok(true)
                    } else {
                        Ok(false)
                    }
                })
            })
            .await?;

        Ok(res)
    }

    async fn submit_kyc_profile(
        &self,
        submission_id_val: &str,
        external_id_val: &str,
    ) -> RepoResult<bool> {
        use backend_model::schema::{kyc_case, kyc_submission};

        let mut conn = self.get_conn().await?;

        let rows_affected = diesel::update(kyc_submission::table)
            .filter(kyc_submission::id.eq(submission_id_val))
            .filter(kyc_submission::status.eq("DRAFT"))
            .filter(
                kyc_submission::kyc_case_id.eq_any(
                    kyc_case::table
                        .filter(kyc_case::user_id.eq(external_id_val))
                        .select(kyc_case::id),
                ),
            )
            .set((
                kyc_submission::status.eq("SUBMITTED"),
                kyc_submission::submitted_at.eq(Utc::now()),
                kyc_submission::updated_at.eq(Utc::now()),
            ))
            .execute(&mut conn)
            .await
            .map_err(|e| backend_core::Error::Diesel(e))?;

        Ok(rows_affected > 0)
    }

    async fn patch_kyc_profile(
        &self,
        external_id_val: &str,
        expected_version_val: Option<i32>,
        req: &backend_model::bff::KycInformationPatchRequest,
    ) -> RepoResult<Option<db::KycSubmissionRow>> {
        use backend_model::schema::{kyc_case, kyc_submission};

        let mut conn = self.get_conn().await?;

        #[derive(AsChangeset)]
        #[diesel(table_name = kyc_submission)]
        struct KycUpdate {
            first_name: Option<String>,
            last_name: Option<String>,
            email: Option<String>,
            phone_number: Option<String>,
            date_of_birth: Option<String>,
            nationality: Option<String>,
            updated_at: chrono::DateTime<chrono::Utc>,
        }

        let update = KycUpdate {
            first_name: req.first_name.clone(),
            last_name: req.last_name.clone(),
            email: req.email.clone(),
            phone_number: req.phone_number.clone(),
            date_of_birth: req.date_of_birth.clone(),
            nationality: req.nationality.clone(),
            updated_at: Utc::now(),
        };

        let mut query = diesel::update(kyc_submission::table)
            .filter(kyc_submission::status.eq("DRAFT"))
            .filter(
                kyc_submission::kyc_case_id.eq_any(
                    kyc_case::table
                        .filter(kyc_case::user_id.eq(external_id_val))
                        .select(kyc_case::id),
                ),
            )
            .into_boxed();

        if let Some(v) = expected_version_val {
            query = query.filter(kyc_submission::version.eq(v));
        }

        query
            .set(update)
            .get_result::<db::KycSubmissionRow>(&mut conn)
            .await
            .optional()
            .map_err(|e| backend_core::Error::Diesel(e))
    }
}
