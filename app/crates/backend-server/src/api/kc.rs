use super::BackendApi;

#[backend_core::async_trait]
impl gen_oas_server_kc::apis::approvals::Approvals for BackendApi {}

#[backend_core::async_trait]
impl gen_oas_server_kc::apis::devices::Devices for BackendApi {}

#[backend_core::async_trait]
impl gen_oas_server_kc::apis::users::Users for BackendApi {}

#[backend_core::async_trait]
impl gen_oas_server_kc::apis::enrollment::Enrollment for BackendApi {}
