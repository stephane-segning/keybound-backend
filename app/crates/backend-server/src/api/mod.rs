pub mod bff;
pub mod kc;
pub mod staff;
mod repro_422;

use crate::state::AppState;
use axum::response::IntoResponse;
use backend_auth::{ServiceContext, SignatureContext};
use backend_core::{AppResult, Error};
use http::{HeaderMap, HeaderValue, Request, header::AUTHORIZATION};
use std::sync::Arc;
use tracing::debug;

#[derive(Clone)]
pub struct BackendApi {
    pub(crate) state: Arc<AppState>,
}

impl AsRef<Self> for BackendApi {
    fn as_ref(&self) -> &Self {
        self
    }
}

impl BackendApi {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    pub(crate) fn require_user_id(context: &ServiceContext) -> AppResult<String> {
        context
            .user_id()
            .map(ToOwned::to_owned)
            .ok_or_else(|| Error::unauthorized("Missing bearer subject"))
    }

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
    type Claims = ServiceContext;

    async fn extract_claims_from_auth_header(
        &self,
        _kind: gen_oas_server_bff::apis::BasicAuthKind,
        headers: &HeaderMap,
        key: &str,
    ) -> Option<Self::Claims> {
        claims_from_header_key(headers, key)
    }
}

#[backend_core::async_trait]
impl gen_oas_server_kc::apis::ApiKeyAuthHeader for BackendApi {
    type Claims = SignatureContext;

    async fn extract_claims_from_header(&self, _: &HeaderMap, _: &str) -> Option<Self::Claims> {
        Some(SignatureContext {})
    }
}

#[backend_core::async_trait]
impl gen_oas_server_staff::apis::ApiAuthBasic for BackendApi {
    type Claims = ServiceContext;

    async fn extract_claims_from_auth_header(
        &self,
        _kind: gen_oas_server_staff::apis::BasicAuthKind,
        headers: &HeaderMap,
        key: &str,
    ) -> Option<Self::Claims> {
        claims_from_header_key(headers, key)
    }
}

fn claims_from_header_key(headers: &HeaderMap<HeaderValue>, key: &str) -> Option<ServiceContext> {
    let auth = headers
        .get(key)
        .or_else(|| headers.get(AUTHORIZATION))?
        .clone();
    let mut req = Request::new(());
    req.headers_mut().insert(AUTHORIZATION, auth);
    let ctx = ServiceContext::from_request(&req);
    debug!(
        has_user_id = ctx.user_id().is_some(),
        "constructed auth claims from header"
    );
    Some(ctx)
}
