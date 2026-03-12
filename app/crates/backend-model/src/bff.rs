//! Data transfer objects for BFF (Backend-for-Frontend) API surface.
//!
//! These types are used by the BFF layer to communicate with the frontend.

/// Request to patch KYC information for a user.
#[derive(Debug, Clone)]
pub struct KycInformationPatchRequest {
    pub full_name: Option<String>,
    pub email: Option<String>,
    pub phone_number: Option<String>,
    pub date_of_birth: Option<String>,
    pub nationality: Option<String>,
}
