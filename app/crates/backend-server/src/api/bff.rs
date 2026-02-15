use super::{BackendApi, repo_err};
use backend_auth::ServiceContext;
use backend_model::bff::{
    KycDocumentUploadRequest, KycInformationPatchRequest, KycInformationResponseDto,
    KycStatusResponseDto,
};
use backend_repository::{KycDocumentInsert, KycRepo};
use gen_oas_server_bff::apis::kyc::{
    ApiKycCasesMineGetResponse, ApiKycCasesMinePatchResponse,
    ApiKycCasesMineSubmissionPostResponse, ApiLimitsGetResponse, Kyc,
};
use gen_oas_server_bff::apis::kyc_documents::{ApiKycCasesMineDocumentsPostResponse, KycDocuments};
use gen_oas_server_bff::models;
use http::Method;

#[backend_core::async_trait]
impl Kyc for BackendApi {
    type Claims = ServiceContext;

    async fn api_kyc_cases_mine_get(
        &self,
        _method: &Method,
        _host: &headers::Host,
        _cookies: &axum_extra::extract::CookieJar,
        claims: &Self::Claims,
    ) -> Result<ApiKycCasesMineGetResponse, ()> {
        let user_id = Self::require_user_id(claims).map_err(|_| ())?;

        let profile = self
            .state
            .kyc
            .get_kyc_profile(&user_id)
            .await
            .map_err(repo_err)
            .map_err(|_| ())?;

        match profile {
            Some(p) => {
                let dto = KycStatusResponseDto {
                    kyc_tier: Some(p.kyc_tier),
                    kyc_status: Some(p.kyc_status),
                    documents: None,
                    required_documents: None,
                    missing_documents: None,
                    page: None,
                    page_size: None,
                    total_documents: None,
                };
                Ok(ApiKycCasesMineGetResponse::Status200_KYCCaseDetails(
                    dto.into(),
                ))
            }
            None => Ok(ApiKycCasesMineGetResponse::Status404_KYCCaseNotFound),
        }
    }

    async fn api_kyc_cases_mine_patch(
        &self,
        _method: &Method,
        _host: &headers::Host,
        _cookies: &axum_extra::extract::CookieJar,
        claims: &Self::Claims,
        body: &models::KycCasePatchRequest,
    ) -> Result<ApiKycCasesMinePatchResponse, ()> {
        let user_id = Self::require_user_id(claims).map_err(|_| ())?;

        let req = KycInformationPatchRequest::from(body.clone());

        let profile = self
            .state
            .kyc
            .patch_kyc_information(&user_id, &req)
            .await
            .map_err(repo_err)
            .map_err(|_| ())?;

        match profile {
            Some(p) => {
                let dto = KycInformationResponseDto {
                    external_id: Some(p.external_id),
                    first_name: p.first_name,
                    last_name: p.last_name,
                    email: p.email,
                    phone_number: p.phone_number,
                    date_of_birth: p.date_of_birth,
                    nationality: p.nationality,
                    updated_at: Some(p.updated_at),
                };
                Ok(ApiKycCasesMinePatchResponse::Status200_KYCCaseUpdated(
                    dto.into(),
                ))
            }
            None => Ok(ApiKycCasesMinePatchResponse::Status404_KYCCaseNotFound),
        }
    }

    async fn api_kyc_cases_mine_submission_post(
        &self,
        _method: &Method,
        _host: &headers::Host,
        _cookies: &axum_extra::extract::CookieJar,
        claims: &Self::Claims,
    ) -> Result<ApiKycCasesMineSubmissionPostResponse, ()> {
        let user_id = Self::require_user_id(claims).map_err(|_| ())?;

        // For now, we just return the current profile as "submitted"
        let profile = self
            .state
            .kyc
            .get_kyc_profile(&user_id)
            .await
            .map_err(repo_err)
            .map_err(|_| ())?;

        match profile {
            Some(p) => {
                let dto = KycStatusResponseDto {
                    kyc_tier: Some(p.kyc_tier),
                    kyc_status: Some(p.kyc_status),
                    documents: None,
                    required_documents: None,
                    missing_documents: None,
                    page: None,
                    page_size: None,
                    total_documents: None,
                };
                Ok(ApiKycCasesMineSubmissionPostResponse::Status200_KYCCaseSubmitted(dto.into()))
            }
            None => Ok(ApiKycCasesMineSubmissionPostResponse::Status404_KYCCaseNotFound),
        }
    }

