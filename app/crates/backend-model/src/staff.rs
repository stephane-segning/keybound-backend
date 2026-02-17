use o2o::o2o;
use std::collections::HashMap;

use crate::db;

#[derive(Debug, Clone, o2o)]
#[from_owned(gen_oas_server_staff::models::KycApprovalRequest)]
pub struct KycApprovalRequest {
    pub new_tier: u8,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, o2o)]
#[from_owned(gen_oas_server_staff::models::KycRejectionRequest)]
pub struct KycRejectionRequest {
    pub reason: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, o2o)]
#[from_owned(gen_oas_server_staff::models::KycRequestInfoRequest)]
pub struct KycRequestInfoRequest {
    pub message: String,
}

#[derive(Debug, Clone, o2o)]
#[owned_into(gen_oas_server_staff::models::KycDocumentDto)]
pub struct KycDocumentDto {
    pub id: Option<String>,
    #[map(r_type)]
    pub document_type: Option<String>,
    #[map(file_name)]
    pub file_name: Option<String>,
    #[map(mime_type)]
    pub mime_type: Option<String>,
    pub url: Option<String>,
    #[map(uploaded_at)]
    pub uploaded_at: Option<String>,
}

impl From<db::KycDocumentRow> for KycDocumentDto {
    fn from(row: db::KycDocumentRow) -> Self {
        Self {
            id: Some(row.id),
            document_type: Some(row.doc_type),
            file_name: Some(row.file_name),
            mime_type: Some(row.mime_type),
            url: None,
            uploaded_at: Some(row.uploaded_at.to_rfc3339()),
        }
    }
}

#[derive(Debug, Clone, o2o)]
#[owned_into(gen_oas_server_staff::models::KycSubmissionSummary)]
pub struct KycSubmissionSummaryDto {
    pub external_id: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub phone_number: Option<String>,
    pub kyc_tier: Option<i32>,
    pub kyc_status: Option<String>,
    pub submitted_at: Option<String>,
}

impl From<db::KycSubmissionRow> for KycSubmissionSummaryDto {
    fn from(row: db::KycSubmissionRow) -> Self {
        Self {
            external_id: Some(row.id),
            first_name: row.first_name,
            last_name: row.last_name,
            email: row.email,
            phone_number: row.phone_number,
            kyc_tier: None, // Calculated dynamically
            kyc_status: Some(row.status),
            submitted_at: row
                .submitted_at
                .map(|v: chrono::DateTime<chrono::Utc>| v.to_rfc3339()),
        }
    }
}

#[derive(Debug, Clone, o2o)]
#[owned_into(gen_oas_server_staff::models::KycSubmissionsResponse)]
pub struct KycSubmissionsResponseDto {
    pub items: Option<Vec<gen_oas_server_staff::models::KycSubmissionSummary>>,
    pub total: Option<i32>,
    pub page: Option<i32>,
    pub page_size: Option<i32>,
}

#[derive(Debug, Clone, o2o)]
#[owned_into(gen_oas_server_staff::models::KycSubmissionDetailResponse)]
pub struct KycSubmissionDetailResponseDto {
    pub external_id: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub phone_number: Option<String>,
    pub date_of_birth: Option<String>,
    pub nationality: Option<String>,
    pub kyc_tier: Option<i32>,
    pub kyc_status: Option<String>,
    pub documents: Option<Vec<gen_oas_server_staff::models::KycDocumentDto>>,
    pub submitted_at: Option<String>,
    pub reviewed_at: Option<String>,
    pub reviewed_by: Option<String>,
    pub rejection_reason: Option<String>,
    pub review_notes: Option<String>,
    pub page: Option<i32>,
    pub page_size: Option<i32>,
    pub total_documents: Option<i32>,
}

impl KycSubmissionDetailResponseDto {
    pub fn from_submission(profile: db::KycSubmissionRow) -> Self {
        Self {
            external_id: Some(profile.id),
            first_name: profile.first_name,
            last_name: profile.last_name,
            email: profile.email,
            phone_number: profile.phone_number,
            date_of_birth: profile.date_of_birth,
            nationality: profile.nationality,
            kyc_tier: None, // Calculated dynamically
            kyc_status: Some(profile.status),
            documents: Some(vec![]),
            submitted_at: profile
                .submitted_at
                .map(|v: chrono::DateTime<chrono::Utc>| v.to_rfc3339()),
            reviewed_at: profile
                .decided_at
                .map(|v: chrono::DateTime<chrono::Utc>| v.to_rfc3339()),
            reviewed_by: profile.decided_by,
            rejection_reason: profile.rejection_reason,
            review_notes: profile.review_notes,
            page: None,
            page_size: None,
            total_documents: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PresignedPut {
    pub url: String,
    pub method: String,
    pub headers: HashMap<String, String>,
}
