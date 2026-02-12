use backend_model::{db, kc as kc_map, staff as staff_map};
use backend_repository::{
    ApprovalCreated, KycDocumentInsert, PgRepository, RepoResult, SmsPendingInsert, SmsQueued,
};
use sqlx_data::{ParamsBuilder, Serial};

#[derive(Clone)]
pub struct BackendService {
    repository: PgRepository,
}

impl BackendService {
    pub fn new(repository: PgRepository) -> Self {
        Self { repository }
    }

    pub async fn ensure_kyc_profile(&self, external_id: &str) -> RepoResult<()> {
        self.repository.ensure_kyc_profile(external_id).await
    }

    pub async fn insert_kyc_document_intent(
        &self,
        input: KycDocumentInsert,
    ) -> RepoResult<db::KycDocumentRow> {
        self.repository.insert_kyc_document_intent(input).await
    }

    pub async fn get_kyc_profile(
        &self,
        external_id: &str,
    ) -> RepoResult<Option<db::KycProfileRow>> {
        self.repository.get_kyc_profile(external_id).await
    }

    pub async fn list_kyc_documents(
        &self,
        external_id: &str,
        page: i32,
        limit: i32,
    ) -> RepoResult<Serial<db::KycDocumentRow>> {
        let params = ParamsBuilder::new()
            .serial()
            .page(page.max(1) as u32, limit.clamp(1, 100) as u32)
            .done()
            .build();

        self.repository
            .list_kyc_documents(external_id.to_owned(), params)
            .await
    }

    pub async fn get_kyc_tier(&self, external_id: &str) -> RepoResult<Option<i32>> {
        self.repository.get_kyc_tier(external_id).await
    }

    pub async fn patch_kyc_information(
        &self,
        external_id: &str,
        req: &backend_model::bff::KycInformationPatchRequest,
    ) -> RepoResult<Option<db::KycProfileRow>> {
        self.repository.patch_kyc_information(external_id, req).await
    }

    pub async fn list_kyc_submissions(
        &self,
        status: Option<String>,
        search: Option<String>,
        page: i32,
        limit: i32,
    ) -> RepoResult<Serial<db::KycProfileRow>> {
        let mut builder = ParamsBuilder::new()
            .serial()
            .page(page.max(1) as u32, limit.clamp(1, 100) as u32)
            .done();

        if let Some(status) = status {
            builder = builder.filter().eq("kyc_status", status).done();
        }

        if let Some(search) = search {
            builder = builder
                .search()
                .search(search, ["external_id", "email", "phone_number"])
                .case_sensitive(false)
                .done();
        }

        self.repository.list_kyc_submissions(builder.build()).await
    }

    pub async fn get_kyc_submission(
        &self,
        external_id: &str,
    ) -> RepoResult<Option<db::KycProfileRow>> {
        self.repository.get_kyc_submission(external_id).await
    }

    pub async fn update_kyc_approved(
        &self,
        external_id: &str,
        req: &staff_map::KycApprovalRequest,
    ) -> RepoResult<bool> {
        self.repository.update_kyc_approved(external_id, req).await
    }

    pub async fn update_kyc_rejected(
        &self,
        external_id: &str,
        req: &staff_map::KycRejectionRequest,
    ) -> RepoResult<bool> {
        self.repository.update_kyc_rejected(external_id, req).await
    }

    pub async fn update_kyc_request_info(
        &self,
        external_id: &str,
        req: &staff_map::KycRequestInfoRequest,
    ) -> RepoResult<bool> {
        self.repository
            .update_kyc_request_info(external_id, req)
            .await
    }

    pub async fn create_user(&self, req: &kc_map::UserUpsert) -> RepoResult<db::UserRow> {
        self.repository.create_user(req).await
    }

    pub async fn get_user(&self, user_id: &str) -> RepoResult<Option<db::UserRow>> {
        self.repository.get_user(user_id).await
    }

