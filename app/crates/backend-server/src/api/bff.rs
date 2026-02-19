use super::BackendApi;
use aws_sdk_s3::types::ServerSideEncryption;
use axum_extra::extract::CookieJar;
use backend_auth::JwtToken;
use backend_core::Error;
use backend_id;
use chrono::{Duration, Utc};
use gen_oas_server_bff::apis::notifications::{
    InternalIssueMagicEmailResponse, InternalIssueOtpResponse, InternalVerifyMagicEmailResponse,
    InternalVerifyOtpResponse, Notifications,
};
use gen_oas_server_bff::apis::steps::{
    InternalCreateStepResponse, InternalGetStepResponse, InternalStartSessionResponse, Steps,
};
use gen_oas_server_bff::apis::uploads::{
    InternalCompleteUploadResponse, InternalPresignUploadResponse, Uploads,
};
use gen_oas_server_bff::models;
use headers::Host;
use http::Method;
use rand::random;
use std::collections::HashMap;
use std::time::Duration as StdDuration;

#[backend_core::async_trait]
impl Steps<Error> for BackendApi {
    type Claims = JwtToken;

    async fn internal_start_session(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::InternalStartSessionRequest,
    ) -> Result<InternalStartSessionResponse, Error> {
        let _user_id = Self::require_user_id(claims)?;
        let session_id = backend_id::prefixed("bffsess")?;
        let session = models::KycSessionInternal::new(
            session_id,
            body.user_id.clone(),
            models::KycSessionInternalStatus::Open,
            vec![],
            Utc::now(),
        );
        Ok(InternalStartSessionResponse::Status201_Session(session))
    }

    async fn internal_create_step(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::CreateStepRequest,
    ) -> Result<InternalCreateStepResponse, Error> {
        let _user_id = Self::require_user_id(claims)?;
        let step_id = backend_id::prefixed("bffstep")?;
        let now = Utc::now();
        let step = models::KycStepInternal {
            id: step_id,
            session_id: body.session_id.clone(),
            user_id: body.user_id.clone(),
            r_type: body.r_type.clone(),
            status: models::KycStatus::InProgress,
            data: None,
            policy: body.policy.clone(),
            created_at: now,
            updated_at: now,
        };
        Ok(InternalCreateStepResponse::Status201_StepCreated(step))
    }

    async fn internal_get_step(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        path_params: &models::InternalGetStepPathParams,
    ) -> Result<InternalGetStepResponse, Error> {
        let _user_id = Self::require_user_id(claims)?;
        let now = Utc::now();
        let step = models::KycStepInternal {
            id: path_params.step_id.clone(),
            session_id: format!("session_{}", path_params.step_id),
            user_id: claims.user_id().to_owned(),
            r_type: models::StepType::Phone,
            status: models::KycStatus::InProgress,
            data: None,
            policy: None,
            created_at: now,
            updated_at: now,
        };
        Ok(InternalGetStepResponse::Status200_Step(step))
    }
}

#[backend_core::async_trait]
impl Notifications<Error> for BackendApi {
    type Claims = JwtToken;

    async fn internal_issue_magic_email(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::IssueMagicEmailRequest,
    ) -> Result<InternalIssueMagicEmailResponse, Error> {
        let _user_id = Self::require_user_id(claims)?;
        let token_ref = backend_id::prefixed("magic")?;
        let token = Self::generate_magic_token();
        let expires_at = Utc::now() + Duration::seconds(body.ttl_seconds.unwrap_or(300) as i64);
        tracing::info!(email = %body.email, token = %token, "magic email token issued");
        let challenge = models::MagicEmailChallengeInternal {
            token_ref,
            expires_at,
        };
        Ok(InternalIssueMagicEmailResponse::Status200_Challenge(
            challenge,
        ))
    }

    async fn internal_issue_otp(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::IssueOtpRequest,
    ) -> Result<InternalIssueOtpResponse, Error> {
        let _user_id = Self::require_user_id(claims)?;
        let otp_ref = backend_id::prefixed("otp")?;
        let ttl = body.ttl_seconds.unwrap_or(300);
        let expires_at = Utc::now() + Duration::seconds(ttl as i64);
        let otp = Self::generate_otp_code();
        tracing::info!(msisdn = %body.msisdn, otp = %otp, "issuing otp");
        self.state
            .sms_provider
            .send_otp(&body.msisdn, &otp)
            .await
            .map_err(|err| Error::internal("SMS_SEND_FAILED", err.to_string()))?;
        let challenge = models::OtpChallengeInternal {
            otp_ref,
            provider_message_id: None,
            expires_at,
            tries_left: 5,
        };
        Ok(InternalIssueOtpResponse::Status200_Challenge(challenge))
    }

