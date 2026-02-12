use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

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
}

#[cfg(feature = "axum")]
mod axum_impl {
    use super::Error;
    use axum::{http::StatusCode, response::IntoResponse};

    impl IntoResponse for Error {
        fn into_response(self) -> axum::response::Response {
            let status_code = match self {
                Error::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
                Error::Yaml(_) => StatusCode::INTERNAL_SERVER_ERROR,
                Error::NotFound => StatusCode::NOT_FOUND,
                Error::Any(_) => StatusCode::INTERNAL_SERVER_ERROR,
                Error::AddrParseError(_) => StatusCode::INTERNAL_SERVER_ERROR,
                Error::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
                Error::Server(_) => StatusCode::INTERNAL_SERVER_ERROR,

                Error::SqlxError(_) => StatusCode::INTERNAL_SERVER_ERROR,
                Error::SqlxDataParser(_) => StatusCode::INTERNAL_SERVER_ERROR,
                Error::AwsS3PresignConfig(_) => StatusCode::INTERNAL_SERVER_ERROR,
                Error::AwsS3PutObject(_) => StatusCode::INTERNAL_SERVER_ERROR,
                Error::AwsSnsPublish(_) => StatusCode::INTERNAL_SERVER_ERROR,
            };

            (status_code, self.to_string()).into_response()
        }
    }
}
