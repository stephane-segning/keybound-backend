pub mod auth;
pub mod bff_flow;
pub mod bff_uploads;
mod date_deserialization_regression;
pub mod kc;
pub mod staff_flow;

use crate::state::AppState;
use axum::response::IntoResponse;
use backend_auth::{JwtToken, OidcState, SignatureContext, SignatureState};
use backend_core::{AppResult, Error};
use http::HeaderMap;
use std::sync::Arc;
use tracing::{debug, instrument};

pub(crate) const BFF_AUTH_USER_ID_HEADER: &str = "x-bff-authenticated-user-id";
pub(crate) const BFF_AUTH_DEVICE_ID_HEADER: &str = "x-bff-authenticated-device-id";

#[derive(Debug, Clone)]
pub struct BffSignatureClaims {
    pub user_id: String,
    pub device_id: String,
}

#[derive(Clone)]
pub struct BackendApi {
    pub(crate) state: Arc<AppState>,
    pub(crate) oidc_state: Arc<OidcState>,
    pub(crate) signature_state: Arc<SignatureState>,
}

impl AsRef<Self> for BackendApi {
    fn as_ref(&self) -> &Self {
        self
    }
}

impl BackendApi {
    pub fn new(
        state: Arc<AppState>,
        oidc_state: Arc<OidcState>,
        signature_state: Arc<SignatureState>,
    ) -> Self {
        Self {
            state,
            oidc_state,
            signature_state,
        }
    }

    pub(crate) fn require_bff_claims(&self, headers: &HeaderMap) -> AppResult<BffSignatureClaims> {
        if !self.state.config.bff.enabled {
            debug!("BFF auth disabled, returning mock claims");
            return Ok(BffSignatureClaims {
                user_id: "usr_auth_disabled".to_owned(),
                device_id: "dvc_auth_disabled".to_owned(),
            });
        }

        Self::extract_bff_claims(headers).ok_or_else(|| {
            debug!("Missing BFF claims in headers");
            Error::unauthorized("Missing signature-authenticated BFF claims")
        })
    }

    pub(crate) fn extract_bff_claims(headers: &HeaderMap) -> Option<BffSignatureClaims> {
        let user_id = headers
            .get(BFF_AUTH_USER_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned)?;
        let device_id = headers
            .get(BFF_AUTH_DEVICE_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned)?;

        Some(BffSignatureClaims { user_id, device_id })
    }

    #[instrument(skip(context))]
    #[allow(dead_code)]
    pub(crate) fn require_user_id(context: &JwtToken) -> AppResult<String> {
        Ok(context.user_id().to_owned())
    }
}

pub(crate) fn kc_error(code: &str, message: &str) -> gen_oas_server_kc::models::Error {
    gen_oas_server_kc::models::Error::new(code.to_owned(), message.to_owned())
}

#[backend_core::async_trait]
impl gen_oas_server_kc::apis::ErrorHandler<Error> for BackendApi {
    #[instrument(skip(self, error))]
    async fn handle_error(
        &self,
        _method: &::http::Method,
        _host: &headers::Host,
        _cookies: &axum_extra::extract::CookieJar,
        error: Error,
    ) -> Result<axum::response::Response, http::StatusCode> {
        Ok(error.into_response())
    }
}

#[backend_core::async_trait]
impl gen_oas_server_kc::apis::ApiKeyAuthHeader for BackendApi {
    type Claims = SignatureContext;

    #[instrument(skip(self, _headers))]
    async fn extract_claims_from_header(
        &self,
        _headers: &HeaderMap,
        _key: &str,
    ) -> Option<Self::Claims> {
        Some(SignatureContext {})
    }
}