    async fn internal_verify_magic_email(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        _body: &models::VerifyMagicEmailRequest,
    ) -> Result<InternalVerifyMagicEmailResponse, Error> {
        let _user_id = Self::require_user_id(claims)?;
        let outcome = models::VerifyOutcome {
            ok: true,
            reason: models::VerifyOutcomeReason::Verified,
            step_status: models::KycStatus::Verified,
        };
        Ok(InternalVerifyMagicEmailResponse::Status200_Outcome(outcome))
    }

    async fn internal_verify_otp(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        _body: &models::VerifyOtpInternalRequest,
    ) -> Result<InternalVerifyOtpResponse, Error> {
        let _user_id = Self::require_user_id(claims)?;
        let outcome = models::VerifyOutcome {
            ok: true,
            reason: models::VerifyOutcomeReason::Verified,
            step_status: models::KycStatus::Verified,
        };
        Ok(InternalVerifyOtpResponse::Status200_VerificationOutcome(
            outcome,
        ))
    }
}

#[backend_core::async_trait]
impl Uploads<Error> for BackendApi {
    type Claims = JwtToken;

    async fn internal_complete_upload(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::InternalCompleteUploadRequest,
    ) -> Result<InternalCompleteUploadResponse, Error> {
        let _user_id = Self::require_user_id(claims)?;
        let evidence = models::EvidenceRef {
            evidence_id: backend_id::prefixed("evidence")?,
            step_id: body.upload_id.clone(),
            asset_type: models::IdentityAssetType::IdFront.to_string(),
            sha256: Some(String::new()),
            created_at: Utc::now(),
        };
        Ok(InternalCompleteUploadResponse::Status200_EvidenceRegistered(evidence))
    }

    async fn internal_presign_upload(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::InternalPresignRequest,
    ) -> Result<InternalPresignUploadResponse, Error> {
        let _user_id = Self::require_user_id(claims)?;
        let s3_config = self
            .state
            .config
            .s3
            .as_ref()
            .ok_or_else(|| Error::internal("S3_CONFIG_MISSING", "S3 is not configured"))?;
        let bucket = s3_config.bucket.clone();
        let object_key = format!("{}/{}", body.user_id, backend_id::kyc_document_id()?);
        let mut builder = self.state.s3.put_object().bucket(&bucket).key(&object_key);
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_owned(), body.mime.clone());
        if let Some(enc) = &body.encryption {
            match enc.mode {
                models::SseMode::SseS3 => {
                    builder = builder.server_side_encryption(ServerSideEncryption::Aes256);
                    headers.insert(
                        "x-amz-server-side-encryption".to_owned(),
                        "AES256".to_owned(),
                    );
                }
                models::SseMode::SseKms => {
                    builder = builder.server_side_encryption(ServerSideEncryption::AwsKms);
                    headers.insert(
                        "x-amz-server-side-encryption".to_owned(),
                        "aws:kms".to_owned(),
                    );
                }
                models::SseMode::SseC => {}
            }
        }
        let presign_config = aws_sdk_s3::presigning::PresigningConfig::expires_in(
            StdDuration::from_secs(s3_config.presign_ttl_seconds),
        )
        .map_err(|err| Error::internal("PRESIGN_CONFIG_ERROR", err.to_string()))?;
        let presigned = builder
            .content_type(body.mime.clone())
            .presigned(presign_config)
            .await
            .map_err(|err| Error::s3(err.to_string()))?;
        let expires_at = Utc::now() + Duration::seconds(s3_config.presign_ttl_seconds as i64);
        let response = models::PresignUploadResponseInternal {
            upload_id: backend_id::prefixed("upload")?,
            bucket: bucket.clone(),
            object_key: object_key.clone(),
            method: models::UploadMethod::Put,
            url: Some(presigned.uri().to_string()),
            headers: Some(headers),
            multipart: None,
            expires_at,
        };
        Ok(InternalPresignUploadResponse::Status200_PresignResponse(
            response,
        ))
    }
}

impl BackendApi {
    fn generate_otp_code() -> String {
        format!("{:06}", random::<u32>() % 1_000_000)
    }

    fn generate_magic_token() -> String {
        format!("{:08x}", random::<u32>())
    }
}
