pub mod bff;
pub mod kc;
pub mod staff;

use crate::state::AppState;
use backend_auth::ServiceContext;
use backend_core::Error;
use gen_oas_server_bff::apis::ErrorHandler;
use std::sync::Arc;
use swagger::ApiError;

#[derive(Clone)]
pub struct BackendApi {
    pub(crate) state: Arc<AppState>,
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

impl ErrorHandler<()> for BackendApi {}
