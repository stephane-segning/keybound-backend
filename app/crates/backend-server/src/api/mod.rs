pub mod bff;
pub mod kc;
pub mod staff;

use crate::state::AppState;
use backend_auth::ServiceContext;
use backend_core::Error;
use http::{header::AUTHORIZATION, HeaderMap, HeaderValue, Request};
use std::sync::Arc;
use swagger::ApiError;
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

    pub(crate) fn require_user_id(context: &ServiceContext) -> std::result::Result<String, ApiError> {
        context
            .user_id()
            .map(ToOwned::to_owned)
            .ok_or_else(|| ApiError("Missing bearer subject".to_owned()))
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

pub(crate) fn repo_err(err: Error) -> ApiError {
    ApiError(err.to_string())
}

pub(crate) fn is_unique_violation(err: &Error) -> bool {
    matches!(
        err,
        Error::SqlxError(sqlx::Error::Database(db_err)) if db_err.code().as_deref() == Some("23505")
    )
}

impl gen_oas_server_bff::apis::ErrorHandler<()> for BackendApi {}
impl gen_oas_server_kc::apis::ErrorHandler<()> for BackendApi {}
impl gen_oas_server_staff::apis::ErrorHandler<()> for BackendApi {}

#[backend_core::async_trait]
impl gen_oas_server_bff::apis::ApiAuthBasic for BackendApi {
    type Claims = ServiceContext;

    async fn extract_claims_from_auth_header(
        &self,
        _kind: gen_oas_server_bff::apis::BasicAuthKind,
        headers: &axum::http::header::HeaderMap,
        key: &str,
    ) -> Option<Self::Claims> {
        claims_from_header_key(headers, key)
    }
}

#[backend_core::async_trait]
impl gen_oas_server_staff::apis::ApiAuthBasic for BackendApi {
    type Claims = ServiceContext;

    async fn extract_claims_from_auth_header(
        &self,
        _kind: gen_oas_server_staff::apis::BasicAuthKind,
        headers: &axum::http::header::HeaderMap,
        key: &str,
    ) -> Option<Self::Claims> {
        claims_from_header_key(headers, key)
    }
}

fn claims_from_header_key(headers: &HeaderMap<HeaderValue>, key: &str) -> Option<ServiceContext> {
    let auth = headers.get(key).or_else(|| headers.get(AUTHORIZATION))?.clone();
    let mut req = Request::new(());
    req.headers_mut().insert(AUTHORIZATION, auth);
    let ctx = ServiceContext::from_request(&req);
    debug!(has_user_id = ctx.user_id().is_some(), "constructed auth claims from header");
    Some(ctx)
}
