use super::BackendApi;

#[backend_core::async_trait]
impl gen_oas_server_bff::apis::kyc::Kyc for BackendApi {}

#[backend_core::async_trait]
impl gen_oas_server_bff::apis::kyc_documents::KycDocuments for BackendApi {}
