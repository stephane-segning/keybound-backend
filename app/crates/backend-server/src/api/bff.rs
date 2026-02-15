use super::BackendApi;
use backend_auth::ServiceContext;
use backend_core::Error;
use backend_repository::{KycDocumentInsert, KycRepo};
use gen_oas_server_bff::apis::kyc::{
    ApiKycCasesMineGetResponse, ApiKycCasesMineOpenSubmissionPostResponse,
    ApiKycDocumentsDocumentIdConfirmPostResponse,
    ApiKycSubmissionsSubmissionIdDocumentsInitPostResponse, ApiKycSubmissionsSubmissionIdGetResponse,
    ApiKycSubmissionsSubmissionIdSubmitPostResponse, ApiRegistrationKycProfilePatchResponse, Kyc,
};
use gen_oas_server_bff::apis::limits::{ApiLimitsGetResponse, Limits};
use gen_oas_server_bff::models;
use gen_oas_server_bff::types::Nullable;
use http::Method;
use serde_json::{Map, Value, json};

#[backend_core::async_trait]
impl Kyc<Error> for BackendApi {
    type Claims = ServiceContext;

    async fn api_kyc_cases_mine_get(
        &self,
        _method: &Method,
        _host: &headers::Host,
        _cookies: &axum_extra::extract::CookieJar,
        claims: &Self::Claims,
    ) -> Result<ApiKycCasesMineGetResponse, Error> {
        let user_id = Self::require_user_id(claims)?;
        let profile = self.state.kyc.get_kyc_profile(&user_id).await?;

        match profile {
            Some(p) => Ok(ApiKycCasesMineGetResponse::Status200_KYCCase {
                body: Self::kyc_case_from_profile(p),
                e_tag: None,
            }),
            None => Ok(ApiKycCasesMineGetResponse::Status404_NotFound(Self::not_found_problem(
                "KYC case not found",
            ))),
        }
    }

    async fn api_kyc_cases_mine_open_submission_post(
        &self,
        _method: &Method,
        _host: &headers::Host,
        _cookies: &axum_extra::extract::CookieJar,
        claims: &Self::Claims,
        _body: &Option<models::OpenSubmissionRequest>,
    ) -> Result<ApiKycCasesMineOpenSubmissionPostResponse, Error> {
        let user_id = Self::require_user_id(claims)?;
        let profile = self.state.kyc.get_kyc_profile(&user_id).await?;

        match profile {
            Some(p) => Ok(ApiKycCasesMineOpenSubmissionPostResponse::Status201_NewSubmissionOpened(
                Self::kyc_case_from_profile(p),
            )),
            None => Ok(ApiKycCasesMineOpenSubmissionPostResponse::Status401_Unauthorized(
                Self::unauthorized_problem(),
            )),
        }
    }

    async fn api_kyc_documents_document_id_confirm_post(
        &self,
        _method: &Method,
        _host: &headers::Host,
        _cookies: &axum_extra::extract::CookieJar,
        _claims: &Self::Claims,
        _header_params: &models::ApiKycDocumentsDocumentIdConfirmPostHeaderParams,
        _path_params: &models::ApiKycDocumentsDocumentIdConfirmPostPathParams,
        _body: &models::ConfirmDocumentUploadRequest,
    ) -> Result<ApiKycDocumentsDocumentIdConfirmPostResponse, Error> {
        Err(Error::internal(
            "NOT_IMPLEMENTED",
            "Endpoint not implemented",
        ))
    }

