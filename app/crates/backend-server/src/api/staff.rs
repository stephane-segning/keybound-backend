use super::BackendApi;
use crate::worker;
use axum_extra::extract::CookieJar;
use backend_auth::JwtToken;
use backend_core::Error;
use backend_repository::{KycRepo, KycReviewCaseRow, KycSubmissionFilter};
use gen_oas_server_staff::apis::kyc_review::{
    ApiKycStaffSubmissionsGetResponse, ApiKycStaffSubmissionsSubmissionIdApprovePostResponse,
    ApiKycStaffSubmissionsSubmissionIdDocumentsDocumentIdDownloadUrlPostResponse,
    ApiKycStaffSubmissionsSubmissionIdGetResponse,
    ApiKycStaffSubmissionsSubmissionIdRejectPostResponse,
    ApiKycStaffSubmissionsSubmissionIdRequestInfoPostResponse, KycReview,
    StaffReviewCasesCaseIdDecisionPostResponse, StaffReviewCasesCaseIdGetResponse,
    StaffReviewQueueGetResponse,
};
use gen_oas_server_staff::models;
use headers::Host;
use http::Method;
use tracing::{info, warn};

#[backend_core::async_trait]
impl KycReview<Error> for BackendApi {
    type Claims = JwtToken;

    async fn api_kyc_staff_submissions_get(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        query_params: &models::ApiKycStaffSubmissionsGetQueryParams,
    ) -> Result<ApiKycStaffSubmissionsGetResponse, Error> {
        let (page, limit) = Self::normalize_page_limit(query_params.page, query_params.limit);
        let filter = KycSubmissionFilter {
            status: query_params.status.map(|status| status.to_string()),
            search: query_params.search.clone(),
            page,
            limit,
        };

        let (rows, total_count) = self.state.kyc.list_staff_submissions(filter).await?;

        let items = rows
            .into_iter()
            .map(|row| models::KycSubmissionSummary {
                submission_id: Some(row.submission_id),
                user_id: Some(row.user_id),
                first_name: row.first_name,
                last_name: row.last_name,
                email: row.email,
                phone_number: row.phone_number,
                kyc_tier: None,
                kyc_status: parse_staff_status(&row.status),
                submitted_at: row.submitted_at.map(|ts| ts.to_rfc3339()),
            })
            .collect::<Vec<_>>();

        Ok(
            ApiKycStaffSubmissionsGetResponse::Status200_PageOfKYCSubmissions(
                models::KycSubmissionsResponse {
                    items: Some(items),
                    total: Some(i32::try_from(total_count).unwrap_or(i32::MAX)),
                    page: Some(page),
                    page_size: Some(limit),
                },
            ),
        )
    }

    async fn api_kyc_staff_submissions_submission_id_approve_post(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        path_params: &models::ApiKycStaffSubmissionsSubmissionIdApprovePostPathParams,
        body: &models::KycApprovalRequest,
    ) -> Result<ApiKycStaffSubmissionsSubmissionIdApprovePostResponse, Error> {
        let reviewer_id = Self::require_user_id(claims)?;

        let updated = self
            .state
            .kyc
            .approve_submission(
                &path_params.submission_id,
                Some(reviewer_id.as_str()),
                body.notes.as_deref(),
            )
            .await?;

        if !updated {
            return Ok(
                ApiKycStaffSubmissionsSubmissionIdApprovePostResponse::Status400_ValidationFailed,
            );
        }

        info!(
            submission_id = %path_params.submission_id,
            new_tier = body.new_tier,
            notes = body.notes.as_deref(),
            "staff approved KYC, enqueuing fineract provisioning job"
        );

        if let Some(submission) = self
            .state
            .kyc
            .get_staff_submission(&path_params.submission_id)
            .await?
        {
            if let Err(err) = worker::enqueue_fineract_provisioning(
                &self.state.config.redis.url,
                &submission.user_id,
            )
            .await
            {
                warn!(
                    submission_id = %path_params.submission_id,
                    error = %err,
                    "failed to enqueue fineract provisioning job"
                );
            }
        }

        Ok(ApiKycStaffSubmissionsSubmissionIdApprovePostResponse::Status200_KYCApproved)
    }

