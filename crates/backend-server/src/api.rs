use crate::state::AppState;
use aws_sdk_s3::presigning::PresigningConfig;
use backend_auth::{KcContext, ServiceContext};
use backend_model::{bff as bff_map, kc as kc_map, staff as staff_map};
use backend_repository::{
    ApprovalCreated, BffRepo, KcRepo, KycDocumentInsert, KycSubmissionsQuery, RepoError,
    SmsPendingInsert, StaffRepo,
};
use chrono::Utc;
use gen_oas_server_bff::{
    Api as BffApi, ApiRegistrationKycDocumentsPostResponse, ApiRegistrationKycStatusGetResponse,
    ApiRegistrationLimitsGetResponse,
};
use gen_oas_server_kc::{
    Api as KcApi, CancelApprovalResponse, ConfirmSmsResponse, CreateApprovalResponse,
    CreateUserResponse, DecideApprovalResponse, DeleteUserResponse, EnrollmentBindResponse,
    EnrollmentPrecheckResponse, GetApprovalResponse, GetUserResponse, ListUserApprovalsResponse,
    ListUserDevicesResponse, LookupDeviceResponse, ResolveOrCreateUserByPhoneResponse,
    ResolveUserByPhoneResponse, SearchUsersResponse, SendSmsResponse, UpdateUserResponse,
};
use gen_oas_server_staff::{
    Api as StaffApi, ApiKycStaffSubmissionsExternalIdApprovePostResponse,
    ApiKycStaffSubmissionsExternalIdGetResponse, ApiKycStaffSubmissionsExternalIdRejectPostResponse,
    ApiKycStaffSubmissionsExternalIdRequestInfoPostResponse, ApiKycStaffSubmissionsGetResponse,
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use swagger::ApiError;

#[derive(Clone)]
pub struct BackendApi {
    state: Arc<AppState>,
}

impl BackendApi {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    fn require_external_id(x_external_id: Option<String>) -> std::result::Result<String, ApiError> {
        x_external_id.ok_or_else(|| ApiError("Missing X-External-Id".to_owned()))
    }
}

fn kc_error(code: &str, message: &str) -> gen_oas_server_kc::models::Error {
    gen_oas_server_kc::models::Error::new(code.to_owned(), message.to_owned())
}

fn repo_err(err: RepoError) -> ApiError {
    ApiError(err.to_string())
}

// ---- BFF ----

#[backend_core::async_trait]
impl BffApi<ServiceContext> for BackendApi {
    async fn api_registration_kyc_documents_post(
        &self,
        kyc_document_upload_request: gen_oas_server_bff::models::KycDocumentUploadRequest,
        x_external_id: Option<String>,
        _context: &ServiceContext,
    ) -> std::result::Result<ApiRegistrationKycDocumentsPostResponse, ApiError> {
        let external_id = Self::require_external_id(x_external_id)?;
        let req: bff_map::KycDocumentUploadRequest = kyc_document_upload_request.into();

        let now = Utc::now();
        let expires_at =
            now + chrono::Duration::seconds(self.state.config.aws.s3.presign_ttl_seconds as i64);
        let object_id = backend_id::prefixed("obj").map_err(|e| ApiError(e.to_string()))?;
        let s3_key = format!("kyc/{external_id}/{object_id}/{}", req.file_name);

        self.state
            .repository
            .ensure_kyc_profile(&external_id)
            .await
            .map_err(repo_err)?;

        let doc_row = self
            .state
            .repository
            .insert_kyc_document_intent(KycDocumentInsert {
                external_id: external_id.clone(),
                document_type: req.document_type.clone(),
                file_name: req.file_name.clone(),
                mime_type: req.mime_type.clone(),
                content_length: req.content_length,
                s3_bucket: self.state.config.aws.s3.bucket.clone(),
                s3_key: s3_key.clone(),
                presigned_expires_at: expires_at,
            })
            .await
            .map_err(repo_err)?;

        let presign_cfg = PresigningConfig::expires_in(Duration::from_secs(
            self.state.config.aws.s3.presign_ttl_seconds,
        ))
        .map_err(|e| ApiError(e.to_string()))?;

        let presigned = self
            .state
            .s3
            .put_object()
            .bucket(&self.state.config.aws.s3.bucket)
            .key(&s3_key)
            .content_type(req.mime_type)
            .content_length(req.content_length)
            .presigned(presign_cfg)
            .await
            .map_err(|e| ApiError(e.to_string()))?;

        let headers = presigned
            .headers()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<HashMap<String, String>>();

        let dto = bff_map::KycDocumentUploadResponseDto {
            document_id: Some(doc_row.id),
            document_type: Some(doc_row.document_type),
            status: Some(doc_row.status),
            uploaded_at: doc_row.uploaded_at,
            file_name: Some(doc_row.file_name),
            mime_type: Some(doc_row.mime_type),
            upload_url: Some(presigned.uri().to_string()),
            upload_method: Some(presigned.method().to_string()),
            upload_headers: Some(headers),
            expires_at: Some(expires_at),
            s3_bucket: Some(doc_row.s3_bucket),
            s3_key: Some(doc_row.s3_key),
        };

        Ok(ApiRegistrationKycDocumentsPostResponse::UploadURLCreatedSuccessfully(dto.into()))
    }

    async fn api_registration_kyc_status_get(
        &self,
        x_external_id: Option<String>,
        _context: &ServiceContext,
    ) -> std::result::Result<ApiRegistrationKycStatusGetResponse, ApiError> {
        let external_id = Self::require_external_id(x_external_id)?;
        let profile = self
            .state
            .repository
            .get_kyc_profile(&external_id)
            .await
            .map_err(repo_err)?;

        let Some(profile) = profile else {
            return Ok(ApiRegistrationKycStatusGetResponse::CustomerNotFound);
        };

        let docs = self
            .state
            .repository
            .list_kyc_documents(&external_id)
            .await
            .map_err(repo_err)?;

        let documents = docs
            .into_iter()
            .map(bff_map::KycStatusDocumentStatusDto::from)
            .map(Into::into)
            .collect::<Vec<_>>();

        let dto = bff_map::KycStatusResponseDto {
            kyc_tier: Some(profile.kyc_tier),
            kyc_status: Some(profile.kyc_status),
            documents: Some(documents),
            required_documents: Some(vec![]),
            missing_documents: Some(vec![]),
        };

        Ok(ApiRegistrationKycStatusGetResponse::KYCStatusInformation(dto.into()))
    }

    async fn api_registration_limits_get(
        &self,
        x_external_id: Option<String>,
        _context: &ServiceContext,
    ) -> std::result::Result<ApiRegistrationLimitsGetResponse, ApiError> {
        let external_id = Self::require_external_id(x_external_id)?;

        let kyc_tier = self
            .state
            .repository
            .get_kyc_tier(&external_id)
            .await
            .map_err(repo_err)?;

        let Some(kyc_tier) = kyc_tier else {
            return Ok(ApiRegistrationLimitsGetResponse::CustomerNotFound);
        };

        let mut resp = gen_oas_server_bff::models::LimitsResponse::new();
        resp.kyc_tier = Some(kyc_tier);
        resp.tier_name = Some(match kyc_tier {
            0 => "TIER_0",
            1 => "TIER_1",
            2 => "TIER_2",
            _ => "TIER_UNKNOWN",
        }
        .to_owned());
        resp.currency = Some("USD".to_owned());
        resp.allowed_payment_methods = Some(vec!["CARD".to_owned(), "BANK_TRANSFER".to_owned()]);
        resp.restricted_features = Some(vec![]);

        Ok(ApiRegistrationLimitsGetResponse::LimitsAndUsageDetails(resp))
    }
}

// ---- STAFF ----

#[backend_core::async_trait]
impl StaffApi<ServiceContext> for BackendApi {
    async fn api_kyc_staff_submissions_get(
        &self,
        status: Option<String>,
        search: Option<String>,
        page: Option<i32>,
        limit: Option<i32>,
        _context: &ServiceContext,
    ) -> std::result::Result<ApiKycStaffSubmissionsGetResponse, ApiError> {
        let query = KycSubmissionsQuery {
            status,
            search,
            page: page.unwrap_or(1).max(1),
            limit: limit.unwrap_or(20).clamp(1, 100),
        };
        let data = self
            .state
            .repository
            .list_kyc_submissions(query.clone())
            .await
            .map_err(repo_err)?;

        let items = data
            .items
            .into_iter()
            .map(staff_map::KycSubmissionSummaryDto::from)
            .map(Into::into)
            .collect::<Vec<_>>();

        let dto = staff_map::KycSubmissionsResponseDto {
            items: Some(items),
            total: Some(data.total),
            page: Some(query.page),
            page_size: Some(query.limit),
        };

        Ok(ApiKycStaffSubmissionsGetResponse::PageOfKYCSubmissions(dto.into()))
    }

    async fn api_kyc_staff_submissions_external_id_get(
        &self,
        external_id: String,
        _context: &ServiceContext,
    ) -> std::result::Result<ApiKycStaffSubmissionsExternalIdGetResponse, ApiError> {
        let profile = self
            .state
            .repository
            .get_kyc_submission(&external_id)
            .await
            .map_err(repo_err)?;
        let Some(profile) = profile else {
            return Ok(ApiKycStaffSubmissionsExternalIdGetResponse::SubmissionNotFound);
        };

        let docs = self
            .state
            .repository
            .list_kyc_documents(&external_id)
            .await
            .map_err(repo_err)?;

        let mut dto = staff_map::KycSubmissionDetailResponseDto::from_profile(profile);
        dto.documents = Some(
            docs.into_iter()
                .map(staff_map::KycDocumentDto::from)
                .map(Into::into)
                .collect(),
        );

        Ok(ApiKycStaffSubmissionsExternalIdGetResponse::DetailedSubmission(dto.into()))
    }

    async fn api_kyc_staff_submissions_external_id_approve_post(
        &self,
        external_id: String,
        kyc_approval_request: gen_oas_server_staff::models::KycApprovalRequest,
        _context: &ServiceContext,
    ) -> std::result::Result<ApiKycStaffSubmissionsExternalIdApprovePostResponse, ApiError> {
        let req: staff_map::KycApprovalRequest = kyc_approval_request.into();
        let updated = self
            .state
            .repository
            .update_kyc_approved(&external_id, &req)
            .await
            .map_err(repo_err)?;
        if !updated {
            return Ok(ApiKycStaffSubmissionsExternalIdApprovePostResponse::ValidationFailed);
        }
        Ok(ApiKycStaffSubmissionsExternalIdApprovePostResponse::KYCApproved)
    }

    async fn api_kyc_staff_submissions_external_id_reject_post(
        &self,
        external_id: String,
        kyc_rejection_request: gen_oas_server_staff::models::KycRejectionRequest,
        _context: &ServiceContext,
    ) -> std::result::Result<ApiKycStaffSubmissionsExternalIdRejectPostResponse, ApiError> {
        let req: staff_map::KycRejectionRequest = kyc_rejection_request.into();
        let updated = self
            .state
            .repository
            .update_kyc_rejected(&external_id, &req)
            .await
            .map_err(repo_err)?;
        if !updated {
            return Ok(ApiKycStaffSubmissionsExternalIdRejectPostResponse::ValidationFailed);
        }
        Ok(ApiKycStaffSubmissionsExternalIdRejectPostResponse::KYCRejected)
    }

    async fn api_kyc_staff_submissions_external_id_request_info_post(
        &self,
        external_id: String,
        kyc_request_info_request: gen_oas_server_staff::models::KycRequestInfoRequest,
        _context: &ServiceContext,
    ) -> std::result::Result<ApiKycStaffSubmissionsExternalIdRequestInfoPostResponse, ApiError> {
        let req: staff_map::KycRequestInfoRequest = kyc_request_info_request.into();
        let updated = self
            .state
            .repository
            .update_kyc_request_info(&external_id, &req)
            .await
            .map_err(repo_err)?;
        if !updated {
            return Ok(ApiKycStaffSubmissionsExternalIdRequestInfoPostResponse::ValidationFailed);
        }
        Ok(ApiKycStaffSubmissionsExternalIdRequestInfoPostResponse::AdditionalInfoRequested)
    }
}

// ---- KC ----

#[backend_core::async_trait]
impl KcApi<KcContext> for BackendApi {
    async fn create_user(
        &self,
        user_upsert_request: gen_oas_server_kc::models::UserUpsertRequest,
        _context: &KcContext,
    ) -> std::result::Result<CreateUserResponse, ApiError> {
        let req: kc_map::UserUpsert = user_upsert_request.into();
        match self.state.repository.create_user(&req).await {
            Ok(row) => Ok(CreateUserResponse::Created(kc_map::UserRecordDto::from(row).into())),
            Err(RepoError::Conflict) => Ok(CreateUserResponse::Conflict(kc_error(
                "CONFLICT",
                "User already exists",
            ))),
            Err(err) => Err(repo_err(err)),
        }
    }

    async fn get_user(
        &self,
        user_id: String,
        _context: &KcContext,
    ) -> std::result::Result<GetUserResponse, ApiError> {
        let row = self
            .state
            .repository
            .get_user(&user_id)
            .await
            .map_err(repo_err)?;
        let Some(row) = row else {
            return Ok(GetUserResponse::NotFound(kc_error("NOT_FOUND", "User not found")));
        };
        Ok(GetUserResponse::User(kc_map::UserRecordDto::from(row).into()))
    }

    async fn update_user(
        &self,
        user_id: String,
        user_upsert_request: gen_oas_server_kc::models::UserUpsertRequest,
        _context: &KcContext,
    ) -> std::result::Result<UpdateUserResponse, ApiError> {
        let req: kc_map::UserUpsert = user_upsert_request.into();
        let row = self
            .state
            .repository
            .update_user(&user_id, &req)
            .await
            .map_err(repo_err)?;
        let Some(row) = row else {
            return Ok(UpdateUserResponse::NotFound(kc_error("NOT_FOUND", "User not found")));
        };
        Ok(UpdateUserResponse::Updated(kc_map::UserRecordDto::from(row).into()))
    }

    async fn delete_user(
        &self,
        user_id: String,
        _context: &KcContext,
    ) -> std::result::Result<DeleteUserResponse, ApiError> {
        let affected = self
            .state
            .repository
            .delete_user(&user_id)
            .await
            .map_err(repo_err)?;
        if affected == 0 {
            return Ok(DeleteUserResponse::NotFound(kc_error("NOT_FOUND", "User not found")));
        }
        Ok(DeleteUserResponse::Deleted)
    }

    async fn search_users(
        &self,
        user_search_request: gen_oas_server_kc::models::UserSearchRequest,
        _context: &KcContext,
    ) -> std::result::Result<SearchUsersResponse, ApiError> {
        let req: kc_map::UserSearch = user_search_request.into();
        let users = self
            .state
            .repository
            .search_users(&req)
            .await
            .map_err(repo_err)?;
        let out_users = users
            .into_iter()
            .map(kc_map::UserRecordDto::from)
            .map(Into::into)
            .collect::<Vec<_>>();
        let total_count = out_users.len() as i32;
        let resp = gen_oas_server_kc::models::UserSearchResponse {
            users: out_users,
            total_count: Some(total_count),
        };
        Ok(SearchUsersResponse::SearchResults(resp))
    }

    async fn lookup_device(
        &self,
        device_lookup_request: gen_oas_server_kc::models::DeviceLookupRequest,
        _context: &KcContext,
    ) -> std::result::Result<LookupDeviceResponse, ApiError> {
        let req: kc_map::DeviceLookupRequest = device_lookup_request.into();
        if req.device_id.is_none() && req.jkt.is_none() {
            return Ok(LookupDeviceResponse::BadRequest(kc_error(
                "BAD_REQUEST",
                "device_id or jkt must be set",
            )));
        }

        let row = self
            .state
            .repository
            .lookup_device(&req)
            .await
            .map_err(repo_err)?;
        let Some(row) = row else {
            return Ok(LookupDeviceResponse::NotFound(kc_error("NOT_FOUND", "Not found")));
        };

        let public_jwk = match &row.public_jwk {
            serde_json::Value::Object(map) => Some(map.clone().into_iter().collect()),
            _ => None,
        };
        let mut resp = gen_oas_server_kc::models::DeviceLookupResponse::new(true);
        resp.user_id = Some(row.user_id.clone());
        resp.device = Some(kc_map::DeviceRecordDto::from(row).into());
        resp.public_jwk = public_jwk;
        Ok(LookupDeviceResponse::LookupResult(resp))
    }

    async fn list_user_devices(
        &self,
        user_id: String,
        include_revoked: Option<bool>,
        _context: &KcContext,
    ) -> std::result::Result<ListUserDevicesResponse, ApiError> {
        let rows = self
            .state
            .repository
            .list_user_devices(&user_id, include_revoked.unwrap_or(false))
            .await
            .map_err(repo_err)?;
        let devices = rows
            .into_iter()
            .map(kc_map::DeviceRecordDto::from)
            .map(Into::into)
            .collect::<Vec<_>>();
        Ok(ListUserDevicesResponse::DeviceList(
            gen_oas_server_kc::models::UserDevicesResponse { user_id, devices },
        ))
    }

    async fn disable_user_device(
        &self,
        user_id: String,
        device_id: String,
        _context: &KcContext,
    ) -> std::result::Result<gen_oas_server_kc::DisableUserDeviceResponse, ApiError> {
        let row = self
            .state
            .repository
            .get_user_device(&user_id, &device_id)
            .await
            .map_err(repo_err)?;
        let Some(row) = row else {
            return Ok(gen_oas_server_kc::DisableUserDeviceResponse::NotFound(kc_error(
                "NOT_FOUND",
                "Device not found",
            )));
        };
        if row.status != "ACTIVE" {
            return Ok(
                gen_oas_server_kc::DisableUserDeviceResponse::DeviceCannotBeDisabledInItsCurrentState(
                    kc_error("INVALID_STATE", "Device cannot be disabled"),
                ),
            );
        }
        let updated = self
            .state
            .repository
            .update_device_status(&row.id, "REVOKED")
            .await
            .map_err(repo_err)?;
        Ok(gen_oas_server_kc::DisableUserDeviceResponse::DeviceDisabled(
            kc_map::DeviceRecordDto::from(updated).into(),
        ))
    }

    async fn enrollment_precheck(
        &self,
        enrollment_precheck_request: gen_oas_server_kc::models::EnrollmentPrecheckRequest,
        _idempotency_key: Option<String>,
        _context: &KcContext,
    ) -> std::result::Result<EnrollmentPrecheckResponse, ApiError> {
        let req: kc_map::EnrollmentPrecheckRequest = enrollment_precheck_request.into();
        let mut resp = gen_oas_server_kc::models::EnrollmentPrecheckResponse::new(
            gen_oas_server_kc::models::EnrollmentPrecheckResponseDecision::Allow,
        );

        let existing = self
            .state
            .repository
            .find_device_binding(&req.device_id, &req.jkt)
            .await
            .map_err(repo_err)?;
        if let Some((_record_id, user_id)) = existing {
            resp.decision = gen_oas_server_kc::models::EnrollmentPrecheckResponseDecision::Reject;
            resp.reason = Some("DEVICE_ALREADY_BOUND".to_owned());
            resp.bound_user_id = Some(user_id);
        }
        Ok(EnrollmentPrecheckResponse::PolicyDecision(resp))
    }

    async fn enrollment_bind(
        &self,
        enrollment_bind_request: gen_oas_server_kc::models::EnrollmentBindRequest,
        _idempotency_key: Option<String>,
        _context: &KcContext,
    ) -> std::result::Result<EnrollmentBindResponse, ApiError> {
        let req: kc_map::EnrollmentBindRequest = enrollment_bind_request.into();

        let existing = self
            .state
            .repository
            .find_device_binding(&req.device_id, &req.jkt)
            .await
            .map_err(repo_err)?;

        if let Some((device_record_id, bound_user_id)) = existing {
            if bound_user_id != req.user_id {
                return Ok(EnrollmentBindResponse::DeviceAlreadyBoundToADifferentUser(
                    kc_error("CONFLICT", "Device already bound"),
                ));
            }
            let mut resp = gen_oas_server_kc::models::EnrollmentBindResponse::new(
                gen_oas_server_kc::models::EnrollmentBindResponseStatus::AlreadyBound,
            );
            resp.device_record_id = Some(device_record_id);
            resp.bound_user_id = Some(bound_user_id);
            return Ok(EnrollmentBindResponse::Bound(resp));
        }

        let insert = self.state.repository.bind_device(&req).await;
        let device_record_id = match insert {
            Ok(id) => id,
            Err(RepoError::Conflict) => {
                let checked = self
                    .state
                    .repository
                    .find_device_binding(&req.device_id, &req.jkt)
                    .await
                    .map_err(repo_err)?;
                if let Some((existing_id, bound_user_id)) = checked {
                    if bound_user_id != req.user_id {
                        return Ok(EnrollmentBindResponse::DeviceAlreadyBoundToADifferentUser(
                            kc_error("CONFLICT", "Device already bound"),
                        ));
                    }
                    let mut resp = gen_oas_server_kc::models::EnrollmentBindResponse::new(
                        gen_oas_server_kc::models::EnrollmentBindResponseStatus::AlreadyBound,
                    );
                    resp.device_record_id = Some(existing_id);
                    resp.bound_user_id = Some(bound_user_id);
                    return Ok(EnrollmentBindResponse::Bound(resp));
                }
                return Err(ApiError("Device bind conflict".to_owned()));
            }
            Err(err) => return Err(repo_err(err)),
        };

        let mut resp = gen_oas_server_kc::models::EnrollmentBindResponse::new(
            gen_oas_server_kc::models::EnrollmentBindResponseStatus::Bound,
        );
        resp.device_record_id = Some(device_record_id);
        resp.bound_user_id = Some(req.user_id);
        Ok(EnrollmentBindResponse::Bound(resp))
    }

    async fn create_approval(
        &self,
        approval_create_request: gen_oas_server_kc::models::ApprovalCreateRequest,
        idempotency_key: Option<String>,
        _context: &KcContext,
    ) -> std::result::Result<CreateApprovalResponse, ApiError> {
        let req: kc_map::ApprovalCreateRequest = approval_create_request.into();
        let created: ApprovalCreated = match self
            .state
            .repository
            .create_approval(&req, idempotency_key)
            .await
        {
            Ok(created) => created,
            Err(RepoError::Conflict) => {
                return Ok(CreateApprovalResponse::Conflict(kc_error(
                    "CONFLICT",
                    "Duplicate idempotency key",
                )));
            }
            Err(err) => return Err(repo_err(err)),
        };

        let mut resp = gen_oas_server_kc::models::ApprovalCreateResponse::new(
            created.request_id,
            created
                .status
                .parse()
                .unwrap_or(gen_oas_server_kc::models::ApprovalCreateResponseStatus::Pending),
        );
        resp.expires_at = created.expires_at;
        Ok(CreateApprovalResponse::Created(resp))
    }

    async fn get_approval(
        &self,
        request_id: String,
        _context: &KcContext,
    ) -> std::result::Result<GetApprovalResponse, ApiError> {
        let row = self
            .state
            .repository
            .get_approval(&request_id)
            .await
            .map_err(repo_err)?;
        let Some(row) = row else {
            return Ok(GetApprovalResponse::NotFound(kc_error(
                "NOT_FOUND",
                "Approval not found",
            )));
        };
        Ok(GetApprovalResponse::Status(
            kc_map::ApprovalStatusDto::from(row).into(),
        ))
    }

    async fn list_user_approvals<'a>(
        &self,
        user_id: String,
        status: Option<&'a Vec<gen_oas_server_kc::models::ListUserApprovalsStatusParameterInner>>,
        _context: &KcContext,
    ) -> std::result::Result<ListUserApprovalsResponse, ApiError> {
        let statuses = status.map(|v| v.iter().map(ToString::to_string).collect::<Vec<_>>());
        let rows = self
            .state
            .repository
            .list_user_approvals(&user_id, statuses)
            .await
            .map_err(repo_err)?;
        let approvals = rows
            .into_iter()
            .map(kc_map::UserApprovalRecordDto::from)
            .map(Into::into)
            .collect::<Vec<_>>();
        Ok(ListUserApprovalsResponse::ApprovalList(
            gen_oas_server_kc::models::UserApprovalsResponse { user_id, approvals },
        ))
    }

    async fn decide_approval(
        &self,
        request_id: String,
        approval_decision_request: gen_oas_server_kc::models::ApprovalDecisionRequest,
        _context: &KcContext,
    ) -> std::result::Result<DecideApprovalResponse, ApiError> {
        let req: kc_map::ApprovalDecisionRequest = approval_decision_request.into();
        let row = self
            .state
            .repository
            .decide_approval(&request_id, &req)
            .await
            .map_err(repo_err)?;
        let Some(row) = row else {
            return Ok(DecideApprovalResponse::NotFound(kc_error(
                "NOT_FOUND",
                "Approval not found",
            )));
        };
        Ok(DecideApprovalResponse::UpdatedStatus(
            kc_map::ApprovalStatusDto::from(row).into(),
        ))
    }

    async fn cancel_approval(
        &self,
        request_id: String,
        _context: &KcContext,
    ) -> std::result::Result<CancelApprovalResponse, ApiError> {
        let affected = self
            .state
            .repository
            .cancel_approval(&request_id)
            .await
            .map_err(repo_err)?;
        if affected == 0 {
            return Ok(CancelApprovalResponse::NotFound(kc_error(
                "NOT_FOUND",
                "Approval not found",
            )));
        }
        Ok(CancelApprovalResponse::Cancelled)
    }

    async fn resolve_user_by_phone(
        &self,
        phone_resolve_request: gen_oas_server_kc::models::PhoneResolveRequest,
        _context: &KcContext,
    ) -> std::result::Result<ResolveUserByPhoneResponse, ApiError> {
        let phone = phone_resolve_request.phone_number;
        let realm = phone_resolve_request.realm;
        let user = self
            .state
            .repository
            .resolve_user_by_phone(&realm, &phone)
            .await
            .map_err(repo_err)?;

        let has_user = user.is_some();
        let has_device_credentials = if let Some(user) = &user {
            self.state
                .repository
                .count_user_devices(&user.user_id)
                .await
                .map_err(repo_err)?
                > 0
        } else {
            false
        };

        let mut resp = gen_oas_server_kc::models::PhoneResolveResponse::new(
            phone,
            has_user,
            has_device_credentials,
            gen_oas_server_kc::models::EnrollmentPath::Otp,
        );
        if let Some(user) = user {
            resp.user_id = Some(user.user_id);
            resp.username = Some(user.username);
        }
        Ok(ResolveUserByPhoneResponse::PhoneResolutionAndRoutingRecommendation(
            resp,
        ))
    }

    async fn resolve_or_create_user_by_phone(
        &self,
        phone_resolve_or_create_request: gen_oas_server_kc::models::PhoneResolveOrCreateRequest,
        _context: &KcContext,
    ) -> std::result::Result<ResolveOrCreateUserByPhoneResponse, ApiError> {
        let (user, created) = self
            .state
            .repository
            .resolve_or_create_user_by_phone(
                &phone_resolve_or_create_request.realm,
                &phone_resolve_or_create_request.phone_number,
            )
            .await
            .map_err(repo_err)?;

        let resp = gen_oas_server_kc::models::PhoneResolveOrCreateResponse::new(
            phone_resolve_or_create_request.phone_number,
            user.user_id,
            user.username,
            created,
        );
        Ok(ResolveOrCreateUserByPhoneResponse::UserResolvedOrCreated(resp))
    }

    async fn send_sms(
        &self,
        sms_send_request: gen_oas_server_kc::models::SmsSendRequest,
        _context: &KcContext,
    ) -> std::result::Result<SendSmsResponse, ApiError> {
        let req: kc_map::SmsSendRequest = sms_send_request.into();
        let ttl_seconds: i32 = 300;

        let mut hasher = Sha256::new();
        hasher.update(req.otp.as_bytes());
        let otp_sha256 = hasher.finalize().to_vec();
        let message = format!("Your verification code is: {}", req.otp);

        let queued = self
            .state
            .repository
            .queue_sms(SmsPendingInsert {
                realm: req.realm,
                client_id: req.client_id,
                user_id: req.user_id,
                phone_number: req.phone_number,
                otp_sha256,
                ttl_seconds,
                max_attempts: self.state.config.aws.sns.max_attempts as i32,
                metadata: serde_json::json!({ "message": message }),
            })
            .await
            .map_err(repo_err)?;

        let mut resp = gen_oas_server_kc::models::SmsSendResponse::new(queued.hash);
        resp.ttl_seconds = Some(queued.ttl_seconds);
        resp.status = Some(queued.status);
        Ok(SendSmsResponse::OTPQueued(resp))
    }

    async fn confirm_sms(
        &self,
        sms_confirm_request: gen_oas_server_kc::models::SmsConfirmRequest,
        _context: &KcContext,
    ) -> std::result::Result<ConfirmSmsResponse, ApiError> {
        let req: kc_map::SmsConfirmRequest = sms_confirm_request.into();
        let row = self
            .state
            .repository
            .get_sms_by_hash(&req.hash)
            .await
            .map_err(repo_err)?;
        let Some(row) = row else {
            let mut resp = gen_oas_server_kc::models::SmsConfirmResponse::new(false);
            resp.reason = Some("NOT_FOUND".to_owned());
            return Ok(ConfirmSmsResponse::ConfirmationResult(resp));
        };

        if let Some(ttl) = row.ttl_seconds {
            let expires_at = row.created_at + chrono::Duration::seconds(ttl as i64);
            if Utc::now() > expires_at {
                let mut resp = gen_oas_server_kc::models::SmsConfirmResponse::new(false);
                resp.reason = Some("EXPIRED".to_owned());
                return Ok(ConfirmSmsResponse::ConfirmationResult(resp));
            }
        }

        let mut hasher = Sha256::new();
        hasher.update(req.otp.as_bytes());
        let provided = hasher.finalize().to_vec();
        if provided != row.otp_sha256 {
            let mut resp = gen_oas_server_kc::models::SmsConfirmResponse::new(false);
            resp.reason = Some("INVALID_OTP".to_owned());
            return Ok(ConfirmSmsResponse::ConfirmationResult(resp));
        }

        self.state
            .repository
            .mark_sms_confirmed(&req.hash)
            .await
            .map_err(repo_err)?;

        Ok(ConfirmSmsResponse::ConfirmationResult(
            gen_oas_server_kc::models::SmsConfirmResponse::new(true),
        ))
    }
}
