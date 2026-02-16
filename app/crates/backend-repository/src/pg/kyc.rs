use crate::traits::*;
use backend_model::{db, staff as staff_map};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use sqlx_data::{IntoParams, Serial, dml, repo};

#[repo]
pub trait PgKycRepo {
    #[dml(file = "queries/bff/list_kyc_documents.sql", unchecked)]
    async fn list_kyc_documents_db(
        &self,
        external_id: String,
        params: impl IntoParams,
    ) -> sqlx_data::Result<Serial<db::KycDocumentRow>>;

    #[dml(file = "queries/kyc/get_document.sql", unchecked)]
    async fn get_kyc_document_db(
        &self,
        external_id: String,
        id: String,
    ) -> sqlx_data::Result<Option<db::KycDocumentRow>>;

    #[dml(file = "queries/staff/list_kyc_submissions.sql", unchecked)]
    async fn list_kyc_submissions_db(
        &self,
        params: impl IntoParams,
    ) -> sqlx_data::Result<Serial<db::KycProfileRow>>;

    #[dml(file = "queries/kyc/ensure_profile.sql", unchecked)]
    async fn ensure_kyc_profile_db(
        &self,
        external_id: String,
    ) -> sqlx_data::Result<sqlx_data::QueryResult>;

    #[dml(file = "queries/kyc/insert_document_intent.sql", unchecked)]
    async fn insert_kyc_document_intent_db(
        &self,
        id: String,
        external_id: String,
        document_type: String,
        file_name: String,
        mime_type: String,
        content_length: i64,
        s3_bucket: String,
        s3_key: String,
        presigned_expires_at: DateTime<Utc>,
    ) -> sqlx_data::Result<db::KycDocumentRow>;

    #[dml(file = "queries/kyc/get_profile.sql", unchecked)]
    async fn get_kyc_profile_db(
        &self,
        external_id: String,
    ) -> sqlx_data::Result<Option<db::KycProfileRow>>;

    #[dml(file = "queries/kyc/get_tier.sql", unchecked)]
    async fn get_kyc_tier_db(&self, external_id: String) -> sqlx_data::Result<Option<i32>>;

    #[dml(file = "queries/kyc/update_approved.sql", unchecked)]
    async fn update_kyc_approved_db(
        &self,
        external_id: String,
        new_tier: i32,
        notes: Option<String>,
    ) -> sqlx_data::Result<sqlx_data::QueryResult>;

    #[dml(file = "queries/kyc/update_rejected.sql", unchecked)]
    async fn update_kyc_rejected_db(
        &self,
        external_id: String,
        reason: String,
        notes: Option<String>,
    ) -> sqlx_data::Result<sqlx_data::QueryResult>;

    #[dml(file = "queries/kyc/update_request_info.sql", unchecked)]
    async fn update_kyc_request_info_db(
        &self,
        external_id: String,
        message: String,
    ) -> sqlx_data::Result<sqlx_data::QueryResult>;

    #[dml(file = "queries/kyc/patch_information.sql", unchecked)]
    async fn patch_kyc_information_db(
        &self,
        external_id: String,
        expected_version: Option<i32>,
        first_name: Option<String>,
        last_name: Option<String>,
        email: Option<String>,
        phone_number: Option<String>,
        date_of_birth: Option<String>,
        nationality: Option<String>,
    ) -> sqlx_data::Result<Option<db::KycProfileRow>>;

    #[dml(file = "queries/kyc/submit_profile.sql", unchecked)]
    async fn submit_kyc_profile_db(
        &self,
        submission_id: String,
        external_id: String,
    ) -> sqlx_data::Result<sqlx_data::QueryResult>;
}

#[derive(Clone)]
pub struct KycRepository {
    pub(crate) pool: PgPool,
}

impl PgKycRepo for KycRepository {
    fn get_pool(&self) -> &sqlx_data::Pool {
        &self.pool
    }
}

impl KycRepo for KycRepository {
    async fn ensure_kyc_profile(&self, external_id: &str) -> RepoResult<()> {
        self.ensure_kyc_profile_db(external_id.to_owned()).await?;
        Ok(())
    }

