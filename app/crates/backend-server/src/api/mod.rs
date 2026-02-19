pub mod bff;
pub mod kc;
mod repro_422;
pub mod staff;

use crate::state::AppState;
use axum::response::IntoResponse;
use backend_auth::{JwtToken, OidcState, SignatureContext, SignatureState};
use backend_core::{AppResult, Error};
use http::{HeaderMap, HeaderValue};
use std::sync::Arc;
use tracing::{debug, instrument};

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

    #[instrument(skip(context))]
    pub(crate) fn require_user_id(context: &JwtToken) -> AppResult<String> {
        Ok(context.user_id().to_owned())
    }

    #[instrument]
    pub(crate) fn normalize_page_limit(page: Option<i32>, limit: Option<i32>) -> (i32, i32) {
        let page = page.unwrap_or(1).max(1);
        let limit = limit.unwrap_or(20).clamp(1, 100);
        (page, limit)
    }
}

pub(crate) fn kc_error(code: &str, message: &str) -> gen_oas_server_kc::models::Error {
    gen_oas_server_kc::models::Error::new(code.to_owned(), message.to_owned())
}

#[backend_core::async_trait]
impl gen_oas_server_bff::apis::ErrorHandler<Error> for BackendApi {
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
impl gen_oas_server_staff::apis::ErrorHandler<Error> for BackendApi {
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
impl gen_oas_server_bff::apis::ApiAuthBasic for BackendApi {
    type Claims = JwtToken;

    #[instrument(skip(self, headers))]
    async fn extract_claims_from_auth_header(
        &self,
        _kind: gen_oas_server_bff::apis::BasicAuthKind,
        headers: &HeaderMap,
        key: &str,
    ) -> Option<Self::Claims> {
        claims_from_header_key(headers, key, self.oidc_state.clone()).await
    }
}

#[backend_core::async_trait]
impl gen_oas_server_kc::apis::ApiKeyAuthHeader for BackendApi {
    type Claims = SignatureContext;

    #[instrument(skip(self, _headers))]
    async fn extract_claims_from_header(&self, _headers: &HeaderMap, _key: &str) -> Option<Self::Claims> {
        Some(SignatureContext {})
    }
}

#[backend_core::async_trait]
impl gen_oas_server_staff::apis::ApiAuthBasic for BackendApi {
    type Claims = JwtToken;

    #[instrument(skip(self, headers))]
    async fn extract_claims_from_auth_header(
        &self,
        _kind: gen_oas_server_staff::apis::BasicAuthKind,
        headers: &HeaderMap,
        key: &str,
    ) -> Option<Self::Claims> {
        claims_from_header_key(headers, key, self.oidc_state.clone()).await
    }
}

#[instrument(skip(oidc_state))]
async fn claims_from_header_key(
    headers: &HeaderMap<HeaderValue>,
    key: &str,
    oidc_state: Arc<OidcState>,
) -> Option<JwtToken> {
    let auth_header = headers.get(key)?;
    let auth_str = auth_header.to_str().ok()?;
    if !auth_str.to_lowercase().starts_with("bearer ") {
        return None;
    }
    let token = &auth_str[7..];

    match JwtToken::verify(token, &oidc_state).await {
        Ok(jwt) => Some(jwt),
        Err(e) => {
            debug!(error = %e, "JWT token verification failed");
            None
        }
    }
}
