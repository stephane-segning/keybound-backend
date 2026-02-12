use chrono::{DateTime, Utc};
use o2o::o2o;
use std::collections::HashMap;

use crate::db;

#[derive(Debug, Clone, o2o)]
#[from_owned(gen_oas_server_bff::models::KycDocumentUploadRequest)]
pub struct KycDocumentUploadRequest {
    pub document_type: String,
    pub file_name: String,
    pub mime_type: String,
    pub content_length: i64,
}

#[derive(Debug, Clone, o2o)]
#[owned_into(gen_oas_server_bff::models::KycStatusResponseDocumentStatus)]
pub struct KycStatusDocumentStatusDto {
    pub document_type: Option<String>,
    pub status: Option<String>,
    pub uploaded_at: Option<DateTime<Utc>>,
    pub rejection_reason: Option<String>,
}

impl From<db::KycDocumentRow> for KycStatusDocumentStatusDto {
    fn from(row: db::KycDocumentRow) -> Self {
        Self {
            document_type: Some(row.document_type),
            status: Some(row.status),
            uploaded_at: row.uploaded_at,
            rejection_reason: row.rejection_reason,
        }
    }
}

#[derive(Debug, Clone, o2o)]
#[owned_into(gen_oas_server_bff::models::KycDocumentUploadResponse)]
pub struct KycDocumentUploadResponseDto {
    pub document_id: Option<String>,
    pub document_type: Option<String>,
    pub status: Option<String>,
    pub uploaded_at: Option<DateTime<Utc>>,
    pub file_name: Option<String>,
    pub mime_type: Option<String>,
    pub upload_url: Option<String>,
    pub upload_method: Option<String>,
    pub upload_headers: Option<HashMap<String, String>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub s3_bucket: Option<String>,
    pub s3_key: Option<String>,
}

#[derive(Debug, Clone, o2o)]
#[owned_into(gen_oas_server_bff::models::KycStatusResponse)]
pub struct KycStatusResponseDto {
    pub kyc_tier: Option<i32>,
    pub kyc_status: Option<String>,
    pub documents: Option<Vec<gen_oas_server_bff::models::KycStatusResponseDocumentStatus>>,
    pub required_documents: Option<Vec<String>>,
    pub missing_documents: Option<Vec<String>>,
}
