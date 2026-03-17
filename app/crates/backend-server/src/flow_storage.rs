use backend_flow_sdk::{StorageService, UploadUrlResult};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

pub struct FlowStorageService {
    storage: Arc<dyn crate::file_storage::MinioStorage>,
    bucket: String,
    url_expiry: Duration,
}

impl fmt::Debug for FlowStorageService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FlowStorageService")
            .field("bucket", &self.bucket)
            .field("url_expiry", &self.url_expiry)
            .finish_non_exhaustive()
    }
}

impl FlowStorageService {
    pub fn new(storage: Arc<dyn crate::file_storage::MinioStorage>, bucket: String) -> Self {
        Self {
            storage,
            bucket,
            url_expiry: Duration::from_secs(3600),
        }
    }

    pub fn with_url_expiry(mut self, expiry: Duration) -> Self {
        self.url_expiry = expiry;
        self
    }
}

#[async_trait::async_trait]
impl StorageService for FlowStorageService {
    async fn generate_upload_url(
        &self,
        document_type: &str,
        session_id: &str,
    ) -> Result<UploadUrlResult, String> {
        let key = format!("{}/{}.jpg", session_id, document_type);

        let presigned = self
            .storage
            .upload_presigned(
                &self.bucket,
                &key,
                "image/jpeg",
                crate::file_storage::EncryptionMode::S3,
                self.url_expiry,
            )
            .await
            .map_err(|e| e.to_string())?;

        Ok(UploadUrlResult {
            url: presigned.url,
            key,
            headers: presigned.headers,
        })
    }
}