    async fn api_kyc_submissions_submission_id_documents_init_post(
        &self,
        _method: &Method,
        _host: &headers::Host,
        _cookies: &axum_extra::extract::CookieJar,
        claims: &Self::Claims,
        _header_params: &models::ApiKycSubmissionsSubmissionIdDocumentsInitPostHeaderParams,
        _path_params: &models::ApiKycSubmissionsSubmissionIdDocumentsInitPostPathParams,
        body: &models::InitDocumentUploadRequest,
    ) -> Result<ApiKycSubmissionsSubmissionIdDocumentsInitPostResponse, Error> {
        let user_id = Self::require_user_id(claims)?;
        self.state.kyc.ensure_kyc_profile(&user_id).await?;

        let input = KycDocumentInsert {
            external_id: user_id,
            document_type: body.document_type.to_string(),
            file_name: body.file_name.clone(),
            mime_type: body.mime_type.clone(),
            content_length: body
                .size_bytes
                .as_ref()
                .and_then(|size| match size {
                    Nullable::Present(value) => Some(i64::from(*value)),
                    Nullable::Null => None,
                })
                .unwrap_or(0),
            s3_bucket: "azamra-kyc".to_owned(),
            s3_key: "temp/key".to_owned(),
            presigned_expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
        };

        let row = self.state.kyc.insert_kyc_document_intent(input).await?;

        Ok(ApiKycSubmissionsSubmissionIdDocumentsInitPostResponse::Status201_UploadInitialized(
            models::InitDocumentUploadResponse {
                document_id: row.id,
                upload_url: "https://s3.example.com/upload".to_owned(),
                expires_at: row.presigned_expires_at,
                required_headers: None,
            },
        ))
    }

    async fn api_kyc_submissions_submission_id_get(
        &self,
        _method: &Method,
        _host: &headers::Host,
        _cookies: &axum_extra::extract::CookieJar,
        claims: &Self::Claims,
        _path_params: &models::ApiKycSubmissionsSubmissionIdGetPathParams,
    ) -> Result<ApiKycSubmissionsSubmissionIdGetResponse, Error> {
        let user_id = Self::require_user_id(claims)?;
        let profile = self.state.kyc.get_kyc_profile(&user_id).await?;

        match profile {
            Some(p) => Ok(ApiKycSubmissionsSubmissionIdGetResponse::Status200_SubmissionDetail(
                Self::kyc_submission_detail_from_profile(p),
            )),
            None => Ok(ApiKycSubmissionsSubmissionIdGetResponse::Status404_NotFound(
                Self::not_found_problem("KYC submission not found"),
            )),
        }
    }

    async fn api_kyc_submissions_submission_id_submit_post(
        &self,
        _method: &Method,
        _host: &headers::Host,
        _cookies: &axum_extra::extract::CookieJar,
        claims: &Self::Claims,
        _header_params: &models::ApiKycSubmissionsSubmissionIdSubmitPostHeaderParams,
        _path_params: &models::ApiKycSubmissionsSubmissionIdSubmitPostPathParams,
    ) -> Result<ApiKycSubmissionsSubmissionIdSubmitPostResponse, Error> {
        let user_id = Self::require_user_id(claims)?;
        let profile = self.state.kyc.get_kyc_profile(&user_id).await?;

        match profile {
            Some(p) => Ok(ApiKycSubmissionsSubmissionIdSubmitPostResponse::Status200_SubmissionSubmitted(
                Self::kyc_submission_detail_from_profile(p),
            )),
            None => Ok(ApiKycSubmissionsSubmissionIdSubmitPostResponse::Status404_NotFound(
                Self::not_found_problem("KYC submission not found"),
            )),
        }
    }

    async fn api_registration_kyc_profile_patch(
        &self,
        _method: &Method,
        _host: &headers::Host,
        _cookies: &axum_extra::extract::CookieJar,
        claims: &Self::Claims,
        header_params: &models::ApiRegistrationKycProfilePatchHeaderParams,
        body: &Vec<models::JsonPatchOperation>,
    ) -> Result<ApiRegistrationKycProfilePatchResponse, Error> {
        let user_id = Self::require_user_id(claims)?;
        self.state.kyc.ensure_kyc_profile(&user_id).await?;

        let current_profile = self
            .state
            .kyc
            .get_kyc_profile(&user_id)
            .await?
            .ok_or_else(|| Error::not_found("KYC_PROFILE_NOT_FOUND", "KYC profile not found"))?;

        // Optimistic concurrency check
        let expected_version = header_params
            .if_match
            .as_ref()
            .and_then(|etag: &String| etag.trim_matches('"').parse::<i32>().ok());

        if let Some(expected) = expected_version {
            if current_profile.version != expected {
                return Ok(
                    ApiRegistrationKycProfilePatchResponse::Status412_ETagMismatch(
                        Self::precondition_failed_problem("ETag mismatch"),
                    ),
                );
            }
        }

        let mut target = Self::profile_target_from_row(&current_profile);

        let operations = body
            .iter()
            .map(Self::json_patch_op_from_model)
            .collect::<Result<Vec<_>, Error>>()?;

        json_patch::patch(&mut target, &json_patch::Patch(operations))
            .map_err(|error| Error::bad_request("INVALID_JSON_PATCH", error.to_string()))?;

        let req = Self::kyc_information_patch_request_from_target(&target)?;
        let updated = self
            .state
            .kyc
            .patch_kyc_profile(&user_id, expected_version, &req)
            .await?;

        match updated {
            Some(p) => Ok(
                ApiRegistrationKycProfilePatchResponse::Status200_ProfileUpdated(
                    Self::kyc_case_from_profile(p),
                ),
            ),
            None => {
                // If update returned None but profile existed, it means version mismatch race
                Ok(
                    ApiRegistrationKycProfilePatchResponse::Status412_ETagMismatch(
                        Self::precondition_failed_problem("Concurrent modification detected"),
                    ),
                )
            }
        }
    }
}

