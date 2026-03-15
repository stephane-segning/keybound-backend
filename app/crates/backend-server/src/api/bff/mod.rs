mod deposit_flow;
mod email_flow;
mod phone_flow;
mod session_flow;
mod shared;
mod step_flow;
mod upload_flow;
mod user_flow;

use super::BackendApi;
use axum_extra::extract::CookieJar;
use backend_auth::JwtToken;
use backend_core::Error;
use gen_oas_server_bff::apis::deposits::{
    Deposits, InternalCreatePhoneDepositRequestResponse, InternalGetPhoneDepositRequestResponse,
};
use gen_oas_server_bff::apis::email_magic::{
    EmailMagic, InternalCreateEmailMagicStepResponse, InternalIssueMagicEmailChallengeResponse,
    InternalVerifyMagicEmailChallengeResponse,
};
use gen_oas_server_bff::apis::phone_otp::{
    InternalCreatePhoneOtpStepResponse, InternalIssuePhoneOtpChallengeResponse,
    InternalVerifyPhoneOtpChallengeResponse, PhoneOtp,
};
use gen_oas_server_bff::apis::sessions::{
    InternalCreateKycSessionResponse, InternalGetKycSessionResponse,
    InternalListKycSessionsResponse, Sessions,
};
use gen_oas_server_bff::apis::steps::{InternalGetKycStepResponse, Steps};
use gen_oas_server_bff::apis::uploads::{
    InternalCompleteUploadResponse, InternalPresignUploadResponse, Uploads,
};
use gen_oas_server_bff::apis::users::{
    InternalGetUserByIdResponse, InternalGetUserKycLevelResponse,
    InternalGetUserKycSummaryResponse, Users,
};
use gen_oas_server_bff::models;
use headers::Host;
use http::Method;

#[backend_core::async_trait]
impl Sessions<Error> for BackendApi {
    type Claims = JwtToken;

    async fn internal_create_kyc_session(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::CreateKycSessionRequest,
    ) -> Result<InternalCreateKycSessionResponse, Error> {
        self.create_kyc_session_flow(claims, body).await
    }

    async fn internal_get_kyc_session(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        path_params: &models::InternalGetKycSessionPathParams,
    ) -> Result<InternalGetKycSessionResponse, Error> {
        self.get_kyc_session_flow(claims, path_params).await
    }

    async fn internal_list_kyc_sessions(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        query_params: &models::InternalListKycSessionsQueryParams,
    ) -> Result<InternalListKycSessionsResponse, Error> {
        self.list_kyc_sessions_flow(claims, query_params).await
    }
}

#[backend_core::async_trait]
impl Steps<Error> for BackendApi {
    type Claims = JwtToken;

    async fn internal_get_kyc_step(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        path_params: &models::InternalGetKycStepPathParams,
    ) -> Result<InternalGetKycStepResponse, Error> {
        self.get_kyc_step_flow(claims, path_params).await
    }
}

#[backend_core::async_trait]
impl PhoneOtp<Error> for BackendApi {
    type Claims = JwtToken;

    async fn internal_create_phone_otp_step(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::CreateCaseStepRequest,
    ) -> Result<InternalCreatePhoneOtpStepResponse, Error> {
        self.create_phone_otp_step_flow(claims, body).await
    }

    async fn internal_issue_phone_otp_challenge(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::IssuePhoneOtpRequest,
    ) -> Result<InternalIssuePhoneOtpChallengeResponse, Error> {
        self.issue_phone_otp_challenge_flow(claims, body).await
    }

    async fn internal_verify_phone_otp_challenge(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::VerifyPhoneOtpRequest,
    ) -> Result<InternalVerifyPhoneOtpChallengeResponse, Error> {
        self.verify_phone_otp_challenge_flow(claims, body).await
    }
}

#[backend_core::async_trait]
impl EmailMagic<Error> for BackendApi {
    type Claims = JwtToken;

    async fn internal_create_email_magic_step(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::CreateCaseStepRequest,
    ) -> Result<InternalCreateEmailMagicStepResponse, Error> {
        self.create_email_magic_step_flow(claims, body).await
    }

    async fn internal_issue_magic_email_challenge(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::IssueMagicEmailRequest,
    ) -> Result<InternalIssueMagicEmailChallengeResponse, Error> {
        self.issue_magic_email_challenge_flow(claims, body).await
    }

    async fn internal_verify_magic_email_challenge(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::VerifyMagicEmailRequest,
    ) -> Result<InternalVerifyMagicEmailChallengeResponse, Error> {
        self.verify_magic_email_challenge_flow(claims, body).await
    }
}

#[backend_core::async_trait]
impl Deposits<Error> for BackendApi {
    type Claims = JwtToken;

    async fn internal_create_phone_deposit_request(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::CreatePhoneDepositRequest,
    ) -> Result<InternalCreatePhoneDepositRequestResponse, Error> {
        self.create_phone_deposit_request_flow(claims, body).await
    }

    async fn internal_get_phone_deposit_request(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        path_params: &models::InternalGetPhoneDepositRequestPathParams,
    ) -> Result<InternalGetPhoneDepositRequestResponse, Error> {
        self.get_phone_deposit_request_flow(claims, path_params)
            .await
    }
}

#[backend_core::async_trait]
impl Uploads<Error> for BackendApi {
    type Claims = JwtToken;

    async fn internal_presign_upload(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::InternalPresignRequest,
    ) -> Result<InternalPresignUploadResponse, Error> {
        self.presign_upload_flow(claims, body).await
    }

    async fn internal_complete_upload(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        body: &models::InternalCompleteUploadRequest,
    ) -> Result<InternalCompleteUploadResponse, Error> {
        self.complete_upload_flow(claims, body).await
    }
}

#[backend_core::async_trait]
impl Users<Error> for BackendApi {
    type Claims = JwtToken;

    async fn internal_get_user_by_id(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        path_params: &models::InternalGetUserByIdPathParams,
    ) -> Result<InternalGetUserByIdResponse, Error> {
        self.get_user_by_id_flow(claims, path_params).await
    }

    async fn internal_get_user_kyc_level(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        path_params: &models::InternalGetUserKycLevelPathParams,
    ) -> Result<InternalGetUserKycLevelResponse, Error> {
        self.get_user_kyc_level_flow(claims, path_params).await
    }

    async fn internal_get_user_kyc_summary(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        path_params: &models::InternalGetUserKycSummaryPathParams,
    ) -> Result<InternalGetUserKycSummaryResponse, Error> {
        self.get_user_kyc_summary_flow(claims, path_params).await
    }
}
