use super::super::BackendApi;
use super::shared::{ensure_user_match, normalized_user_id, split_step_id, user_id_matches};
use crate::file_storage::EncryptionMode;
use backend_auth::JwtToken;
use backend_core::{Error, StorageType};
use chrono::{Duration, Utc};
use gen_oas_server_bff::apis::uploads::{
    InternalCompleteUploadResponse, InternalPresignUploadResponse,
};
use gen_oas_server_bff::models;
use tracing::instrument;

#[backend_core::async_trait]
pub(super) trait UploadFlow {
    async fn presign_upload_flow(
        &self,
        claims: &JwtToken,
        body: &models::InternalPresignRequest,
    ) -> Result<InternalPresignUploadResponse, Error>;

    async fn complete_upload_flow(
        &self,
        claims: &JwtToken,
        body: &models::InternalCompleteUploadRequest,
    ) -> Result<InternalCompleteUploadResponse, Error>;
}

#[backend_core::async_trait]
impl UploadFlow for BackendApi {
    #[instrument(skip(self))]
    async fn presign_upload_flow(
        &self,
        claims: &JwtToken,
        body: &models::InternalPresignRequest,
    ) -> Result<InternalPresignUploadResponse, Error> {
        ensure_user_match(claims, &body.user_id)?;
        let user_id = normalized_user_id(&body.user_id);

        let (step_session_id, _step_type) = split_step_id(&body.step_id)
            .ok_or_else(|| Error::bad_request("INVALID_STEP_ID", "Step id format is invalid"))?;
        if body.session_id != step_session_id {
            return Err(Error::bad_request(
                "INVALID_STEP",
                "stepId must belong to provided sessionId",
            ));
        }

        let session = self
            .state
            .sm
            .get_instance(&body.session_id)
            .await?
            .ok_or_else(|| Error::not_found("SESSION_NOT_FOUND", "Session not found"))?;
        if !user_id_matches(session.user_id.as_deref(), &user_id) {
            return Err(Error::unauthorized(
                "Session does not belong to authenticated user",
            ));
        }

        let (bucket, presign_ttl_seconds, is_minio_storage) = match self
            .state
            .config
            .storage
            .as_ref()
            .map(|storage| storage.r#type)
        {
            Some(StorageType::Minio) => {
                let minio = self
                    .state
                    .config
                    .storage
                    .as_ref()
                    .and_then(|storage| storage.minio.as_ref())
                    .ok_or_else(|| {
                        Error::internal("MINIO_NOT_CONFIGURED", "MinIO storage is not configured")
                    })?;
                (minio.bucket.clone(), minio.presign_ttl_seconds, true)
            }
            _ => {
                let s3 =
                    self.state.config.s3.as_ref().ok_or_else(|| {
                        Error::internal("S3_NOT_CONFIGURED", "S3 is not configured")
                    })?;
                (s3.bucket.clone(), s3.presign_ttl_seconds, false)
            }
        };

        let upload_id = backend_id::kyc_upload_id()?;
        let object_key = format!(
            "uploads/{}/{}/{}/{}",
            user_id, body.session_id, body.step_id, upload_id
        );

        let encryption = if is_minio_storage {
            EncryptionMode::None
        } else {
            match body.encryption.as_ref().map(|encryption| encryption.mode) {
                Some(models::SseMode::SseS3) => EncryptionMode::S3,
                Some(models::SseMode::SseKms) => EncryptionMode::Kms,
                _ => EncryptionMode::S3,
            }
        };

        let presigned = self
            .state
            .minio
            .upload_presigned(
                &bucket,
                &object_key,
                &body.mime,
                encryption,
                std::time::Duration::from_secs(presign_ttl_seconds),
            )
            .await?;

        Ok(
            InternalPresignUploadResponse::Status200_PresignedUploadResponse(
                models::PresignUploadResponseInternal {
                    upload_id,
                    bucket,
                    object_key,
                    method: models::UploadMethod::Put,
                    url: Some(presigned.url),
                    headers: Some(presigned.headers),
                    multipart: None,
                    expires_at: Utc::now()
                        + Duration::seconds(i64::try_from(presign_ttl_seconds).unwrap_or(i64::MAX)),
                },
            ),
        )
    }

    #[instrument(skip(self))]
    async fn complete_upload_flow(
        &self,
        claims: &JwtToken,
        body: &models::InternalCompleteUploadRequest,
    ) -> Result<InternalCompleteUploadResponse, Error> {
        let user_id = BackendApi::require_user_id(claims)?;

        let (step_session_id, _step_type) = split_step_id(&body.step_id)
            .ok_or_else(|| Error::bad_request("INVALID_STEP_ID", "Step id format is invalid"))?;
        if body.session_id != step_session_id {
            return Err(Error::bad_request(
                "INVALID_STEP",
                "stepId must belong to provided sessionId",
            ));
        }

        let session = self
            .state
            .sm
            .get_instance(&body.session_id)
            .await?
            .ok_or_else(|| Error::not_found("SESSION_NOT_FOUND", "Session not found"))?;
        if !user_id_matches(session.user_id.as_deref(), &user_id) {
            return Err(Error::unauthorized(
                "Session does not belong to authenticated user",
            ));
        }

        self.state
            .minio
            .head_object(&body.bucket, &body.object_key)
            .await
            .map_err(|_| Error::not_found("UPLOAD_NOT_FOUND", "Uploaded object not found"))?;

        Ok(
            InternalCompleteUploadResponse::Status200_EvidenceRegistered(models::EvidenceRef::new(
                backend_id::kyc_evidence_id()?,
                body.session_id.clone(),
                body.step_id.clone(),
                "EVIDENCE".to_owned(),
                Utc::now(),
            )),
        )
    }
}