#[backend_core::async_trait]
impl Limits<Error> for BackendApi {
    type Claims = ServiceContext;

    async fn api_limits_get(
        &self,
        _method: &Method,
        _host: &headers::Host,
        _cookies: &axum_extra::extract::CookieJar,
        claims: &Self::Claims,
    ) -> Result<ApiLimitsGetResponse, Error> {
        let user_id = Self::require_user_id(claims)?;
        let tier = self.state.kyc.get_kyc_tier(&user_id).await?;

        match tier {
            Some(value) => {
                let limits = models::LimitsResponse {
                    kyc_tier: Some(value),
                    tier_name: Some(format!("Tier {value}")),
                    currency: Some("EUR".to_owned()),
                    effective_at: None,
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
                    restricted_features: Some(vec![]),
                };
                Ok(ApiLimitsGetResponse::Status200_LimitsAndUsageDetails(limits))
            }
            None => Ok(ApiLimitsGetResponse::Status404_NotFound(Self::not_found_problem(
                "Customer not found",
            ))),
        }
    }
}

impl BackendApi {
    fn not_found_problem(detail: &str) -> models::ProblemDetails {
        models::ProblemDetails {
            r_type: None,
            title: "Not found".to_owned(),
            status: 404,
            detail: Some(detail.to_owned()),
            instance: None,
            code: None,
            trace_id: None,
            r_errors: None,
        }
    }

    fn unauthorized_problem() -> models::ProblemDetails {
        models::ProblemDetails {
            r_type: None,
            title: "Unauthorized".to_owned(),
            status: 401,
            detail: Some("Unauthorized".to_owned()),
            instance: None,
            code: None,
            trace_id: None,
            r_errors: None,
        }
    }

    fn precondition_failed_problem(detail: &str) -> models::ProblemDetails {
        models::ProblemDetails {
            r_type: None,
            title: "Precondition Failed".to_owned(),
            status: 412,
            detail: Some(detail.to_owned()),
            instance: None,
            code: Some("PRECONDITION_FAILED".to_owned()),
            trace_id: None,
            r_errors: None,
        }
    }

    fn profile_target_from_row(profile: &backend_model::db::KycProfileRow) -> Value {
        let mut user_profile = Map::new();
        user_profile.insert(
            "firstName".to_owned(),
            profile
                .first_name
                .as_ref()
                .map(|value| json!(value))
                .unwrap_or(Value::Null),
        );
        user_profile.insert(
            "lastName".to_owned(),
            profile
                .last_name
                .as_ref()
                .map(|value| json!(value))
                .unwrap_or(Value::Null),
        );
        user_profile.insert(
            "email".to_owned(),
            profile
                .email
                .as_ref()
                .map(|value| json!(value))
                .unwrap_or(Value::Null),
        );
        user_profile.insert(
            "phoneNumber".to_owned(),
            profile
                .phone_number
                .as_ref()
                .map(|value| json!(value))
                .unwrap_or(Value::Null),
        );
        user_profile.insert(
            "dateOfBirth".to_owned(),
            profile
                .date_of_birth
                .as_ref()
                .map(|value| json!(value))
                .unwrap_or(Value::Null),
        );
        user_profile.insert(
            "nationality".to_owned(),
            profile
                .nationality
                .as_ref()
                .map(|value| json!(value))
                .unwrap_or(Value::Null),
        );

        let mut root = Map::new();
        root.insert("externalId".to_owned(), json!(profile.external_id));
        root.insert("userProfile".to_owned(), Value::Object(user_profile));
        Value::Object(root)
    }