    async fn api_kyc_staff_submissions_submission_id_documents_document_id_download_url_post(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        path_params: &models::ApiKycStaffSubmissionsSubmissionIdDocumentsDocumentIdDownloadUrlPostPathParams,
        body: &Option<models::PresignedDownloadUrlRequest>,
    ) -> Result<ApiKycStaffSubmissionsSubmissionIdDocumentsDocumentIdDownloadUrlPostResponse, Error>
    {
        let document = self
            .state
            .kyc
            .get_staff_submission_document(&path_params.submission_id, &path_params.document_id)
            .await?;

        let Some(document) = document else {
            return Ok(ApiKycStaffSubmissionsSubmissionIdDocumentsDocumentIdDownloadUrlPostResponse::Status404_SubmissionOrDocumentNotFound);
        };

        if let Err(err) = self
            .state
            .s3
            .head_object()
            .bucket(&document.bucket)
            .key(&document.object_key)
            .send()
            .await
        {
            let message = err.to_string();
            if message.contains("NotFound")
                || message.contains("NoSuchKey")
                || message.contains("404")
            {
                return Ok(ApiKycStaffSubmissionsSubmissionIdDocumentsDocumentIdDownloadUrlPostResponse::Status410_DocumentNoLongerAvailable);
            }
            return Err(Error::s3(message));
        }

        let expires_in = body
            .as_ref()
            .and_then(|request| request.expires_in_seconds)
            .unwrap_or(300)
            .clamp(60, 3600);

        let content_disposition = body
            .as_ref()
            .and_then(|request| request.response_content_disposition.clone());

        let presigned_req = self
            .state
            .s3
            .get_object()
            .bucket(&document.bucket)
            .key(&document.object_key)
            .set_response_content_disposition(content_disposition)
            .presigned(
                aws_sdk_s3::presigning::PresigningConfig::expires_in(
                    std::time::Duration::from_secs(expires_in as u64),
                )
                .map_err(|e| Error::s3(e.to_string()))?,
            )
            .await
            .map_err(|e| Error::s3(e.to_string()))?;

        Ok(ApiKycStaffSubmissionsSubmissionIdDocumentsDocumentIdDownloadUrlPostResponse::Status200_PresignedDownloadURLCreated(models::PresignedDownloadUrlResponse {
            url: presigned_req.uri().to_string(),
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(i64::from(expires_in)),
            document_id: Some(document.id),
            file_name: Some(document.file_name),
            mime_type: Some(document.mime_type),
        }))
    }

    async fn api_kyc_staff_submissions_submission_id_get(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        path_params: &models::ApiKycStaffSubmissionsSubmissionIdGetPathParams,
        query_params: &models::ApiKycStaffSubmissionsSubmissionIdGetQueryParams,
    ) -> Result<ApiKycStaffSubmissionsSubmissionIdGetResponse, Error> {
        let (page, limit) = Self::normalize_page_limit(query_params.page, query_params.limit);

        let detail = self
            .state
            .kyc
            .get_staff_submission(&path_params.submission_id)
            .await?;

        let Some(detail) = detail else {
            return Ok(ApiKycStaffSubmissionsSubmissionIdGetResponse::Status404_SubmissionNotFound);
        };

        let documents = self
            .state
            .kyc
            .list_staff_submission_documents(&path_params.submission_id)
            .await?;

        let total_documents = i32::try_from(documents.len()).unwrap_or(i32::MAX);
        let start = usize::try_from((page - 1).saturating_mul(limit)).unwrap_or(0);
        let page_size = usize::try_from(limit).unwrap_or(0);
        let paged_documents = documents
            .into_iter()
            .skip(start)
            .take(page_size)
            .map(|doc| models::KycDocumentDto {
                id: Some(doc.id),
                r_type: Some(doc.document_type),
                file_name: Some(doc.file_name),
                mime_type: Some(doc.mime_type),
                url: None,
                uploaded_at: Some(doc.uploaded_at.to_rfc3339()),
            })
            .collect::<Vec<_>>();

        Ok(
            ApiKycStaffSubmissionsSubmissionIdGetResponse::Status200_DetailedSubmission(
                models::KycSubmissionDetailResponse {
                    submission_id: Some(detail.submission_id),
                    user_id: Some(detail.user_id),
                    first_name: detail.first_name,
                    last_name: detail.last_name,
                    email: detail.email,
                    phone_number: detail.phone_number,
                    date_of_birth: detail.date_of_birth,
                    nationality: detail.nationality,
                    kyc_tier: None,
                    kyc_status: parse_staff_status(&detail.status),
                    documents: Some(paged_documents),
                    submitted_at: detail.submitted_at.map(|ts| ts.to_rfc3339()),
                    reviewed_at: detail.reviewed_at.map(|ts| ts.to_rfc3339()),
                    reviewed_by: detail.reviewed_by,
                    rejection_reason: detail.rejection_reason,
                    review_notes: detail.review_notes,
                    page: Some(page),
                    page_size: Some(limit),
                    total_documents: Some(total_documents),
                },
            ),
        )
    }

    async fn api_kyc_staff_submissions_submission_id_reject_post(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        path_params: &models::ApiKycStaffSubmissionsSubmissionIdRejectPostPathParams,
        body: &models::KycRejectionRequest,
    ) -> Result<ApiKycStaffSubmissionsSubmissionIdRejectPostResponse, Error> {
        let reviewer_id = Self::require_user_id(claims)?;

        let updated = self
            .state
            .kyc
            .reject_submission(
                &path_params.submission_id,
                Some(reviewer_id.as_str()),
                &body.reason,
                body.notes.as_deref(),
            )
            .await?;

        if updated {
            Ok(ApiKycStaffSubmissionsSubmissionIdRejectPostResponse::Status200_KYCRejected)
        } else {
            Ok(ApiKycStaffSubmissionsSubmissionIdRejectPostResponse::Status400_ValidationFailed)
        }
    }

