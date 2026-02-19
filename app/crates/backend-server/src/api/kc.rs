use super::{kc_error, BackendApi};
use axum_extra::extract::CookieJar;
use backend_auth::SignatureContext;
use backend_core::Error;
use backend_model::kc::{
    DeviceRecordDto
    , UserRecordDto, UserSearch, UserUpsert,
};
use backend_repository::{DeviceRepo, UserRepo};
use gen_oas_server_kc::apis::devices::{Devices, LookupDeviceResponse};
use gen_oas_server_kc::apis::enrollment::{Enrollment, EnrollmentBindResponse};
use gen_oas_server_kc::apis::users::{
    CreateUserResponse, DeleteUserResponse, GetUserResponse, SearchUsersResponse,
    UpdateUserResponse, Users,
};
use gen_oas_server_kc::models;
use headers::Host;
use http::Method;

#[backend_core::async_trait]
impl Devices<Error> for BackendApi {
    type Claims = SignatureContext;

    async fn lookup_device(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        body: &models::DeviceLookupRequest,
    ) -> Result<LookupDeviceResponse, Error> {
        let req = backend_model::kc::DeviceLookupRequest {
            device_id: body.device_id.clone(),
            jkt: body.jkt.clone(),
        };

        self.state
            .device
            .lookup_device(&req)
            .await
            .map(|res| match res {
                Some(row) => {
                    let user_id = row.user_id.clone();
                    let public_jwk: Option<
                        std::collections::HashMap<String, gen_oas_server_kc::types::Object>,
                    > = serde_json::from_str(&row.public_jwk).ok();
                    let dto = DeviceRecordDto::from(row);
                    LookupDeviceResponse::Status200_LookupResult(models::DeviceLookupResponse {
                        device: Some(dto.into()),
                        found: true,
                        public_jwk,
                        user_id: Some(user_id),
                    })
                }
                None => LookupDeviceResponse::Status404_NotFound(kc_error(
                    "NOT_FOUND",
                    "Device not found",
                )),
            })
            .map_err(Into::into)
    }
}

#[backend_core::async_trait]
impl Users<Error> for BackendApi {
    type Claims = SignatureContext;

    async fn create_user(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        body: &models::UserUpsertRequest,
    ) -> Result<CreateUserResponse, Error> {
        let req = UserUpsert::from(body.clone());
        self.state
            .user
            .create_user(&req)
            .await
            .map(|row| {
                let dto = UserRecordDto::from(row);
                CreateUserResponse::Status201_Created(dto.into())
            })
            .map_err(Into::into)
    }

    async fn delete_user(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        path_params: &models::DeleteUserPathParams,
    ) -> Result<DeleteUserResponse, Error> {
        self.state
            .user
            .delete_user(&path_params.user_id)
            .await
            .map(|count| {
                if count > 0 {
                    DeleteUserResponse::Status204_Deleted
                } else {
                    DeleteUserResponse::Status404_NotFound(kc_error("NOT_FOUND", "User not found"))
                }
            })
            .map_err(Into::into)
    }

    async fn get_user(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        path_params: &models::GetUserPathParams,
    ) -> Result<GetUserResponse, Error> {
        self.state
            .user
            .get_user(&path_params.user_id)
            .await
            .map(|res| match res {
                Some(row) => {
                    let dto = UserRecordDto::from(row);
                    GetUserResponse::Status200_User(dto.into())
                }
                None => {
                    GetUserResponse::Status404_NotFound(kc_error("NOT_FOUND", "User not found"))
                }
            })
            .map_err(Into::into)
    }

    async fn search_users(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        body: &models::UserSearchRequest,
    ) -> Result<SearchUsersResponse, Error> {
        let req = UserSearch::from(body.clone());
        self.state
            .user
            .search_users(&req)
            .await
            .map(|rows| {
                let users = rows
                    .into_iter()
                    .map(|row| UserRecordDto::from(row).into())
                    .collect();
                SearchUsersResponse::Status200_SearchResults(models::UserSearchResponse {
                    users,
                    total_count: None,
                })
            })
            .map_err(Into::into)
    }

    async fn update_user(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        path_params: &models::UpdateUserPathParams,
        body: &models::UserUpsertRequest,
    ) -> Result<UpdateUserResponse, Error> {
        let req = UserUpsert::from(body.clone());
        self.state
            .user
            .update_user(&path_params.user_id, &req)
            .await
            .map(|res| match res {
                Some(row) => {
                    let dto = UserRecordDto::from(row);
                    UpdateUserResponse::Status200_Updated(dto.into())
                }
                None => {
                    UpdateUserResponse::Status404_NotFound(kc_error("NOT_FOUND", "User not found"))
                }
            })
            .map_err(Into::into)
    }
}

#[backend_core::async_trait]
impl Enrollment<Error> for BackendApi {
    type Claims = SignatureContext;

    async fn enrollment_bind(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        _header_params: &models::EnrollmentBindHeaderParams,
        body: &models::EnrollmentBindRequest,
    ) -> Result<EnrollmentBindResponse, Error> {
        let req = backend_model::kc::EnrollmentBindRequest::from(body.clone());

        // Check if device is already bound to someone else
        let existing = self
            .state
            .device
            .find_device_binding(&req.device_id, &req.jkt)
            .await?;

        if let Some((bound_user_id, _)) = existing {
            if bound_user_id != req.user_id {
                return Ok(
                    EnrollmentBindResponse::Status409_DeviceAlreadyBoundToADifferentUser(kc_error(
                        "CONFLICT",
                        "Device already bound to another user",
                    )),
                );
            }
        }

        self.state
            .device
            .bind_device(&req)
            .await
            .map(|record_id| {
                EnrollmentBindResponse::Status200_Bound(models::EnrollmentBindResponse {
                    status: models::EnrollmentBindResponseStatus::Bound,
                    device_record_id: Some(record_id),
                    bound_user_id: Some(req.user_id),
                })
            })
            .map_err(Into::into)
    }
}
