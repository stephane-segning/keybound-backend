use super::BackendApi;
use crate::worker;
use axum_extra::extract::CookieJar;
use backend_auth::ServiceContext;
use backend_core::Error;
use backend_model::staff::{
    KycApprovalRequest, KycDocumentDto, KycRejectionRequest, KycRequestInfoRequest,
    KycSubmissionDetailResponseDto, KycSubmissionSummaryDto, KycSubmissionsResponseDto,
};
use backend_repository::KycRepo;
use gen_oas_server_staff::apis::kyc_review::{
    ApiKycStaffSubmissionsGetResponse, ApiKycStaffSubmissionsUserIdApprovePostResponse,
    ApiKycStaffSubmissionsUserIdDocumentsDocumentIdDownloadUrlPostResponse,
    ApiKycStaffSubmissionsUserIdGetResponse, ApiKycStaffSubmissionsUserIdRejectPostResponse,
    ApiKycStaffSubmissionsUserIdRequestInfoPostResponse, KycReview,
};
use gen_oas_server_staff::models;
use headers::Host;
use http::Method;
use sqlx_data::ParamsBuilder;
use tracing::{info, warn};

#[backend_core::async_trait]
impl KycReview<Error> for BackendApi {
    type Claims = ServiceContext;

    async fn api_kyc_staff_submissions_get(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        query_params: &models::ApiKycStaffSubmissionsGetQueryParams,
    ) -> Result<ApiKycStaffSubmissionsGetResponse, Error> {
        let (page, limit) = Self::normalize_page_limit(query_params.page, query_params.limit);

        let rows = self
            .state
            .kyc
            .list_kyc_submissions(ParamsBuilder::default())
            .await?;

        let status_filter = query_params
            .status
            .as_ref()
            .map(|status| status.to_lowercase());
        let search_filter = query_params
            .search
            .as_ref()
            .map(|search| search.to_lowercase());

        let filtered_rows = rows
            .data
            .into_iter()
            .filter(|row| {
                let status_matches = status_filter
                    .as_ref()
                    .is_none_or(|status| row.kyc_status.eq_ignore_ascii_case(status));

                let search_matches = search_filter.as_ref().is_none_or(|search| {
                    row.first_name
                        .as_deref()
                        .is_some_and(|value| value.to_lowercase().contains(search))
                        || row
                            .last_name
                            .as_deref()
                            .is_some_and(|value| value.to_lowercase().contains(search))
                        || row
                            .email
                            .as_deref()
                            .is_some_and(|value| value.to_lowercase().contains(search))
                });

                status_matches && search_matches
            })
            .collect::<Vec<_>>();

        let total = i32::try_from(filtered_rows.len()).unwrap_or(i32::MAX);
        let start = usize::try_from((page - 1) * limit).unwrap_or(0);
        let end = start.saturating_add(usize::try_from(limit).unwrap_or(0));

        let items = filtered_rows
            .into_iter()
            .skip(start)
            .take(end.saturating_sub(start))
            .map(|row| {
                let dto = KycSubmissionSummaryDto::from(row);
                dto.into()
            })
            .collect::<Vec<_>>();

        let response = KycSubmissionsResponseDto {
            items: Some(items),
            total: Some(total),
            page: Some(page),
            page_size: Some(limit),
        };

        Ok(ApiKycStaffSubmissionsGetResponse::Status200_PageOfKYCSubmissions(response.into()))
    }

    async fn api_kyc_staff_submissions_user_id_approve_post(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        path_params: &models::ApiKycStaffSubmissionsUserIdApprovePostPathParams,
        body: &models::KycApprovalRequest,
    ) -> Result<ApiKycStaffSubmissionsUserIdApprovePostResponse, Error> {
        let req = KycApprovalRequest::from(body.clone());

        let updated = self
            .state
            .kyc
            .update_kyc_approved(&path_params.user_id, &req)
            .await?;

        if updated {
            info!(
                user_id = %path_params.user_id,
                new_tier = req.new_tier,
                notes = req.notes.as_deref(),
                "staff approved KYC, enqueuing fineract provisioning job"
            );

            if let Err(err) = worker::enqueue_fineract_provisioning(
                &self.state.config.redis.url,
                &path_params.user_id,
            )
            .await
            {
                warn!(
                    user_id = %path_params.user_id,
                    error = %err,
                    "failed to enqueue fineract provisioning job"
                );
            }

            Ok(ApiKycStaffSubmissionsUserIdApprovePostResponse::Status200_KYCApproved)
        } else {
            Ok(ApiKycStaffSubmissionsUserIdApprovePostResponse::Status400_ValidationFailed)
        }
    }

