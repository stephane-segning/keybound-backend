use backend_core::Error;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct PresignedUpload {
    pub url: String,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionMode {
    None,
    S3,
    Kms,
}

#[cfg_attr(any(test, feature = "test-utils"), mockall::automock)]
#[backend_core::async_trait]
pub trait FileStorage: Send + Sync {
    async fn head_object(&self, bucket: &str, key: &str) -> Result<(), Error>;

    async fn presign_get_object(
        &self,
        bucket: &str,
        key: &str,
        expires_in: Duration,
        content_disposition: Option<String>,
    ) -> Result<String, Error>;

    async fn presign_put_object(
        &self,
        bucket: &str,
        key: &str,
        mime_type: &str,
        encryption: EncryptionMode,
        expires_in: Duration,
    ) -> Result<PresignedUpload, Error>;
}

pub struct S3FileStorage {
    client: aws_sdk_s3::Client,
}

impl S3FileStorage {
    pub fn new(client: aws_sdk_s3::Client) -> Self {
        Self { client }
    }
}

#[backend_core::async_trait]
impl FileStorage for S3FileStorage {
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

    async fn presign_get_object(
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

    async fn presign_put_object(
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
                builder = builder
                    .server_side_encryption(aws_sdk_s3::types::ServerSideEncryption::Aes256);
                headers.insert(
                    "x-amz-server-side-encryption".to_owned(),
                    "AES256".to_owned(),
                );
            }
            EncryptionMode::Kms => {
                builder = builder
                    .server_side_encryption(aws_sdk_s3::types::ServerSideEncryption::AwsKms);
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
}