    fn json_patch_op_from_model(
        operation: &models::JsonPatchOperation,
    ) -> Result<json_patch::PatchOperation, Error> {
        let value = operation.value.as_ref().map(|nullable| match nullable {
            Nullable::Present(object) => object.0.clone(),
            Nullable::Null => Value::Null,
        });

        match operation.op.as_str() {
            "move" | "copy" => {
                let from = operation.from.clone().ok_or_else(|| {
                    Error::bad_request(
                        "INVALID_JSON_PATCH",
                        format!("{} operation requires 'from'", operation.op),
                    )
                })?;

                let patch_value = json!({
                    "op": operation.op,
                    "path": operation.path,
                    "from": from,
                });

                serde_json::from_value(patch_value)
                    .map_err(|error| Error::bad_request("INVALID_JSON_PATCH", error.to_string()))
            }
            "add" | "replace" | "test" => {
                let patch_value = json!({
                    "op": operation.op,
                    "path": operation.path,
                    "value": value.unwrap_or(Value::Null),
                });

                serde_json::from_value(patch_value)
                    .map_err(|error| Error::bad_request("INVALID_JSON_PATCH", error.to_string()))
            }
            "remove" => {
                let patch_value = json!({
                    "op": operation.op,
                    "path": operation.path,
                });

                serde_json::from_value(patch_value)
                    .map_err(|error| Error::bad_request("INVALID_JSON_PATCH", error.to_string()))
            }
            other => Err(Error::bad_request(
                "INVALID_JSON_PATCH",
                format!("Unsupported JSON patch op: {other}"),
            )),
        }
    }

    fn kyc_information_patch_request_from_target(
        target: &Value,
    ) -> Result<backend_model::bff::KycInformationPatchRequest, Error> {
        let user_profile = target
            .get("userProfile")
            .and_then(Value::as_object)
            .ok_or_else(|| Error::bad_request("INVALID_JSON_PATCH", "Missing /userProfile object"))?;

        Ok(backend_model::bff::KycInformationPatchRequest {
            first_name: user_profile
                .get("firstName")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            last_name: user_profile
                .get("lastName")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            email: user_profile
                .get("email")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            phone_number: user_profile
                .get("phoneNumber")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            date_of_birth: user_profile
                .get("dateOfBirth")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            nationality: user_profile
                .get("nationality")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        })
    }

    fn kyc_case_from_profile(profile: backend_model::db::KycProfileRow) -> models::KycCaseResponse {
        models::KycCaseResponse {
            case_id: format!("kyc_{}", profile.external_id),
            case_status: models::KycCaseStatus::Open,
            current_tier: Some(profile.kyc_tier as u8),
            active_submission: models::KycSubmissionSummary {
                submission_id: format!("sub_{}", profile.external_id),
                version: 1,
                status: profile
                    .kyc_status
                    .parse()
                    .unwrap_or(models::KycSubmissionStatus::Draft),
                requested_tier: Some(profile.kyc_tier.max(1) as u8),
                decided_tier: None,
                submitted_at: Some(match profile.submitted_at {
                    Some(value) => Nullable::Present(value),
                    None => Nullable::Null,
                }),
                decided_at: Some(match profile.reviewed_at {
                    Some(value) => Nullable::Present(value),
                    None => Nullable::Null,
                }),
                provisioning_status: Some(models::KycProvisioningStatus::None),
                next_action: Some(models::KycNextAction::FixProfile),
            },
        }
    }

    fn kyc_submission_detail_from_profile(
        profile: backend_model::db::KycProfileRow,
    ) -> models::KycSubmissionDetailResponse {
        models::KycSubmissionDetailResponse {
            submission_id: format!("sub_{}", profile.external_id),
            version: 1,
            status: profile
                .kyc_status
                .parse()
                .unwrap_or(models::KycSubmissionStatus::Draft),
            user_profile: models::UserProfile {
                first_name: profile.first_name,
                last_name: profile.last_name,
                email: profile.email,
                phone_number: profile.phone_number,
                date_of_birth: profile.date_of_birth.and_then(|value| value.parse().ok()),
                nationality: profile.nationality,
                address: None,
            },
            documents: vec![],
            staff_messages: None,
        }
    }
}
