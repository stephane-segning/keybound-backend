use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;
pub type AppResult<T> = Result<T, Error>;

#[derive(Debug, Clone, Serialize)]
pub struct ErrorPayload {
    pub error_key: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct ErrorMeta {
    pub error_key: &'static str,
    pub status_code: u16,
    pub message: String,
    pub context: Option<Value>,
}

impl ErrorMeta {
    pub fn payload(&self) -> ErrorPayload {
        ErrorPayload {
            error_key: self.error_key.to_owned(),
            message: self.message.clone(),
            context: self.context.clone(),
        }
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML parse error")]
    Yaml(#[from] serde_yaml::Error),

    #[error("Not found")]
    NotFound,

    #[error("Any: {0}")]
    Any(#[from] anyhow::Error),

    #[error("Server Error: {0}")]
    Server(String),

    #[error("Address parse error: {0}")]
    AddrParseError(#[from] std::net::AddrParseError),

    #[error("Database error: {0}")]
    Database(String),

    #[error("SQLx error: {0}")]
    SqlxError(#[from] sqlx::Error),

    #[error("SQLx-Data parser error: {0}")]
    SqlxDataParser(#[from] sqlx_data_parser::ParserError),

    #[error("S3 presign config error: {0}")]
    AwsS3PresignConfig(#[from] aws_sdk_s3::presigning::PresigningConfigError),

    #[error("S3 put object error: {0}")]
    AwsS3PutObject(
        #[from] aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::put_object::PutObjectError>,
    ),

    #[error("SNS publish error: {0}")]
    AwsSnsPublish(
        #[from] aws_sdk_sns::error::SdkError<aws_sdk_sns::operation::publish::PublishError>,
    ),

    #[error("{message}")]
    Http {
        error_key: &'static str,
        status_code: u16,
        message: String,
        context: Option<Value>,
    },
}

impl Error {
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Http {
            error_key: "UNAUTHORIZED",
            status_code: 401,
            message: message.into(),
            context: None,
        }
    }

    pub fn bad_request(error_key: &'static str, message: impl Into<String>) -> Self {
        Self::Http {
            error_key,
            status_code: 400,
            message: message.into(),
            context: None,
        }
    }

    pub fn not_found(error_key: &'static str, message: impl Into<String>) -> Self {
        Self::Http {
            error_key,
            status_code: 404,
            message: message.into(),
            context: None,
        }
    }

    pub fn conflict(error_key: &'static str, message: impl Into<String>) -> Self {
        Self::Http {
            error_key,
            status_code: 409,
            message: message.into(),
            context: None,
        }
    }

    pub fn internal(error_key: &'static str, message: impl Into<String>) -> Self {
        Self::Http {
            error_key,
            status_code: 500,
            message: message.into(),
            context: None,
        }
    }

    pub fn with_context(self, context: Value) -> Self {
        match self {
            Self::Http {
                error_key,
                status_code,
                message,
                ..
            } => Self::Http {
                error_key,
                status_code,
                message,
                context: Some(context),
            },
            other => other,
        }
    }

    pub fn meta(&self) -> ErrorMeta {
        match self {
            Self::NotFound => ErrorMeta {
                error_key: "NOT_FOUND",
                status_code: 404,
                message: self.to_string(),
                context: None,
            },
            Self::Database(_) | Self::SqlxError(_) | Self::SqlxDataParser(_) => ErrorMeta {
                error_key: "DATABASE_ERROR",
                status_code: 500,
                message: "Database operation failed".to_owned(),
                context: None,
            },
            Self::Http {
                error_key,
                status_code,
                message,
                context,
            } => ErrorMeta {
                error_key,
                status_code: *status_code,
                message: message.clone(),
                context: context.clone(),
            },
            _ => ErrorMeta {
                error_key: "INTERNAL_SERVER_ERROR",
                status_code: 500,
                message: "Internal server error".to_owned(),
                context: None,
            },
        }
    }
}

#[cfg(feature = "axum")]
mod axum_impl {
    use super::Error;
    use axum::{
        http::StatusCode,
        response::{IntoResponse, Response},
        Json,
    };

    impl IntoResponse for Error {
        fn into_response(self) -> Response {
            let meta = self.meta();
            let status =
                StatusCode::from_u16(meta.status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (status, Json(meta.payload())).into_response()
        }
    }
}