    async fn api_kyc_staff_submissions_user_id_get(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        path_params: &models::ApiKycStaffSubmissionsUserIdGetPathParams,
        query_params: &models::ApiKycStaffSubmissionsUserIdGetQueryParams,
    ) -> Result<ApiKycStaffSubmissionsUserIdGetResponse, Error> {
        let (page, limit) = Self::normalize_page_limit(query_params.page, query_params.limit);

        let profile = self
            .state
            .kyc
            .get_kyc_submission(&path_params.user_id)
            .await?;

        let Some(profile) = profile else {
            return Ok(ApiKycStaffSubmissionsUserIdGetResponse::Status404_SubmissionNotFound);
        };

        let documents = self
            .state
            .kyc
            .list_kyc_documents(path_params.user_id.clone(), ParamsBuilder::default())
            .await?;

        let total_documents = i32::try_from(documents.data.len()).unwrap_or(i32::MAX);
        let start = usize::try_from((page - 1) * limit).unwrap_or(0);
        let end = start.saturating_add(usize::try_from(limit).unwrap_or(0));

        let document_items = documents
            .data
            .into_iter()
            .skip(start)
            .take(end.saturating_sub(start))
            .map(|row| {
                let dto = KycDocumentDto::from(row);
                dto.into()
            })
            .collect::<Vec<_>>();

        let mut response = KycSubmissionDetailResponseDto::from_profile(profile);
        response.documents = Some(document_items);
        response.page = Some(page);
        response.page_size = Some(limit);
        response.total_documents = Some(total_documents);

        Ok(ApiKycStaffSubmissionsUserIdGetResponse::Status200_DetailedSubmission(response.into()))
    }

    async fn api_kyc_staff_submissions_user_id_reject_post(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        path_params: &models::ApiKycStaffSubmissionsUserIdRejectPostPathParams,
        body: &models::KycRejectionRequest,
    ) -> Result<ApiKycStaffSubmissionsUserIdRejectPostResponse, Error> {
        let req = KycRejectionRequest::from(body.clone());

        let updated = self
            .state
            .kyc
            .update_kyc_rejected(&path_params.user_id, &req)
            .await?;

        if updated {
            Ok(ApiKycStaffSubmissionsUserIdRejectPostResponse::Status200_KYCRejected)
        } else {
            Ok(ApiKycStaffSubmissionsUserIdRejectPostResponse::Status400_ValidationFailed)
        }
    }

    async fn api_kyc_staff_submissions_user_id_request_info_post(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        path_params: &models::ApiKycStaffSubmissionsUserIdRequestInfoPostPathParams,
        body: &models::KycRequestInfoRequest,
    ) -> Result<ApiKycStaffSubmissionsUserIdRequestInfoPostResponse, Error> {
        let req = KycRequestInfoRequest::from(body.clone());

        let updated = self
            .state
            .kyc
            .update_kyc_request_info(&path_params.user_id, &req)
            .await?;

        if updated {
            Ok(ApiKycStaffSubmissionsUserIdRequestInfoPostResponse::Status200_AdditionalInfoRequested)
        } else {
            Ok(ApiKycStaffSubmissionsUserIdRequestInfoPostResponse::Status400_ValidationFailed)
        }
    }

    async fn api_kyc_staff_submissions_user_id_documents_document_id_download_url_post(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        path_params: &models::ApiKycStaffSubmissionsUserIdDocumentsDocumentIdDownloadUrlPostPathParams,
        body: &Option<models::PresignedDownloadUrlRequest>,
    ) -> Result<ApiKycStaffSubmissionsUserIdDocumentsDocumentIdDownloadUrlPostResponse, Error> {
        let document = self
            .state
            .kyc
            .get_kyc_document(&path_params.user_id, &path_params.document_id)
            .await?;

        let Some(document) = document else {
            return Ok(ApiKycStaffSubmissionsUserIdDocumentsDocumentIdDownloadUrlPostResponse::Status404_SubmissionOrDocumentNotFound);
        };

        let expires_in = body
            .as_ref()
            .and_then(|b| b.expires_in_seconds)
            .unwrap_or(300)
            .clamp(60, 3600);

        let content_disposition = body
            .as_ref()
            .and_then(|b| b.response_content_disposition.clone());

        let presigned_req = self
            .state
            .s3
            .get_object()
            .bucket(&document.s3_bucket)
            .key(&document.s3_key)
            .set_response_content_disposition(content_disposition)
            .presigned(
                aws_sdk_s3::presigning::PresigningConfig::expires_in(
                    std::time::Duration::from_secs(expires_in as u64),
                )
                .map_err(|e| Error::s3(e.to_string()))?,
            )
            .await
            .map_err(|e| Error::s3(e.to_string()))?;

        let expires_at = chrono::Utc::now() + chrono::Duration::seconds(expires_in as i64);

        let response = models::PresignedDownloadUrlResponse {
            url: presigned_req.uri().to_string(),
            expires_at,
            document_id: Some(document.id),
            file_name: Some(document.file_name),
            mime_type: Some(document.mime_type),
        };

        Ok(ApiKycStaffSubmissionsUserIdDocumentsDocumentIdDownloadUrlPostResponse::Status200_PresignedDownloadURLCreated(response))
    }
}
