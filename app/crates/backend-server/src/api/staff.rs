use super::BackendApi;

#[backend_core::async_trait]
impl gen_oas_server_staff::apis::kyc_review::KycReview for BackendApi {}