    async fn api_kyc_staff_submissions_submission_id_request_info_post(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        path_params: &models::ApiKycStaffSubmissionsSubmissionIdRequestInfoPostPathParams,
        body: &models::KycRequestInfoRequest,
    ) -> Result<ApiKycStaffSubmissionsSubmissionIdRequestInfoPostResponse, Error> {
        let updated = self
            .state
            .kyc
            .request_submission_info(&path_params.submission_id, &body.message)
            .await?;

        if updated {
            Ok(ApiKycStaffSubmissionsSubmissionIdRequestInfoPostResponse::Status200_AdditionalInfoRequested)
        } else {
            Ok(ApiKycStaffSubmissionsSubmissionIdRequestInfoPostResponse::Status400_ValidationFailed)
        }
    }

    async fn staff_review_cases_case_id_decision_post(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        path_params: &models::StaffReviewCasesCaseIdDecisionPostPathParams,
        body: &models::ReviewDecision,
    ) -> Result<StaffReviewCasesCaseIdDecisionPostResponse, Error> {
        let reviewer_id = Self::require_user_id(claims)?;
        let record = self
            .state
            .kyc
            .decide_review_case(
                &path_params.case_id,
                &body.decision.to_string(),
                &body.reason_code.to_string(),
                body.comment.as_deref(),
                Some(reviewer_id.as_str()),
            )
            .await?;

        let Some(record) = record else {
            return Err(Error::not_found(
                "REVIEW_CASE_NOT_FOUND",
                "Review case not found",
            ));
        };

        if record.decision == models::ReviewDecisionOutcome::Approve.to_string()
            && let Some(submission) = self.state.kyc.get_staff_submission(&record.case_id).await?
            && let Err(err) = worker::enqueue_fineract_provisioning(
                &self.state.config.redis.url,
                &submission.user_id,
            )
            .await
        {
            warn!(
                case_id = %record.case_id,
                error = %err,
                "failed to enqueue fineract provisioning job"
            );
        }

        let decision = parse_review_outcome(&record.decision)?;
        Ok(
            StaffReviewCasesCaseIdDecisionPostResponse::Status200_DecisionRecorded(
                models::ReviewDecisionResult::new(record.case_id, decision, record.decided_at),
            ),
        )
    }

    async fn staff_review_cases_case_id_get(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        path_params: &models::StaffReviewCasesCaseIdGetPathParams,
    ) -> Result<StaffReviewCasesCaseIdGetResponse, Error> {
        let row = self.state.kyc.get_review_case(&path_params.case_id).await?;
        let Some(row) = row else {
            return Err(Error::not_found(
                "REVIEW_CASE_NOT_FOUND",
                "Review case not found",
            ));
        };

        Ok(StaffReviewCasesCaseIdGetResponse::Status200_Case(
            review_case_from_row(row)?,
        ))
    }

    async fn staff_review_queue_get(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        query_params: &models::StaffReviewQueueGetQueryParams,
    ) -> Result<StaffReviewQueueGetResponse, Error> {
        let (page, limit) = Self::normalize_page_limit(query_params.page, query_params.limit);

        let (rows, total_count) = self.state.kyc.list_review_cases(page, limit).await?;
        let items = rows
            .into_iter()
            .map(review_case_from_row)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(StaffReviewQueueGetResponse::Status200_PaginatedListOfCases(
            models::ReviewQueueResponse::new(
                models::PaginationMeta::new(
                    page,
                    limit,
                    i32::try_from(total_count).unwrap_or(i32::MAX),
                ),
                items,
            ),
        ))
    }
}

fn parse_staff_status(raw: &str) -> Option<models::KycStatus> {
    raw.parse::<models::KycStatus>().ok()
}

fn parse_review_outcome(raw: &str) -> Result<models::ReviewDecisionOutcome, Error> {
    raw.parse::<models::ReviewDecisionOutcome>().map_err(|_| {
        Error::internal(
            "INVALID_REVIEW_DECISION",
            format!("Unsupported review decision outcome: {raw}"),
        )
    })
}

fn parse_identity_asset_type(raw: &str) -> Result<models::IdentityAssetType, Error> {
    raw.parse::<models::IdentityAssetType>().map_err(|_| {
        Error::internal(
            "INVALID_ASSET_TYPE",
            format!("Unsupported identity asset type: {raw}"),
        )
    })
}

fn review_case_from_row(row: KycReviewCaseRow) -> Result<models::ReviewCase, Error> {
    let evidence = row
        .evidence
        .into_iter()
        .map(|entry| {
            Ok(models::ReviewCaseEvidenceInner::new(
                parse_identity_asset_type(&entry.asset_type)?,
                entry.evidence_id,
            ))
        })
        .collect::<Result<Vec<_>, Error>>()?;

    let status = parse_staff_status(&row.status).unwrap_or(models::KycStatus::PendingReview);

    Ok(models::ReviewCase::new(
        row.case_id,
        row.user_id,
        row.step_id,
        status,
        row.submitted_at,
        models::ReviewCaseFullName::new(row.first_name, row.last_name),
        evidence,
    ))
}
