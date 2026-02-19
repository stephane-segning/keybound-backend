use super::BackendApi;
use aws_sdk_s3::types::ServerSideEncryption;
use axum_extra::extract::CookieJar;
use backend_auth::JwtToken;
use backend_core::Error;
use backend_model::db;
use backend_repository::{
    KycRepo, KycStepCreateInput, MagicChallengeCreateInput, OtpChallengeCreateInput,
    UploadCompleteInput, UploadIntentCreateInput,
};
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
use gen_oas_server_bff::types::Object;
use headers::Host;
use http::Method;
use rand::random;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::time::Duration as StdDuration;

const OTP_RATE_LIMIT_WINDOW_MINUTES: i64 = 10;
const OTP_RATE_LIMIT_MAX_ISSUES: i64 = 5;
const OTP_MAX_TRIES: i32 = 5;

const MAGIC_RATE_LIMIT_WINDOW_MINUTES: i64 = 10;
const MAGIC_RATE_LIMIT_MAX_ISSUES: i64 = 5;

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
        ensure_user_match(claims, &body.user_id)?;

        let (session, step_ids) = self
            .state
            .kyc
            .start_or_resume_session(&body.user_id)
            .await?;

        Ok(InternalStartSessionResponse::Status201_Session(
            models::KycSessionInternal::new(
                session.id,
                session.user_id,
                parse_session_status(&session.status),
                step_ids,
                session.updated_at,
            ),
        ))
    }

    async fn internal_create_step(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::CreateStepRequest,
    ) -> Result<InternalCreateStepResponse, Error> {
        ensure_user_match(claims, &body.user_id)?;

        let row = self
            .state
            .kyc
            .create_step(KycStepCreateInput {
                session_id: body.session_id.clone(),
                user_id: body.user_id.clone(),
                step_type: body.r_type.to_string(),
                policy: map_objects_to_json(body.policy.as_ref()),
            })
            .await?;

        Ok(InternalCreateStepResponse::Status201_StepCreated(
            step_from_row(row)?,
        ))
    }

    async fn internal_get_step(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        path_params: &models::InternalGetStepPathParams,
    ) -> Result<InternalGetStepResponse, Error> {
        let user_id = Self::require_user_id(claims)?;
        let step = load_owned_step(self, &path_params.step_id, &user_id).await?;

        Ok(InternalGetStepResponse::Status200_Step(step_from_row(
            step,
        )?))
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
        let user_id = Self::require_user_id(claims)?;
        let step = load_owned_step(self, &body.step_id, &user_id).await?;
        ensure_step_type(&step, models::StepType::Email)?;

        let now = Utc::now();
        let challenge_count = self
            .state
            .kyc
            .count_recent_magic_challenges(
                &step.id,
                now - Duration::minutes(MAGIC_RATE_LIMIT_WINDOW_MINUTES),
            )
            .await?;

        if challenge_count >= MAGIC_RATE_LIMIT_MAX_ISSUES {
            return Err(rate_limited_error(
                "MAGIC_RATE_LIMITED",
                "Too many magic-link issuance requests",
            ));
        }

        let ttl_seconds = body.ttl_seconds.unwrap_or(300);
        let expires_at = now + Duration::seconds(i64::from(ttl_seconds));
        let secret = generate_magic_secret();
        let secret_hash = hash_secret(&secret)?;

        let challenge = self
            .state
            .kyc
            .create_magic_challenge(MagicChallengeCreateInput {
                step_id: step.id,
                email: body.email.clone(),
                token_hash: secret_hash,
                expires_at,
            })
            .await?;

        tracing::info!(
            email = %body.email,
            token = %format!("{}.{}", challenge.token_ref, secret),
            "magic email token issued"
        );

        Ok(InternalIssueMagicEmailResponse::Status200_Challenge(
            models::MagicEmailChallengeInternal::new(challenge.token_ref, challenge.expires_at),
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
        let user_id = Self::require_user_id(claims)?;
        let step = load_owned_step(self, &body.step_id, &user_id).await?;
        ensure_step_type(&step, models::StepType::Phone)?;

        let now = Utc::now();
        let challenge_count = self
            .state
            .kyc
            .count_recent_otp_challenges(
                &step.id,
                now - Duration::minutes(OTP_RATE_LIMIT_WINDOW_MINUTES),
            )
            .await?;

        if challenge_count >= OTP_RATE_LIMIT_MAX_ISSUES {
            return Err(rate_limited_error(
                "OTP_RATE_LIMITED",
                "Too many OTP issuance requests",
            ));
        }

        let ttl_seconds = body.ttl_seconds.unwrap_or(300);
        let expires_at = now + Duration::seconds(i64::from(ttl_seconds));
        let otp = generate_otp_code();
        let otp_hash = hash_secret(&otp)?;
        let channel = body
            .channel
            .unwrap_or(models::IssueOtpRequestChannel::Sms)
            .to_string();

        let challenge = self
            .state
            .kyc
            .create_otp_challenge(OtpChallengeCreateInput {
                step_id: step.id,
                msisdn: body.msisdn.clone(),
                channel,
                otp_hash,
                expires_at,
                tries_left: OTP_MAX_TRIES,
            })
            .await?;

        self.state
            .sms_provider
            .send_otp(&body.msisdn, &otp)
            .await
            .map_err(|err| Error::internal("SMS_SEND_FAILED", err.to_string()))?;

        Ok(InternalIssueOtpResponse::Status200_Challenge(
            models::OtpChallengeInternal::new(
                challenge.otp_ref,
                challenge.expires_at,
                u32::try_from(challenge.tries_left).unwrap_or(0),
            ),
        ))
    }

    async fn internal_verify_magic_email(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::VerifyMagicEmailRequest,
    ) -> Result<InternalVerifyMagicEmailResponse, Error> {
        let user_id = Self::require_user_id(claims)?;

        let Some((token_ref, secret)) = body.token.split_once('.') else {
            return Ok(InternalVerifyMagicEmailResponse::Status200_Outcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::Invalid,
                    models::KycStatus::Failed,
                ),
            ));
        };

        let challenge = self.state.kyc.get_magic_challenge(token_ref).await?;
        let Some(challenge) = challenge else {
            return Ok(InternalVerifyMagicEmailResponse::Status200_Outcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::Invalid,
                    models::KycStatus::Failed,
                ),
            ));
        };

        let step = load_owned_step(self, &challenge.step_id, &user_id).await?;
        ensure_step_type(&step, models::StepType::Email)?;

        let recent_count = self
            .state
            .kyc
            .count_recent_magic_challenges(
                &step.id,
                Utc::now() - Duration::minutes(MAGIC_RATE_LIMIT_WINDOW_MINUTES),
            )
            .await?;
        if recent_count >= MAGIC_RATE_LIMIT_MAX_ISSUES {
            return Ok(InternalVerifyMagicEmailResponse::Status200_Outcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::RateLimited,
                    parse_step_status(&step.status),
                ),
            ));
        }

        if challenge.expires_at < Utc::now() {
            return Ok(InternalVerifyMagicEmailResponse::Status200_Outcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::Expired,
                    parse_step_status(&step.status),
                ),
            ));
        }

        if verify_secret(secret, &challenge.token_hash)? {
            self.state.kyc.mark_magic_verified(token_ref).await?;
            self.state
                .kyc
                .update_step_status(&step.id, "VERIFIED")
                .await?;

            return Ok(InternalVerifyMagicEmailResponse::Status200_Outcome(
                models::VerifyOutcome::new(
                    true,
                    models::VerifyOutcomeReason::Verified,
                    models::KycStatus::Verified,
                ),
            ));
        }

        Ok(InternalVerifyMagicEmailResponse::Status200_Outcome(
            models::VerifyOutcome::new(
                false,
                models::VerifyOutcomeReason::Invalid,
                parse_step_status(&step.status),
            ),
        ))
    }

    async fn internal_verify_otp(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::VerifyOtpInternalRequest,
    ) -> Result<InternalVerifyOtpResponse, Error> {
        let user_id = Self::require_user_id(claims)?;
        let step = load_owned_step(self, &body.step_id, &user_id).await?;
        ensure_step_type(&step, models::StepType::Phone)?;

        let recent_count = self
            .state
            .kyc
            .count_recent_otp_challenges(
                &step.id,
                Utc::now() - Duration::minutes(OTP_RATE_LIMIT_WINDOW_MINUTES),
            )
            .await?;
        if recent_count >= OTP_RATE_LIMIT_MAX_ISSUES {
            return Ok(InternalVerifyOtpResponse::Status200_VerificationOutcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::RateLimited,
                    parse_step_status(&step.status),
                ),
            ));
        }

        let challenge = self
            .state
            .kyc
            .get_otp_challenge(&body.step_id, &body.otp_ref)
            .await?;

        let Some(challenge) = challenge else {
            return Ok(InternalVerifyOtpResponse::Status200_VerificationOutcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::Invalid,
                    parse_step_status(&step.status),
                ),
            ));
        };

        if challenge.expires_at < Utc::now() {
            return Ok(InternalVerifyOtpResponse::Status200_VerificationOutcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::Expired,
                    parse_step_status(&step.status),
                ),
            ));
        }

        if challenge.tries_left <= 0 {
            self.state
                .kyc
                .update_step_status(&step.id, "FAILED")
                .await?;
            return Ok(InternalVerifyOtpResponse::Status200_VerificationOutcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::Locked,
                    models::KycStatus::Failed,
                ),
            ));
        }

        if verify_secret(&body.code, &challenge.otp_hash)? {
            self.state
                .kyc
                .mark_otp_verified(&body.step_id, &body.otp_ref)
                .await?;
            self.state
                .kyc
                .update_step_status(&step.id, "VERIFIED")
                .await?;

            return Ok(InternalVerifyOtpResponse::Status200_VerificationOutcome(
                models::VerifyOutcome::new(
                    true,
                    models::VerifyOutcomeReason::Verified,
                    models::KycStatus::Verified,
                ),
            ));
        }

        let remaining = self
            .state
            .kyc
            .decrement_otp_tries(&body.step_id, &body.otp_ref)
            .await?;

        if remaining <= 0 {
            self.state
                .kyc
                .update_step_status(&step.id, "FAILED")
                .await?;
            return Ok(InternalVerifyOtpResponse::Status200_VerificationOutcome(
                models::VerifyOutcome::new(
                    false,
                    models::VerifyOutcomeReason::Locked,
                    models::KycStatus::Failed,
                ),
            ));
        }

        Ok(InternalVerifyOtpResponse::Status200_VerificationOutcome(
            models::VerifyOutcome::new(
                false,
                models::VerifyOutcomeReason::Invalid,
                parse_step_status(&step.status),
            ),
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
        let user_id = Self::require_user_id(claims)?;

        let completed = self
            .state
            .kyc
            .complete_upload_and_register_evidence(UploadCompleteInput {
                upload_id: body.upload_id.clone(),
                user_id,
                bucket: body.bucket.clone(),
                object_key: body.object_key.clone(),
                etag: body.etag.clone(),
                computed_sha256: body.computed_sha256.clone(),
            })
            .await?;

        if completed.moved_to_pending_review {
            tracing::info!(
                step_id = %completed.evidence.step_id,
                "identity step moved to pending review"
            );
        }

        Ok(
            InternalCompleteUploadResponse::Status200_EvidenceRegistered(models::EvidenceRef {
                evidence_id: completed.evidence.evidence_id,
                step_id: completed.evidence.step_id,
                asset_type: completed.evidence.asset_type,
                sha256: completed.evidence.sha256,
                created_at: completed.evidence.created_at,
            }),
        )
    }

    async fn internal_presign_upload(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::InternalPresignRequest,
    ) -> Result<InternalPresignUploadResponse, Error> {
        let user_id = Self::require_user_id(claims)?;
        ensure_user_match(claims, &body.user_id)?;
        let step = load_owned_step(self, &body.step_id, &user_id).await?;
        ensure_step_type(&step, models::StepType::Identity)?;

        let s3_config = self
            .state
            .config
            .s3
            .as_ref()
            .ok_or_else(|| Error::internal("S3_CONFIG_MISSING", "S3 is not configured"))?;

        let bucket = s3_config.bucket.clone();
        let object_key = format!(
            "{}/{}/{}",
            body.user_id,
            body.step_id,
            backend_id::kyc_upload_id()?
        );

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

        let presign_ttl = s3_config.presign_ttl_seconds;
        let presign_config = aws_sdk_s3::presigning::PresigningConfig::expires_in(
            StdDuration::from_secs(presign_ttl),
        )
        .map_err(|err| Error::internal("PRESIGN_CONFIG_ERROR", err.to_string()))?;

        let presigned = builder
            .content_type(body.mime.clone())
            .presigned(presign_config)
            .await
            .map_err(|err| Error::s3(err.to_string()))?;

        let upload = self
            .state
            .kyc
            .create_upload_intent(UploadIntentCreateInput {
                step_id: body.step_id.clone(),
                user_id: body.user_id.clone(),
                purpose: body.purpose.to_string(),
                asset_type: body.asset_type.to_string(),
                mime: body.mime.clone(),
                size_bytes: i64::from(body.size_bytes),
                bucket: bucket.clone(),
                object_key: object_key.clone(),
                method: models::UploadMethod::Put.to_string(),
                url: presigned.uri().to_string(),
                headers: serde_json::to_value(&headers)
                    .map_err(|err| Error::internal("JSON_SERIALIZATION", err.to_string()))?,
                multipart: None,
                expires_at: Utc::now() + Duration::seconds(presign_ttl as i64),
            })
            .await?;

        Ok(InternalPresignUploadResponse::Status200_PresignResponse(
            models::PresignUploadResponseInternal {
                upload_id: upload.upload_id,
                bucket,
                object_key,
                method: models::UploadMethod::Put,
                url: Some(upload.url),
                headers: Some(headers),
                multipart: None,
                expires_at: upload.expires_at,
            },
        ))
    }
}

