//! Object storage abstraction layer for S3-compatible services.
//!
//! Provides a unified interface for file operations using S3-compatible storage
//! (AWS S3, MinIO, etc.). Supports presigned URLs, encryption modes, and basic
//! object operations.

use backend_core::Error;
use bytes::Bytes;
use std::collections::HashMap;
use std::time::Duration;

/// Presigned upload information returned by the storage provider.
///
/// Contains the URL and required headers for uploading files via presigned URLs.
#[derive(Debug, Clone)]
pub struct PresignedUpload {
    /// The presigned URL for uploading
    pub url: String,
    /// Required headers to include with the upload request
    pub headers: HashMap<String, String>,
}

/// Server-side encryption modes for stored objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionMode {
    /// No server-side encryption
    None,
    /// S3-managed encryption (AES-256)
    S3,
    /// AWS KMS-managed encryption
    Kms,
}

/// Storage trait for S3-compatible object storage operations.
///
/// This trait abstracts S3 operations and is implemented for different storage backends.
/// It provides methods for uploading, downloading, and generating presigned URLs.
#[cfg_attr(any(test, feature = "test-utils"), mockall::automock)]
#[backend_core::async_trait]
pub trait ObjectStorage: Send + Sync {
    /// Checks if an object exists in the bucket.
    ///
    /// # Arguments
    /// * `bucket` - S3 bucket name
    /// * `key` - Object key/path
    ///
    /// # Returns
    /// `Result<()>` - Ok if object exists, Err otherwise
    async fn head_object(&self, bucket: &str, key: &str) -> Result<(), Error>;

    /// Uploads an object directly to storage.
    ///
    /// # Arguments
    /// * `bucket` - S3 bucket name
    /// * `key` - Object key/path
    /// * `mime_type` - MIME type of the content
    /// * `encryption` - Server-side encryption mode
    /// * `body` - Object content as bytes
    ///
    /// # Returns
    /// `Result<()>` indicating success or error
    async fn upload(
        &self,
        bucket: &str,
        key: &str,
        mime_type: &str,
        encryption: EncryptionMode,
        body: Bytes,
    ) -> Result<(), Error>;

    /// Generates a presigned URL for uploading an object.
    ///
    /// # Arguments
    /// * `bucket` - S3 bucket name
    /// * `key` - Object key/path
    /// * `mime_type` - MIME type of the content
    /// * `encryption` - Server-side encryption mode
    /// * `expires_in` - URL expiration duration
    ///
    /// # Returns
    /// `Result<PresignedUpload>` containing the URL and required headers
    async fn upload_presigned(
        &self,
        bucket: &str,
        key: &str,
        mime_type: &str,
        encryption: EncryptionMode,
        expires_in: Duration,
    ) -> Result<PresignedUpload, Error>;

    /// Downloads an object directly from storage.
    ///
    /// # Arguments
    /// * `bucket` - S3 bucket name  
    /// * `key` - Object key/path
    ///
    /// # Returns
    /// `Result<Bytes>` containing the object content
    async fn download(&self, bucket: &str, key: &str) -> Result<Bytes, Error>;

    /// Generates a presigned URL for downloading an object.
    ///
    /// # Arguments
    /// * `bucket` - S3 bucket name
    /// * `key` - Object key/path
    /// * `expires_in` - URL expiration duration
    /// * `content_disposition` - Optional Content-Disposition header value
    ///
    /// # Returns
    /// `Result<String>` containing the presigned download URL
    async fn download_presigned(
        &self,
        bucket: &str,
        key: &str,
        expires_in: Duration,
        content_disposition: Option<String>,
    ) -> Result<String, Error>;
}

/// S3-compatible storage implementation using the AWS SDK.
pub struct S3ObjectStorage {
    client: aws_sdk_s3::Client,
}

impl S3ObjectStorage {
    /// Creates a new S3-compatible storage instance.
    ///
    /// # Arguments
    /// * `client` - AWS SDK S3 client (configured for S3 or MinIO)
    pub fn new(client: aws_sdk_s3::Client) -> Self {
        Self { client }
    }
}

#[backend_core::async_trait]
impl ObjectStorage for S3ObjectStorage {
    async fn head_object(&self, bucket: &str, key: &str) -> Result<(), Error> {
        self.client
            .head_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map(|_| ())
            .map_err(|e| Error::s3(e.to_string()))
    }

    async fn upload(
        &self,
        _bucket: &str,
        _key: &str,
        _mime_type: &str,
        _encryption: EncryptionMode,
        _body: Bytes,
    ) -> Result<(), Error> {
        Err(Error::bad_request(
            "STORAGE_UNSUPPORTED",
            "direct upload is not supported by this object storage backend",
        ))
    }

    async fn upload_presigned(
        &self,
        bucket: &str,
        key: &str,
        mime_type: &str,
        encryption: EncryptionMode,
        expires_in: Duration,
    ) -> Result<PresignedUpload, Error> {
        let mut builder = self.client.put_object().bucket(bucket).key(key);
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_owned(), mime_type.to_owned());

        match encryption {
            EncryptionMode::S3 => {
                builder =
                    builder.server_side_encryption(aws_sdk_s3::types::ServerSideEncryption::Aes256);
                headers.insert(
                    "x-amz-server-side-encryption".to_owned(),
                    "AES256".to_owned(),
                );
            }
            EncryptionMode::Kms => {
                builder =
                    builder.server_side_encryption(aws_sdk_s3::types::ServerSideEncryption::AwsKms);
                headers.insert(
                    "x-amz-server-side-encryption".to_owned(),
                    "aws:kms".to_owned(),
                );
            }
            EncryptionMode::None => {}
        }

        let presigned = builder
            .content_type(mime_type)
            .presigned(
                aws_sdk_s3::presigning::PresigningConfig::expires_in(expires_in)
                    .map_err(|e| Error::s3(e.to_string()))?,
            )
            .await
            .map_err(|e| Error::s3(e.to_string()))?;

        Ok(PresignedUpload {
            url: presigned.uri().to_string(),
            headers,
        })
    }

    async fn download(&self, _bucket: &str, _key: &str) -> Result<Bytes, Error> {
        Err(Error::bad_request(
            "STORAGE_UNSUPPORTED",
            "direct download is not supported by this object storage backend",
        ))
    }

    async fn download_presigned(
        &self,
        bucket: &str,
        key: &str,
        expires_in: Duration,
        content_disposition: Option<String>,
    ) -> Result<String, Error> {
        let mut builder = self.client.get_object().bucket(bucket).key(key);
        if let Some(cd) = content_disposition {
            builder = builder.response_content_disposition(cd);
        }

        let presigned_req = builder
            .presigned(
                aws_sdk_s3::presigning::PresigningConfig::expires_in(expires_in)
                    .map_err(|e| Error::s3(e.to_string()))?,
            )
            .await
            .map_err(|e| Error::s3(e.to_string()))?;

        Ok(presigned_req.uri().to_string())
    }
}
