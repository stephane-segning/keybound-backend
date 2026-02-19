use super::BackendApi;
use axum_extra::extract::CookieJar;
use backend_auth::JwtToken;
use backend_core::Error;
use backend_repository::{KycReviewCaseRow, KycSubmissionFilter};
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
                path_params.submission_id.clone(),
                Some(reviewer_id),
                body.notes.clone(),
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
            if let Err(err) = self
                .state
                .provisioning_queue
                .enqueue_fineract_provisioning(&submission.user_id)
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
            .head_object(&document.bucket, &document.object_key)
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

        let url = self
            .state
            .s3
            .presign_get_object(
                &document.bucket,
                &document.object_key,
                std::time::Duration::from_secs(expires_in as u64),
                content_disposition,
            )
            .await?;

        Ok(ApiKycStaffSubmissionsSubmissionIdDocumentsDocumentIdDownloadUrlPostResponse::Status200_PresignedDownloadURLCreated(models::PresignedDownloadUrlResponse {
            url,
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
                path_params.submission_id.clone(),
                Some(reviewer_id),
                body.reason.clone(),
                body.notes.clone(),
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
                path_params.case_id.clone(),
                body.decision.to_string(),
                body.reason_code.to_string(),
                body.comment.clone(),
                Some(reviewer_id),
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
            && let Err(err) = self
                .state
                .provisioning_queue
                .enqueue_fineract_provisioning(&submission.user_id)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{create_fake_jwt, MockFileStorage, MockKycRepo, TestAppStateBuilder};
    use backend_repository::{
        KycStaffDocumentRow, KycStaffSubmissionDetailRow, KycStaffSubmissionSummaryRow,
    };
    use chrono::Utc;
    use mockall::predicate::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_api_kyc_staff_submissions_get_success() {
        let mut kyc_repo = MockKycRepo::new();
        kyc_repo
            .expect_list_staff_submissions()
            .returning(|filter| {
                assert_eq!(filter.page, 1);
                assert_eq!(filter.limit, 10);
                Ok((
                    vec![KycStaffSubmissionSummaryRow {
                        submission_id: "sub123".to_string(),
                        user_id: "user123".to_string(),
                        first_name: Some("John".to_string()),
                        last_name: Some("Doe".to_string()),
                        email: Some("john@example.com".to_string()),
                        phone_number: Some("+1234567890".to_string()),
                        status: "PENDING_REVIEW".to_string(),
                        submitted_at: Some(Utc::now()),
                    }],
                    1,
                ))
            });

        let state = TestAppStateBuilder::new()
            .with_kyc(Arc::new(kyc_repo))
            .build()
            .await;
        let api = BackendApi::new(
            Arc::new(state.clone()),
            state.oidc_state.clone(),
            state.signature_state.clone(),
        );

        let query_params = models::ApiKycStaffSubmissionsGetQueryParams {
            page: Some(1),
            limit: Some(10),
            status: None,
            search: None,
        };

        let result = api
            .api_kyc_staff_submissions_get(
                &Method::GET,
                &Host::from(http::uri::Authority::from_static("localhost")),
                &CookieJar::new(),
                &create_fake_jwt("staff123"),
                &query_params,
            )
            .await;

        assert!(result.is_ok());
        match result.unwrap() {
            ApiKycStaffSubmissionsGetResponse::Status200_PageOfKYCSubmissions(resp) => {
                assert_eq!(resp.total, Some(1));
                let items = resp.items.unwrap();
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].submission_id, Some("sub123".to_string()));
            }
            _ => panic!("Unexpected response"),
        }
    }

    #[tokio::test]
    async fn test_api_kyc_staff_submissions_submission_id_approve_post_success() {
        let mut kyc_repo = MockKycRepo::new();
        kyc_repo
            .expect_approve_submission()
            .with(
                eq("sub123".to_string()),
                eq(Some("staff123".to_string())),
                eq(Some("Looks good".to_string())),
            )
            .returning(|_, _, _| Ok(true));

        kyc_repo
            .expect_get_staff_submission()
            .with(eq("sub123"))
            .returning(|_| {
                Ok(Some(KycStaffSubmissionDetailRow {
                    submission_id: "sub123".to_string(),
                    user_id: "user123".to_string(),
                    first_name: Some("John".to_string()),
                    last_name: Some("Doe".to_string()),
                    email: Some("john@example.com".to_string()),
                    phone_number: Some("+1234567890".to_string()),
                    date_of_birth: None,
                    nationality: None,
                    status: "APPROVED".to_string(),
                    submitted_at: Some(Utc::now()),
                    reviewed_at: Some(Utc::now()),
                    reviewed_by: Some("staff123".to_string()),
                    rejection_reason: None,
                    review_notes: Some("Looks good".to_string()),
                }))
            });

        let mut provisioning_queue = crate::test_utils::MockProvisioningQueue::new();
        provisioning_queue
            .expect_enqueue_fineract_provisioning()
            .with(eq("user123"))
            .returning(|_| Ok(()));

        let state = TestAppStateBuilder::new()
            .with_kyc(Arc::new(kyc_repo))
            .with_provisioning_queue(Arc::new(provisioning_queue))
            .build()
            .await;
        let api = BackendApi::new(
            Arc::new(state.clone()),
            state.oidc_state.clone(),
            state.signature_state.clone(),
        );

        let path_params = models::ApiKycStaffSubmissionsSubmissionIdApprovePostPathParams {
            submission_id: "sub123".to_string(),
        };
        let body = models::KycApprovalRequest {
            new_tier: 1,
            notes: Some("Looks good".to_string()),
        };

        let result = api
            .api_kyc_staff_submissions_submission_id_approve_post(
                &Method::POST,
                &Host::from(http::uri::Authority::from_static("localhost")),
                &CookieJar::new(),
                &create_fake_jwt("staff123"),
                &path_params,
                &body,
            )
            .await;

        assert!(result.is_ok());
        assert!(matches!(
            result.unwrap(),
            ApiKycStaffSubmissionsSubmissionIdApprovePostResponse::Status200_KYCApproved
        ));
    }

    #[tokio::test]
    async fn test_api_kyc_staff_submissions_submission_id_reject_post_success() {
        let mut kyc_repo = MockKycRepo::new();
        kyc_repo
            .expect_reject_submission()
            .with(
                eq("sub123".to_string()),
                eq(Some("staff123".to_string())),
                eq("Invalid ID".to_string()),
                eq(Some("Please re-upload".to_string())),
            )
            .returning(|_, _, _, _| Ok(true));

        let state = TestAppStateBuilder::new()
            .with_kyc(Arc::new(kyc_repo))
            .build()
            .await;
        let api = BackendApi::new(
            Arc::new(state.clone()),
            state.oidc_state.clone(),
            state.signature_state.clone(),
        );

        let path_params = models::ApiKycStaffSubmissionsSubmissionIdRejectPostPathParams {
            submission_id: "sub123".to_string(),
        };
        let body = models::KycRejectionRequest {
            reason: "Invalid ID".to_string(),
            notes: Some("Please re-upload".to_string()),
        };

        let result = api
            .api_kyc_staff_submissions_submission_id_reject_post(
                &Method::POST,
                &Host::from(http::uri::Authority::from_static("localhost")),
                &CookieJar::new(),
                &create_fake_jwt("staff123"),
                &path_params,
                &body,
            )
            .await;

        assert!(result.is_ok());
        assert!(matches!(
            result.unwrap(),
            ApiKycStaffSubmissionsSubmissionIdRejectPostResponse::Status200_KYCRejected
        ));
    }

    #[tokio::test]
    async fn test_api_kyc_staff_submissions_submission_id_request_info_post_success() {
        let mut kyc_repo = MockKycRepo::new();
        kyc_repo
            .expect_request_submission_info()
            .with(eq("sub123"), eq("Need more info"))
            .returning(|_, _| Ok(true));

        let state = TestAppStateBuilder::new()
            .with_kyc(Arc::new(kyc_repo))
            .build()
            .await;
        let api = BackendApi::new(
            Arc::new(state.clone()),
            state.oidc_state.clone(),
            state.signature_state.clone(),
        );

        let path_params = models::ApiKycStaffSubmissionsSubmissionIdRequestInfoPostPathParams {
            submission_id: "sub123".to_string(),
        };
        let body = models::KycRequestInfoRequest {
            message: "Need more info".to_string(),
        };

        let result = api
            .api_kyc_staff_submissions_submission_id_request_info_post(
                &Method::POST,
                &Host::from(http::uri::Authority::from_static("localhost")),
                &CookieJar::new(),
                &create_fake_jwt("staff123"),
                &path_params,
                &body,
            )
            .await;

        assert!(result.is_ok());
        assert!(matches!(
            result.unwrap(),
            ApiKycStaffSubmissionsSubmissionIdRequestInfoPostResponse::Status200_AdditionalInfoRequested
        ));
    }

    #[tokio::test]
    async fn test_download_document_not_found() {
        let mut kyc_repo = MockKycRepo::new();
        kyc_repo
            .expect_get_staff_submission_document()
            .returning(|_, _| Ok(None));

        let state = TestAppStateBuilder::new()
            .with_kyc(Arc::new(kyc_repo))
            .build()
            .await;
        let api = BackendApi::new(
            Arc::new(state.clone()),
            state.oidc_state.clone(),
            state.signature_state.clone(),
        );

        let path_params =
            models::ApiKycStaffSubmissionsSubmissionIdDocumentsDocumentIdDownloadUrlPostPathParams {
                submission_id: "sub123".to_string(),
                document_id: "doc123".to_string(),
            };

        let result = api
            .api_kyc_staff_submissions_submission_id_documents_document_id_download_url_post(
                &Method::POST,
                &Host::from(http::uri::Authority::from_static("localhost")),
                &CookieJar::new(),
                &create_fake_jwt("staff123"),
                &path_params,
                &None,
            )
            .await;

        assert!(result.is_ok());
        assert!(matches!(
            result.unwrap(),
            ApiKycStaffSubmissionsSubmissionIdDocumentsDocumentIdDownloadUrlPostResponse::Status404_SubmissionOrDocumentNotFound
        ));
    }

    #[tokio::test]
    async fn test_download_document_expired() {
        let mut kyc_repo = MockKycRepo::new();
        kyc_repo
            .expect_get_staff_submission_document()
            .returning(|_, _| {
                Ok(Some(KycStaffDocumentRow {
                    id: "doc123".to_string(),
                    submission_id: "sub123".to_string(),
                    document_type: "IDENTITY".to_string(),
                    file_name: "id.jpg".to_string(),
                    mime_type: "image/jpeg".to_string(),
                    bucket: "bucket".to_string(),
                    object_key: "key".to_string(),
                    uploaded_at: Utc::now(),
                }))
            });

        let mut s3 = MockFileStorage::new();
        s3.expect_head_object()
            .returning(|_, _| Err(Error::s3("NotFound")));

        let state = TestAppStateBuilder::new()
            .with_kyc(Arc::new(kyc_repo))
            .with_s3(Arc::new(s3))
            .build()
            .await;
        let api = BackendApi::new(
            Arc::new(state.clone()),
            state.oidc_state.clone(),
            state.signature_state.clone(),
        );

        let path_params =
            models::ApiKycStaffSubmissionsSubmissionIdDocumentsDocumentIdDownloadUrlPostPathParams {
                submission_id: "sub123".to_string(),
                document_id: "doc123".to_string(),
            };

        let result = api
            .api_kyc_staff_submissions_submission_id_documents_document_id_download_url_post(
                &Method::POST,
                &Host::from(http::uri::Authority::from_static("localhost")),
                &CookieJar::new(),
                &create_fake_jwt("staff123"),
                &path_params,
                &None,
            )
            .await;

        assert!(result.is_ok());
        assert!(matches!(
            result.unwrap(),
            ApiKycStaffSubmissionsSubmissionIdDocumentsDocumentIdDownloadUrlPostResponse::Status410_DocumentNoLongerAvailable
        ));
    }

    #[tokio::test]
    async fn test_download_document_success() {
        let mut kyc_repo = MockKycRepo::new();
        kyc_repo
            .expect_get_staff_submission_document()
            .returning(|_, _| {
                Ok(Some(KycStaffDocumentRow {
                    id: "doc123".to_string(),
                    submission_id: "sub123".to_string(),
                    document_type: "IDENTITY".to_string(),
                    file_name: "id.jpg".to_string(),
                    mime_type: "image/jpeg".to_string(),
                    bucket: "bucket".to_string(),
                    object_key: "key".to_string(),
                    uploaded_at: Utc::now(),
                }))
            });

        let mut s3 = MockFileStorage::new();
        s3.expect_head_object().returning(|_, _| Ok(()));
        s3.expect_presign_get_object()
            .returning(|_, _, _, _| Ok("http://presigned-url".to_string()));

        let state = TestAppStateBuilder::new()
            .with_kyc(Arc::new(kyc_repo))
            .with_s3(Arc::new(s3))
            .build()
            .await;
        let api = BackendApi::new(
            Arc::new(state.clone()),
            state.oidc_state.clone(),
            state.signature_state.clone(),
        );

        let path_params =
            models::ApiKycStaffSubmissionsSubmissionIdDocumentsDocumentIdDownloadUrlPostPathParams {
                submission_id: "sub123".to_string(),
                document_id: "doc123".to_string(),
            };

        let result = api
            .api_kyc_staff_submissions_submission_id_documents_document_id_download_url_post(
                &Method::POST,
                &Host::from(http::uri::Authority::from_static("localhost")),
                &CookieJar::new(),
                &create_fake_jwt("staff123"),
                &path_params,
                &None,
            )
            .await;

        assert!(result.is_ok());
        match result.unwrap() {
            ApiKycStaffSubmissionsSubmissionIdDocumentsDocumentIdDownloadUrlPostResponse::Status200_PresignedDownloadURLCreated(
                resp,
            ) => {
                assert_eq!(resp.url, "http://presigned-url");
                assert_eq!(resp.document_id, Some("doc123".to_string()));
            }
            _ => panic!("Unexpected response"),
        }
    }
}