fn ensure_user_match(claims: &JwtToken, body_user_id: &str) -> Result<(), Error> {
    let claims_user_id = claims.user_id();
    if claims_user_id != body_user_id {
        return Err(Error::unauthorized(
            "Authenticated user does not match request userId",
        ));
    }
    Ok(())
}

async fn load_owned_step(
    api: &BackendApi,
    step_id: &str,
    user_id: &str,
) -> Result<db::KycStepRow, Error> {
    let step = api.state.kyc.get_step(step_id).await?;
    let Some(step) = step else {
        return Err(Error::not_found("STEP_NOT_FOUND", "Step not found"));
    };

    if step.user_id != user_id {
        return Err(Error::unauthorized(
            "Step does not belong to authenticated user",
        ));
    }

    Ok(step)
}

fn ensure_step_type(step: &db::KycStepRow, expected: models::StepType) -> Result<(), Error> {
    if step.step_type == expected.to_string() {
        return Ok(());
    }
    Err(Error::bad_request(
        "INVALID_STEP_TYPE",
        format!("Expected step type {}, got {}", expected, step.step_type),
    ))
}

fn parse_session_status(status: &str) -> models::KycSessionInternalStatus {
    status
        .parse::<models::KycSessionInternalStatus>()
        .unwrap_or(models::KycSessionInternalStatus::Open)
}