    pub async fn update_user(
        &self,
        user_id: &str,
        req: &kc_map::UserUpsert,
    ) -> RepoResult<Option<db::UserRow>> {
        self.repository.update_user(user_id, req).await
    }

    pub async fn delete_user(&self, user_id: &str) -> RepoResult<u64> {
        self.repository.delete_user(user_id).await
    }

    pub async fn search_users(&self, req: &kc_map::UserSearch) -> RepoResult<Vec<db::UserRow>> {
        self.repository.search_users(req).await
    }

    pub async fn lookup_device(
        &self,
        req: &kc_map::DeviceLookupRequest,
    ) -> RepoResult<Option<db::DeviceRow>> {
        self.repository.lookup_device(req).await
    }

    pub async fn list_user_devices(
        &self,
        user_id: &str,
        include_revoked: bool,
    ) -> RepoResult<Vec<db::DeviceRow>> {
        self.repository
            .list_user_devices(user_id, include_revoked)
            .await
    }

    pub async fn get_user_device(
        &self,
        user_id: &str,
        device_id: &str,
    ) -> RepoResult<Option<db::DeviceRow>> {
        self.repository.get_user_device(user_id, device_id).await
    }

    pub async fn update_device_status(
        &self,
        record_id: &str,
        status: &str,
    ) -> RepoResult<db::DeviceRow> {
        self.repository
            .update_device_status(record_id, status)
            .await
    }

    pub async fn find_device_binding(
        &self,
        device_id: &str,
        jkt: &str,
    ) -> RepoResult<Option<(String, String)>> {
        self.repository.find_device_binding(device_id, jkt).await
    }

    pub async fn bind_device(&self, req: &kc_map::EnrollmentBindRequest) -> RepoResult<String> {
        self.repository.bind_device(req).await
    }

    pub async fn create_approval(
        &self,
        req: &kc_map::ApprovalCreateRequest,
        idempotency_key: Option<String>,
    ) -> RepoResult<ApprovalCreated> {
        self.repository.create_approval(req, idempotency_key).await
    }

    pub async fn get_approval(&self, request_id: &str) -> RepoResult<Option<db::ApprovalRow>> {
        self.repository.get_approval(request_id).await
    }

    pub async fn list_user_approvals(
        &self,
        user_id: &str,
        statuses: Option<Vec<String>>,
    ) -> RepoResult<Vec<db::ApprovalRow>> {
        self.repository.list_user_approvals(user_id, statuses).await
    }

    pub async fn decide_approval(
        &self,
        request_id: &str,
        req: &kc_map::ApprovalDecisionRequest,
    ) -> RepoResult<Option<db::ApprovalRow>> {
        self.repository.decide_approval(request_id, req).await
    }

    pub async fn cancel_approval(&self, request_id: &str) -> RepoResult<u64> {
        self.repository.cancel_approval(request_id).await
    }

    pub async fn resolve_user_by_phone(
        &self,
        realm: &str,
        phone: &str,
    ) -> RepoResult<Option<db::UserRow>> {
        self.repository.resolve_user_by_phone(realm, phone).await
    }

    pub async fn resolve_or_create_user_by_phone(
        &self,
        realm: &str,
        phone: &str,
    ) -> RepoResult<(db::UserRow, bool)> {
        self.repository
            .resolve_or_create_user_by_phone(realm, phone)
            .await
    }

    pub async fn count_user_devices(&self, user_id: &str) -> RepoResult<i64> {
        self.repository.count_user_devices(user_id).await
    }

    pub async fn queue_sms(&self, sms: SmsPendingInsert) -> RepoResult<SmsQueued> {
        self.repository.queue_sms(sms).await
    }

    pub async fn get_sms_by_hash(&self, hash: &str) -> RepoResult<Option<db::SmsMessageRow>> {
        self.repository.get_sms_by_hash(hash).await
    }

    pub async fn mark_sms_confirmed(&self, hash: &str) -> RepoResult<()> {
        self.repository.mark_sms_confirmed(hash).await
    }
}