    async fn api_limits_get(
        &self,
        _method: &Method,
        _host: &headers::Host,
        _cookies: &axum_extra::extract::CookieJar,
        claims: &Self::Claims,
    ) -> Result<ApiLimitsGetResponse, ()> {
        let user_id = Self::require_user_id(claims).map_err(|_| ())?;

        let tier = self
            .state
            .kyc
            .get_kyc_tier(&user_id)
            .await
            .map_err(repo_err)
            .map_err(|_| ())?;

        match tier {
            Some(t) => {
                let limits = models::LimitsResponse {
                    kyc_tier: Some(t),
                    tier_name: Some(format!("Tier {t}")),
                    limits: Some(models::LimitsResponseLimitsDto {
                        daily_deposit_limit: Some(1000.0),
                        daily_withdrawal_limit: Some(1000.0),
                        per_transaction_limit: Some(500.0),
                        monthly_transaction_limit: Some(5000.0),
                    }),
                    usage: Some(models::LimitsResponseUsageDto {
                        daily_deposit_used: Some(0.0),
                        daily_withdrawal_used: Some(0.0),
                        monthly_used: Some(0.0),
                    }),
                    available: Some(models::LimitsResponseAvailableDto {
                        deposit_remaining: Some(1000.0),
                        withdrawal_remaining: Some(1000.0),
                    }),
                    allowed_payment_methods: Some(vec!["bank_transfer".to_owned()]),
                    restricted_features: Some(Vec::new()),
                    currency: Some("EUR".to_owned()),
                };
                Ok(ApiLimitsGetResponse::Status200_LimitsAndUsageDetails(
                    limits,
                ))
            }
            None => Ok(ApiLimitsGetResponse::Status404_CustomerNotFound),
        }
    }
}

#[backend_core::async_trait]
impl KycDocuments for BackendApi {
    type Claims = ServiceContext;

    async fn api_kyc_cases_mine_documents_post(
        &self,
        _method: &Method,
        _host: &headers::Host,
        _cookies: &axum_extra::extract::CookieJar,
        claims: &Self::Claims,
        body: &models::KycDocumentUploadRequest,
    ) -> Result<ApiKycCasesMineDocumentsPostResponse, ()> {
        let user_id = Self::require_user_id(claims).map_err(|_| ())?;

        let req = KycDocumentUploadRequest::from(body.clone());

        // Ensure profile exists
        self.state
            .kyc
            .ensure_kyc_profile(&user_id)
            .await
            .map_err(repo_err)
            .map_err(|_| ())?;

        let input = KycDocumentInsert {
            external_id: user_id,
            document_type: req.document_type,
            file_name: req.file_name,
            mime_type: req.mime_type,
            content_length: req.content_length,
            s3_bucket: "azamra-kyc".to_owned(), // Mocked
            s3_key: "temp/key".to_owned(),      // Mocked
            presigned_expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
        };

        let row = self
            .state
            .kyc
            .insert_kyc_document_intent(input)
            .await
            .map_err(repo_err)
            .map_err(|_| ())?;

        let resp = models::KycDocumentUploadResponse {
            document_id: Some(row.id),
            document_type: Some(row.document_type),
            status: Some(row.status),
            uploaded_at: row.uploaded_at,
            file_name: Some(row.file_name),
            mime_type: Some(row.mime_type),
            upload_url: Some("https://s3.example.com/upload".to_owned()), // Mocked
            upload_method: Some("PUT".to_owned()),
            upload_headers: None,
            expires_at: Some(row.presigned_expires_at),
            s3_bucket: Some(row.s3_bucket),
            s3_key: Some(row.s3_key),
        };

        Ok(ApiKycCasesMineDocumentsPostResponse::Status201_UploadURLCreatedSuccessfully(resp))
    }
}