fn parse_step_status(status: &str) -> models::KycStatus {
    status
        .parse::<models::KycStatus>()
        .unwrap_or(models::KycStatus::InProgress)
}

fn parse_step_type(step_type: &str) -> Result<models::StepType, Error> {
    step_type.parse::<models::StepType>().map_err(|_| {
        Error::internal(
            "INVALID_STEP_TYPE",
            format!("Unsupported step type stored in database: {step_type}"),
        )
    })
}

fn step_from_row(row: db::KycStepRow) -> Result<models::KycStepInternal, Error> {
    Ok(models::KycStepInternal {
        id: row.id,
        session_id: row.session_id,
        user_id: row.user_id,
        r_type: parse_step_type(&row.step_type)?,
        status: parse_step_status(&row.status),
        data: json_to_object_map(&row.data),
        policy: json_to_object_map(&row.policy),
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

fn json_to_object_map(value: &Value) -> Option<HashMap<String, Object>> {
    let Value::Object(entries) = value else {
        return None;
    };

    Some(
        entries
            .iter()
            .map(|(key, value)| (key.clone(), Object(value.clone())))
            .collect(),
    )
}

fn map_objects_to_json(value: Option<&HashMap<String, Object>>) -> Value {
    let Some(value) = value else {
        return Value::Object(Map::new());
    };

    Value::Object(
        value
            .iter()
            .map(|(key, value)| (key.clone(), value.0.clone()))
            .collect(),
    )
}

fn hash_secret(secret: &str) -> Result<String, Error> {
    use argon2::password_hash::rand_core::OsRng;
    use argon2::password_hash::{PasswordHasher, SaltString};

    let salt = SaltString::generate(&mut OsRng);
    argon2::Argon2::default()
        .hash_password(secret.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|err| Error::internal("ARGON2_HASH_FAILED", err.to_string()))
}

fn verify_secret(plain: &str, hash: &str) -> Result<bool, Error> {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};

    let parsed = PasswordHash::new(hash)
        .map_err(|err| Error::internal("ARGON2_HASH_INVALID", err.to_string()))?;

    Ok(argon2::Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .is_ok())
}

fn rate_limited_error(key: &'static str, message: &str) -> Error {
    Error::Http {
        error_key: key,
        status_code: 429,
        message: message.to_owned(),
        context: None,
    }
}

fn generate_otp_code() -> String {
    format!("{:06}", random::<u32>() % 1_000_000)
}

fn generate_magic_secret() -> String {
    format!("{:032x}", random::<u128>())
}