    async fn insert_kyc_document_intent(
        &self,
        input: KycDocumentInsert,
    ) -> RepoResult<db::KycDocumentRow> {
        let id = backend_id::kyc_document_id()?;

        let row = self
            .insert_kyc_document_intent_db(
                id,
                input.external_id,
                input.document_type,
                input.file_name,
                input.mime_type,
                input.content_length,
                input.s3_bucket,
                input.s3_key,
                input.presigned_expires_at,
            )
            .await?;
        Ok(row)
    }

    async fn get_kyc_profile(&self, external_id: &str) -> RepoResult<Option<db::KycProfileRow>> {
        let row = self.get_kyc_profile_db(external_id.to_owned()).await?;
        Ok(row)
    }

    async fn list_kyc_documents(
        &self,
        external_id: String,
        params: impl IntoParams + Send,
    ) -> RepoResult<Serial<db::KycDocumentRow>> {
        let rows = self.list_kyc_documents_db(external_id, params).await?;
        Ok(rows)
    }

    async fn get_kyc_document(
        &self,
        external_id: &str,
        document_id: &str,
    ) -> RepoResult<Option<db::KycDocumentRow>> {
        let row = self
            .get_kyc_document_db(external_id.to_owned(), document_id.to_owned())
            .await?;
        Ok(row)
    }

    async fn get_kyc_tier(&self, external_id: &str) -> RepoResult<Option<i32>> {
        let tier = self.get_kyc_tier_db(external_id.to_owned()).await?;
        Ok(tier)
    }

    async fn list_kyc_submissions(
        &self,
        params: impl IntoParams + Send,
    ) -> RepoResult<Serial<db::KycProfileRow>> {
        let rows = self.list_kyc_submissions_db(params).await?;
        Ok(rows)
    }

    async fn get_kyc_submission(&self, external_id: &str) -> RepoResult<Option<db::KycProfileRow>> {
        let row = self.get_kyc_profile_db(external_id.to_owned()).await?;
        Ok(row)
    }

    async fn update_kyc_approved(
        &self,
        external_id: &str,
        req: &staff_map::KycApprovalRequest,
    ) -> RepoResult<bool> {
        let res = self
            .update_kyc_approved_db(
                external_id.to_owned(),
                req.new_tier as i32,
                req.notes.clone(),
            )
            .await?;
        Ok(res.rows_affected() > 0)
    }

    async fn update_kyc_rejected(
        &self,
        external_id: &str,
        req: &staff_map::KycRejectionRequest,
    ) -> RepoResult<bool> {
        let res = self
            .update_kyc_rejected_db(
                external_id.to_owned(),
                req.reason.clone(),
                req.notes.clone(),
            )
            .await?;
        Ok(res.rows_affected() > 0)
    }

    async fn update_kyc_request_info(
        &self,
        external_id: &str,
        req: &staff_map::KycRequestInfoRequest,
    ) -> RepoResult<bool> {
        let res = self
            .update_kyc_request_info_db(external_id.to_owned(), req.message.clone())
            .await?;
        Ok(res.rows_affected() > 0)
    }

    async fn patch_kyc_profile(
        &self,
        external_id: &str,
        expected_version: Option<i32>,
        req: &backend_model::bff::KycInformationPatchRequest,
    ) -> RepoResult<Option<db::KycProfileRow>> {
        let row = self
            .patch_kyc_information_db(
                external_id.to_owned(),
                expected_version,
                req.first_name.clone(),
                req.last_name.clone(),
                req.email.clone(),
                req.phone_number.clone(),
                req.date_of_birth.clone(),
                req.nationality.clone(),
            )
            .await?;
        Ok(row)
    }

    async fn submit_kyc_profile(&self, submission_id: &str, external_id: &str) -> RepoResult<bool> {
        let res = self
            .submit_kyc_profile_db(submission_id.to_owned(), external_id.to_owned())
            .await?;
        Ok(res.rows_affected() > 0)
    }
}

impl KycRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}
