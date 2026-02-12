use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use aws_sdk_s3::presigning::PresigningConfig;
use backend_core::Error;
use backend_model::{bff as bff_map, kc as kc_map, staff as staff_map};
use backend_repository::{ApprovalCreated, KycDocumentInsert, SmsPendingInsert};
use chrono::Utc;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::state::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/registration/kyc/documents", post(api_registration_kyc_documents_post))
        .route("/api/registration/kyc/status", get(api_registration_kyc_status_get))
        .route("/api/registration/limits", get(api_registration_limits_get))
        .route("/api/kyc/staff/submissions", get(api_kyc_staff_submissions_get))
        .route(
            "/api/kyc/staff/submissions/{external_id}",
            get(api_kyc_staff_submissions_external_id_get),
        )
        .route(
            "/api/kyc/staff/submissions/{external_id}/approve",
            post(api_kyc_staff_submissions_external_id_approve_post),
        )
        .route(
            "/api/kyc/staff/submissions/{external_id}/reject",
            post(api_kyc_staff_submissions_external_id_reject_post),
        )
        .route(
            "/api/kyc/staff/submissions/{external_id}/request-info",
            post(api_kyc_staff_submissions_external_id_request_info_post),
        )
        .route("/v1/approvals", post(create_approval))
        .route(
            "/v1/approvals/{request_id}",
            get(get_approval).delete(cancel_approval),
        )
        .route("/v1/approvals/{request_id}/decision", post(decide_approval))
        .route("/v1/users/{user_id}/approvals", get(list_user_approvals))
        .route("/v1/devices/lookup", post(lookup_device))
        .route("/v1/users/{user_id}/devices", get(list_user_devices))
        .route(
            "/v1/users/{user_id}/devices/{device_id}/disable",
            post(disable_user_device),
        )
        .route("/v1/sms/confirm", post(confirm_sms))
        .route("/v1/enrollments/bind", post(enrollment_bind))
        .route("/v1/enrollments/precheck", post(enrollment_precheck))
        .route(
            "/v1/enrollments/phone/resolve-or-create",
            post(resolve_or_create_user_by_phone),
        )
        .route("/v1/enrollments/phone/resolve", post(resolve_user_by_phone))
        .route("/v1/sms/send", post(send_sms))
        .route("/v1/users", post(create_user))
        .route("/v1/users:search", post(search_users))
        .route("/v1/users/{user_id}", delete(delete_user).get(get_user).put(update_user))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct PaginationQuery {
    page: Option<i32>,
    limit: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct StaffSubmissionsQuery {
    status: Option<String>,
    search: Option<String>,
    page: Option<i32>,
    limit: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct ListUserApprovalsQuery {
    status: Option<Vec<gen_oas_server_kc::models::ListUserApprovalsStatusParameterInner>>,
}

#[derive(Debug, Deserialize)]
struct IncludeRevokedQuery {
    include_revoked: Option<bool>,
}

fn normalize_page_limit(page: Option<i32>, limit: Option<i32>) -> (i32, i32) {
    let page = page.unwrap_or(1).max(1);
    let limit = limit.unwrap_or(20).clamp(1, 100);
    (page, limit)
}

fn require_external_id(headers: &HeaderMap) -> Result<String, Error> {
    let external_id = headers
        .get("x-external-id")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
        .ok_or_else(|| Error::Server("Missing X-External-Id".to_owned()))?;
    Ok(external_id)
}

fn idempotency_key(headers: &HeaderMap) -> Option<String> {
    headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

fn kc_error(code: &str, message: &str) -> gen_oas_server_kc::models::Error {
    gen_oas_server_kc::models::Error::new(code.to_owned(), message.to_owned())
}

fn is_unique_violation(err: &Error) -> bool {
    matches!(
        err,
        Error::SqlxError(sqlx::Error::Database(db_err)) if db_err.code().as_deref() == Some("23505")
    )
}

fn json_with_status<T: serde::Serialize>(status: StatusCode, payload: T) -> Response {
    (status, Json(payload)).into_response()
}

async fn api_registration_kyc_documents_post(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(kyc_document_upload_request): Json<gen_oas_server_bff::models::KycDocumentUploadRequest>,
) -> Result<Response, Error> {
    let external_id = require_external_id(&headers)?;
    let req: bff_map::KycDocumentUploadRequest = kyc_document_upload_request.into();

    let now = Utc::now();
    let expires_at = now + chrono::Duration::seconds(state.config.aws.s3.presign_ttl_seconds as i64);
    let object_id = backend_id::prefixed("obj")?;
    let s3_key = format!("kyc/{external_id}/{object_id}/{}", req.file_name);

    state.service.ensure_kyc_profile(&external_id).await?;
    let doc_row = state
        .service
        .insert_kyc_document_intent(KycDocumentInsert {
            external_id: external_id.clone(),
            document_type: req.document_type.clone(),
            file_name: req.file_name.clone(),
            mime_type: req.mime_type.clone(),
            content_length: req.content_length,
            s3_bucket: state.config.aws.s3.bucket.clone(),
            s3_key: s3_key.clone(),
            presigned_expires_at: expires_at,
        })
        .await?;

    let presign_cfg =
        PresigningConfig::expires_in(Duration::from_secs(state.config.aws.s3.presign_ttl_seconds))?;

    let presigned = state
        .s3
        .put_object()
        .bucket(&state.config.aws.s3.bucket)
        .key(&s3_key)
        .content_type(req.mime_type)
        .content_length(req.content_length)
        .presigned(presign_cfg)
        .await?;

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

    state.invalidate_bff_cache(&external_id);

    let response: gen_oas_server_bff::models::KycDocumentUploadResponse = dto.into();
    Ok(json_with_status(StatusCode::CREATED, response))
}

async fn api_registration_kyc_status_get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<PaginationQuery>,
) -> Result<Response, Error> {
    let external_id = require_external_id(&headers)?;
    let (page, limit) = normalize_page_limit(query.page, query.limit);
    let use_default_cache = page == 1 && limit == 20;

    if use_default_cache {
        if let Some(cached) = state.get_kyc_status_cache(&external_id) {
            return Ok(Json(cached).into_response());
        }
    }

    let Some(profile) = state.service.get_kyc_profile(&external_id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    let docs = state
        .service
        .list_kyc_documents(&external_id, page, limit)
        .await?;
    let total_documents = docs.total_items as i32;

    let documents = docs
        .data
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
        page: Some(page),
        page_size: Some(limit),
        total_documents: Some(total_documents),
    };

    let response: gen_oas_server_bff::models::KycStatusResponse = dto.into();
    if use_default_cache {
        state.put_kyc_status_cache(external_id, response.clone());
    }

    Ok(Json(response).into_response())
}

async fn api_registration_limits_get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, Error> {
    let external_id = require_external_id(&headers)?;
    if let Some(cached) = state.get_limits_cache(&external_id) {
        return Ok(Json(cached).into_response());
    }

    let Some(kyc_tier) = state.service.get_kyc_tier(&external_id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    let mut resp = gen_oas_server_bff::models::LimitsResponse::new();
    resp.kyc_tier = Some(kyc_tier);
    resp.tier_name = Some(
        match kyc_tier {
            0 => "TIER_0",
            1 => "TIER_1",
            2 => "TIER_2",
            _ => "TIER_UNKNOWN",
        }
        .to_owned(),
    );
    resp.currency = Some("USD".to_owned());
    resp.allowed_payment_methods = Some(vec!["CARD".to_owned(), "BANK_TRANSFER".to_owned()]);
    resp.restricted_features = Some(vec![]);

    state.put_limits_cache(external_id, resp.clone());
    Ok(Json(resp).into_response())
}

async fn api_kyc_staff_submissions_get(
    State(state): State<Arc<AppState>>,
    Query(query): Query<StaffSubmissionsQuery>,
) -> Result<Json<gen_oas_server_staff::models::KycSubmissionsResponse>, Error> {
    let data = state
        .service
        .list_kyc_submissions(query.status, query.search, query.page.unwrap_or(1), query.limit.unwrap_or(20))
        .await?;

    let items = data
        .data
        .into_iter()
        .map(staff_map::KycSubmissionSummaryDto::from)
        .map(Into::into)
        .collect::<Vec<_>>();

    let dto = staff_map::KycSubmissionsResponseDto {
        items: Some(items),
        total: Some(data.total_items as i32),
        page: Some(data.page as i32),
        page_size: Some(data.size as i32),
    };

    Ok(Json(dto.into()))
}

async fn api_kyc_staff_submissions_external_id_get(
    State(state): State<Arc<AppState>>,
    Path(external_id): Path<String>,
    Query(query): Query<PaginationQuery>,
) -> Result<Response, Error> {
    let (page, limit) = normalize_page_limit(query.page, query.limit);

    let Some(profile) = state.service.get_kyc_submission(&external_id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    let docs = state
        .service
        .list_kyc_documents(&external_id, page, limit)
        .await?;

    let mut dto = staff_map::KycSubmissionDetailResponseDto::from_profile(profile);
    dto.documents = Some(
        docs.data
            .into_iter()
            .map(staff_map::KycDocumentDto::from)
            .map(Into::into)
            .collect(),
    );
    dto.page = Some(page);
    dto.page_size = Some(limit);
    dto.total_documents = Some(docs.total_items as i32);

    let response: gen_oas_server_staff::models::KycSubmissionDetailResponse = dto.into();
    Ok(Json(response).into_response())
}

async fn api_kyc_staff_submissions_external_id_approve_post(
    State(state): State<Arc<AppState>>,
    Path(external_id): Path<String>,
    Json(kyc_approval_request): Json<gen_oas_server_staff::models::KycApprovalRequest>,
) -> Result<Response, Error> {
    let req: staff_map::KycApprovalRequest = kyc_approval_request.into();
    let updated = state.service.update_kyc_approved(&external_id, &req).await?;
    if !updated {
        return Ok(StatusCode::UNPROCESSABLE_ENTITY.into_response());
    }
    state.invalidate_bff_cache(&external_id);
    Ok(StatusCode::OK.into_response())
}

async fn api_kyc_staff_submissions_external_id_reject_post(
    State(state): State<Arc<AppState>>,
    Path(external_id): Path<String>,
    Json(kyc_rejection_request): Json<gen_oas_server_staff::models::KycRejectionRequest>,
) -> Result<Response, Error> {
    let req: staff_map::KycRejectionRequest = kyc_rejection_request.into();
    let updated = state.service.update_kyc_rejected(&external_id, &req).await?;
    if !updated {
        return Ok(StatusCode::UNPROCESSABLE_ENTITY.into_response());
    }
    state.invalidate_bff_cache(&external_id);
    Ok(StatusCode::OK.into_response())
}

async fn api_kyc_staff_submissions_external_id_request_info_post(
    State(state): State<Arc<AppState>>,
    Path(external_id): Path<String>,
    Json(kyc_request_info_request): Json<gen_oas_server_staff::models::KycRequestInfoRequest>,
) -> Result<Response, Error> {
    let req: staff_map::KycRequestInfoRequest = kyc_request_info_request.into();
    let updated = state
        .service
        .update_kyc_request_info(&external_id, &req)
        .await?;
    if !updated {
        return Ok(StatusCode::UNPROCESSABLE_ENTITY.into_response());
    }
    state.invalidate_bff_cache(&external_id);
    Ok(StatusCode::OK.into_response())
}

async fn create_approval(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(approval_create_request): Json<gen_oas_server_kc::models::ApprovalCreateRequest>,
) -> Result<Response, Error> {
    let req: kc_map::ApprovalCreateRequest = approval_create_request.into();
    let created: ApprovalCreated = match state
        .service
        .create_approval(&req, idempotency_key(&headers))
        .await
    {
        Ok(created) => created,
        Err(err) if is_unique_violation(&err) => {
            return Ok(json_with_status(
                StatusCode::CONFLICT,
                kc_error("CONFLICT", "Duplicate idempotency key"),
            ));
        }
        Err(err) => return Err(err),
    };

    let mut resp = gen_oas_server_kc::models::ApprovalCreateResponse::new(
        created.request_id,
        created
            .status
            .parse()
            .unwrap_or(gen_oas_server_kc::models::ApprovalCreateResponseStatus::Pending),
    );
    resp.expires_at = created.expires_at;
    Ok(json_with_status(StatusCode::CREATED, resp))
}

async fn cancel_approval(
    State(state): State<Arc<AppState>>,
    Path(request_id): Path<String>,
) -> Result<Response, Error> {
    let affected = state.service.cancel_approval(&request_id).await?;
    if affected == 0 {
        return Ok(json_with_status(
            StatusCode::NOT_FOUND,
            kc_error("NOT_FOUND", "Approval not found"),
        ));
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn decide_approval(
    State(state): State<Arc<AppState>>,
    Path(request_id): Path<String>,
    Json(approval_decision_request): Json<gen_oas_server_kc::models::ApprovalDecisionRequest>,
) -> Result<Response, Error> {
    let req: kc_map::ApprovalDecisionRequest = approval_decision_request.into();
    let Some(row) = state.service.decide_approval(&request_id, &req).await? else {
        return Ok(json_with_status(
            StatusCode::NOT_FOUND,
            kc_error("NOT_FOUND", "Approval not found"),
        ));
    };
    let response: gen_oas_server_kc::models::ApprovalStatusResponse =
        kc_map::ApprovalStatusDto::from(row).into();
    Ok(Json(response).into_response())
}

async fn get_approval(
    State(state): State<Arc<AppState>>,
    Path(request_id): Path<String>,
) -> Result<Response, Error> {
    let Some(row) = state.service.get_approval(&request_id).await? else {
        return Ok(json_with_status(
            StatusCode::NOT_FOUND,
            kc_error("NOT_FOUND", "Approval not found"),
        ));
    };
    let response: gen_oas_server_kc::models::ApprovalStatusResponse =
        kc_map::ApprovalStatusDto::from(row).into();
    Ok(Json(response).into_response())
}

async fn list_user_approvals(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
    Query(query): Query<ListUserApprovalsQuery>,
) -> Result<Json<gen_oas_server_kc::models::UserApprovalsResponse>, Error> {
    let statuses = query
        .status
        .map(|v| v.into_iter().map(|value| value.to_string()).collect::<Vec<_>>());
    let rows = state.service.list_user_approvals(&user_id, statuses).await?;
    let approvals = rows
        .into_iter()
        .map(kc_map::UserApprovalRecordDto::from)
        .map(Into::into)
        .collect::<Vec<_>>();
    Ok(Json(gen_oas_server_kc::models::UserApprovalsResponse {
        user_id,
        approvals,
    }))
}

async fn lookup_device(
    State(state): State<Arc<AppState>>,
    Json(device_lookup_request): Json<gen_oas_server_kc::models::DeviceLookupRequest>,
) -> Result<Response, Error> {
    let req: kc_map::DeviceLookupRequest = device_lookup_request.into();
    if req.device_id.is_none() && req.jkt.is_none() {
        return Ok(json_with_status(
            StatusCode::BAD_REQUEST,
            kc_error("BAD_REQUEST", "device_id or jkt must be set"),
        ));
    }

    let Some(row) = state.service.lookup_device(&req).await? else {
        return Ok(json_with_status(
            StatusCode::NOT_FOUND,
            kc_error("NOT_FOUND", "Not found"),
        ));
    };

    let public_jwk = match &row.public_jwk {
        serde_json::Value::Object(map) => Some(map.clone().into_iter().collect()),
        _ => None,
    };

    let mut resp = gen_oas_server_kc::models::DeviceLookupResponse::new(true);
    resp.user_id = Some(row.user_id.clone());
    resp.device = Some(kc_map::DeviceRecordDto::from(row).into());
    resp.public_jwk = public_jwk;
    Ok(Json(resp).into_response())
}

async fn list_user_devices(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
    Query(query): Query<IncludeRevokedQuery>,
) -> Result<Json<gen_oas_server_kc::models::UserDevicesResponse>, Error> {
    let rows = state
        .service
        .list_user_devices(&user_id, query.include_revoked.unwrap_or(false))
        .await?;
    let devices = rows
        .into_iter()
        .map(kc_map::DeviceRecordDto::from)
        .map(Into::into)
        .collect::<Vec<_>>();
    Ok(Json(gen_oas_server_kc::models::UserDevicesResponse {
        user_id,
        devices,
    }))
}

async fn disable_user_device(
    State(state): State<Arc<AppState>>,
    Path((user_id, device_id)): Path<(String, String)>,
) -> Result<Response, Error> {
    let Some(row) = state.service.get_user_device(&user_id, &device_id).await? else {
        return Ok(json_with_status(
            StatusCode::NOT_FOUND,
            kc_error("NOT_FOUND", "Device not found"),
        ));
    };

    if row.status != "ACTIVE" {
        return Ok(json_with_status(
            StatusCode::CONFLICT,
            kc_error("INVALID_STATE", "Device cannot be disabled"),
        ));
    }

    let updated = state.service.update_device_status(&row.id, "REVOKED").await?;
    let response: gen_oas_server_kc::models::DeviceRecord =
        kc_map::DeviceRecordDto::from(updated).into();
    Ok(Json(response).into_response())
}

async fn confirm_sms(
    State(state): State<Arc<AppState>>,
    Json(sms_confirm_request): Json<gen_oas_server_kc::models::SmsConfirmRequest>,
) -> Result<Json<gen_oas_server_kc::models::SmsConfirmResponse>, Error> {
    let req: kc_map::SmsConfirmRequest = sms_confirm_request.into();
    let row = state.service.get_sms_by_hash(&req.hash).await?;
    let Some(row) = row else {
        let mut resp = gen_oas_server_kc::models::SmsConfirmResponse::new(false);
        resp.reason = Some("NOT_FOUND".to_owned());
        return Ok(Json(resp));
    };

    if let Some(ttl) = row.ttl_seconds {
        let expires_at = row.created_at + chrono::Duration::seconds(ttl as i64);
        if Utc::now() > expires_at {
            let mut resp = gen_oas_server_kc::models::SmsConfirmResponse::new(false);
            resp.reason = Some("EXPIRED".to_owned());
            return Ok(Json(resp));
        }
    }

    let mut hasher = Sha256::new();
    hasher.update(req.otp.as_bytes());
    let provided = hasher.finalize().to_vec();
    if provided != row.otp_sha256 {
        let mut resp = gen_oas_server_kc::models::SmsConfirmResponse::new(false);
        resp.reason = Some("INVALID_OTP".to_owned());
        return Ok(Json(resp));
    }

    state.service.mark_sms_confirmed(&req.hash).await?;
    Ok(Json(gen_oas_server_kc::models::SmsConfirmResponse::new(true)))
}

async fn enrollment_bind(
    State(state): State<Arc<AppState>>,
    Json(enrollment_bind_request): Json<gen_oas_server_kc::models::EnrollmentBindRequest>,
) -> Result<Response, Error> {
    let req: kc_map::EnrollmentBindRequest = enrollment_bind_request.into();

    let existing = state
        .service
        .find_device_binding(&req.device_id, &req.jkt)
        .await?;
    if let Some((device_record_id, bound_user_id)) = existing {
        if bound_user_id != req.user_id {
            return Ok(json_with_status(
                StatusCode::CONFLICT,
                kc_error("CONFLICT", "Device already bound"),
            ));
        }

        let mut resp = gen_oas_server_kc::models::EnrollmentBindResponse::new(
            gen_oas_server_kc::models::EnrollmentBindResponseStatus::AlreadyBound,
        );
        resp.device_record_id = Some(device_record_id);
        resp.bound_user_id = Some(bound_user_id);
        return Ok(Json(resp).into_response());
    }

    let device_record_id = match state.service.bind_device(&req).await {
        Ok(id) => id,
        Err(err) if is_unique_violation(&err) => {
            let checked = state
                .service
                .find_device_binding(&req.device_id, &req.jkt)
                .await?;
            if let Some((existing_id, bound_user_id)) = checked {
                if bound_user_id != req.user_id {
                    return Ok(json_with_status(
                        StatusCode::CONFLICT,
                        kc_error("CONFLICT", "Device already bound"),
                    ));
                }

                let mut resp = gen_oas_server_kc::models::EnrollmentBindResponse::new(
                    gen_oas_server_kc::models::EnrollmentBindResponseStatus::AlreadyBound,
                );
                resp.device_record_id = Some(existing_id);
                resp.bound_user_id = Some(bound_user_id);
                return Ok(Json(resp).into_response());
            }
            return Ok(json_with_status(
                StatusCode::CONFLICT,
                kc_error("CONFLICT", "Device bind conflict"),
            ));
        }
        Err(err) => return Err(err),
    };

    let mut resp = gen_oas_server_kc::models::EnrollmentBindResponse::new(
        gen_oas_server_kc::models::EnrollmentBindResponseStatus::Bound,
    );
    resp.device_record_id = Some(device_record_id);
    resp.bound_user_id = Some(req.user_id);
    Ok(Json(resp).into_response())
}

async fn enrollment_precheck(
    State(state): State<Arc<AppState>>,
    Json(enrollment_precheck_request): Json<gen_oas_server_kc::models::EnrollmentPrecheckRequest>,
) -> Result<Json<gen_oas_server_kc::models::EnrollmentPrecheckResponse>, Error> {
    let req: kc_map::EnrollmentPrecheckRequest = enrollment_precheck_request.into();
    let mut resp = gen_oas_server_kc::models::EnrollmentPrecheckResponse::new(
        gen_oas_server_kc::models::EnrollmentPrecheckResponseDecision::Allow,
    );

    let existing = state
        .service
        .find_device_binding(&req.device_id, &req.jkt)
        .await?;
    if let Some((_record_id, user_id)) = existing {
        resp.decision = gen_oas_server_kc::models::EnrollmentPrecheckResponseDecision::Reject;
        resp.reason = Some("DEVICE_ALREADY_BOUND".to_owned());
        resp.bound_user_id = Some(user_id);
    }

    Ok(Json(resp))
}

async fn resolve_or_create_user_by_phone(
    State(state): State<Arc<AppState>>,
    Json(phone_resolve_or_create_request): Json<gen_oas_server_kc::models::PhoneResolveOrCreateRequest>,
) -> Result<Json<gen_oas_server_kc::models::PhoneResolveOrCreateResponse>, Error> {
    let (user, created) = state
        .service
        .resolve_or_create_user_by_phone(
            &phone_resolve_or_create_request.realm,
            &phone_resolve_or_create_request.phone_number,
        )
        .await?;

    let resp = gen_oas_server_kc::models::PhoneResolveOrCreateResponse::new(
        phone_resolve_or_create_request.phone_number,
        user.user_id,
        user.username,
        created,
    );
    Ok(Json(resp))
}

async fn resolve_user_by_phone(
    State(state): State<Arc<AppState>>,
    Json(phone_resolve_request): Json<gen_oas_server_kc::models::PhoneResolveRequest>,
) -> Result<Json<gen_oas_server_kc::models::PhoneResolveResponse>, Error> {
    let phone = phone_resolve_request.phone_number;
    let realm = phone_resolve_request.realm;
    let user = state.service.resolve_user_by_phone(&realm, &phone).await?;

    let has_user = user.is_some();
    let has_device_credentials = if let Some(user) = &user {
        state.service.count_user_devices(&user.user_id).await? > 0
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
    Ok(Json(resp))
}

async fn send_sms(
    State(state): State<Arc<AppState>>,
    Json(sms_send_request): Json<gen_oas_server_kc::models::SmsSendRequest>,
) -> Result<Json<gen_oas_server_kc::models::SmsSendResponse>, Error> {
    let req: kc_map::SmsSendRequest = sms_send_request.into();
    let ttl_seconds: i32 = 300;

    let mut hasher = Sha256::new();
    hasher.update(req.otp.as_bytes());
    let otp_sha256 = hasher.finalize().to_vec();
    let message = format!("Your verification code is: {}", req.otp);

    let queued = state
        .service
        .queue_sms(SmsPendingInsert {
            realm: req.realm,
            client_id: req.client_id,
            user_id: req.user_id,
            phone_number: req.phone_number,
            otp_sha256,
            ttl_seconds,
            max_attempts: state.config.aws.sns.max_attempts as i32,
            metadata: serde_json::json!({ "message": message }),
        })
        .await?;

    let mut resp = gen_oas_server_kc::models::SmsSendResponse::new(queued.hash);
    resp.ttl_seconds = Some(queued.ttl_seconds);
    resp.status = Some(queued.status);
    Ok(Json(resp))
}

async fn create_user(
    State(state): State<Arc<AppState>>,
    Json(user_upsert_request): Json<gen_oas_server_kc::models::UserUpsertRequest>,
) -> Result<Response, Error> {
    let req: kc_map::UserUpsert = user_upsert_request.into();
    match state.service.create_user(&req).await {
        Ok(row) => {
            let response: gen_oas_server_kc::models::UserRecord =
                kc_map::UserRecordDto::from(row).into();
            Ok(json_with_status(StatusCode::CREATED, response))
        }
        Err(err) if is_unique_violation(&err) => Ok(json_with_status(
            StatusCode::CONFLICT,
            kc_error("CONFLICT", "User already exists"),
        )),
        Err(err) => Err(err),
    }
}

async fn search_users(
    State(state): State<Arc<AppState>>,
    Json(user_search_request): Json<gen_oas_server_kc::models::UserSearchRequest>,
) -> Result<Json<gen_oas_server_kc::models::UserSearchResponse>, Error> {
    let req: kc_map::UserSearch = user_search_request.into();
    let users = state.service.search_users(&req).await?;
    let out_users = users
        .into_iter()
        .map(kc_map::UserRecordDto::from)
        .map(Into::into)
        .collect::<Vec<_>>();
    let total_count = out_users.len() as i32;
    Ok(Json(gen_oas_server_kc::models::UserSearchResponse {
        users: out_users,
        total_count: Some(total_count),
    }))
}

async fn delete_user(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
) -> Result<Response, Error> {
    let affected = state.service.delete_user(&user_id).await?;
    if affected == 0 {
        return Ok(json_with_status(
            StatusCode::NOT_FOUND,
            kc_error("NOT_FOUND", "User not found"),
        ));
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn get_user(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
) -> Result<Response, Error> {
    let Some(row) = state.service.get_user(&user_id).await? else {
        return Ok(json_with_status(
            StatusCode::NOT_FOUND,
            kc_error("NOT_FOUND", "User not found"),
        ));
    };
    let response: gen_oas_server_kc::models::UserRecord = kc_map::UserRecordDto::from(row).into();
    Ok(Json(response).into_response())
}

async fn update_user(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
    Json(user_upsert_request): Json<gen_oas_server_kc::models::UserUpsertRequest>,
) -> Result<Response, Error> {
    let req: kc_map::UserUpsert = user_upsert_request.into();
    let Some(row) = state.service.update_user(&user_id, &req).await? else {
        return Ok(json_with_status(
            StatusCode::NOT_FOUND,
            kc_error("NOT_FOUND", "User not found"),
        ));
    };
    let response: gen_oas_server_kc::models::UserRecord = kc_map::UserRecordDto::from(row).into();
    Ok(Json(response).into_response())
}
