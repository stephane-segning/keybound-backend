use super::super::BackendApi;
use super::shared::{
    KIND_KYC_ADDRESS_PROOF, KIND_KYC_FIRST_DEPOSIT, KIND_KYC_ID_DOCUMENT, KIND_KYC_PHONE_OTP,
    api_map_to_value, ensure_user_match, flow_kind, is_instance_active, parse_flow,
    put_flow_in_context, session_from_instance,
};
use crate::state_machine::engine::Engine;
use backend_auth::JwtToken;
use backend_core::Error;
use backend_repository::SmInstanceFilter;
use gen_oas_server_bff::apis::sessions::{
    InternalCreateKycSessionResponse, InternalGetKycSessionResponse,
    InternalListKycSessionsResponse,
};
use gen_oas_server_bff::models;
use serde_json::Value;

#[backend_core::async_trait]
pub(super) trait SessionFlow {
    async fn create_kyc_session_flow(
        &self,
        claims: &JwtToken,
        body: &models::CreateKycSessionRequest,
    ) -> Result<InternalCreateKycSessionResponse, Error>;

    async fn get_kyc_session_flow(
        &self,
        claims: &JwtToken,
        path_params: &models::InternalGetKycSessionPathParams,
    ) -> Result<InternalGetKycSessionResponse, Error>;

    async fn list_kyc_sessions_flow(
        &self,
        claims: &JwtToken,
        query_params: &models::InternalListKycSessionsQueryParams,
    ) -> Result<InternalListKycSessionsResponse, Error>;
}

#[backend_core::async_trait]
impl SessionFlow for BackendApi {
    async fn create_kyc_session_flow(
        &self,
        claims: &JwtToken,
        body: &models::CreateKycSessionRequest,
    ) -> Result<InternalCreateKycSessionResponse, Error> {
        ensure_user_match(claims, &body.user_id)?;

        let kind = flow_kind(body.flow);
        let key = format!("{}:{}:{}", kind, body.flow, body.user_id);

        let mut context =
            api_map_to_value(body.context.clone()).unwrap_or(Value::Object(Default::default()));
        if !context.is_object() {
            context = Value::Object(Default::default());
        }
        if let Some(obj) = context.as_object_mut()
            && !obj.get("step_ids").is_some_and(Value::is_array)
        {
            obj.insert("step_ids".to_owned(), Value::Array(vec![]));
        }
        put_flow_in_context(&mut context, body.flow);

        let engine = Engine::new(self.state.clone());
        let mut instance = engine
            .ensure_active_instance(kind, Some(body.user_id.clone()), key, context)
            .await?;

        let mut normalized = instance.context.clone();
        let mut changed = false;
        if !normalized
            .as_object()
            .and_then(|obj| obj.get("step_ids"))
            .is_some_and(Value::is_array)
        {
            if let Some(obj) = normalized.as_object_mut() {
                obj.insert("step_ids".to_owned(), Value::Array(vec![]));
                changed = true;
            } else {
                normalized = Value::Object(Default::default());
                if let Some(obj) = normalized.as_object_mut() {
                    obj.insert("step_ids".to_owned(), Value::Array(vec![]));
                    changed = true;
                }
            }
        }
        if put_flow_in_context(&mut normalized, body.flow) {
            changed = true;
        }

        if changed {
            self.state
                .sm
                .update_instance_context(&instance.id, normalized)
                .await?;
            instance = self
                .state
                .sm
                .get_instance(&instance.id)
                .await?
                .ok_or_else(|| Error::not_found("SESSION_NOT_FOUND", "Session not found"))?;
        }

        Ok(
            InternalCreateKycSessionResponse::Status201_SessionCreatedOrResumed(
                session_from_instance(instance),
            ),
        )
    }

    async fn get_kyc_session_flow(
        &self,
        claims: &JwtToken,
        path_params: &models::InternalGetKycSessionPathParams,
    ) -> Result<InternalGetKycSessionResponse, Error> {
        let user_id = BackendApi::require_user_id(claims)?;
        let instance = self
            .state
            .sm
            .get_instance(&path_params.session_id)
            .await?
            .ok_or_else(|| Error::not_found("SESSION_NOT_FOUND", "Session not found"))?;

        if instance.user_id.as_deref() != Some(&user_id) {
            return Err(Error::unauthorized(
                "Session does not belong to authenticated user",
            ));
        }

        Ok(InternalGetKycSessionResponse::Status200_Session(
            session_from_instance(instance),
        ))
    }

    async fn list_kyc_sessions_flow(
        &self,
        claims: &JwtToken,
        query_params: &models::InternalListKycSessionsQueryParams,
    ) -> Result<InternalListKycSessionsResponse, Error> {
        let authed_user_id = BackendApi::require_user_id(claims)?;
        let user_id = query_params.user_id.as_deref().unwrap_or(&authed_user_id);
        ensure_user_match(claims, user_id)?;

        let active_only = query_params.active_only.unwrap_or(true);
        let kinds: Vec<&'static str> = if let Some(flow) = query_params.flow {
            vec![flow_kind(flow)]
        } else {
            vec![
                KIND_KYC_PHONE_OTP,
                KIND_KYC_FIRST_DEPOSIT,
                KIND_KYC_ID_DOCUMENT,
                KIND_KYC_ADDRESS_PROOF,
            ]
        };

        let mut items = Vec::new();
        for kind in kinds {
            let (instances, _) = self
                .state
                .sm
                .list_instances(SmInstanceFilter {
                    kind: Some(kind.to_owned()),
                    status: None,
                    user_id: Some(user_id.to_owned()),
                    phone_number: None,
                    created_from: None,
                    created_to: None,
                    page: 1,
                    limit: 100,
                })
                .await?;

            for instance in instances {
                if let Some(flow_filter) = query_params.flow
                    && parse_flow(&instance.kind, &instance.context) != flow_filter
                {
                    continue;
                }
                if active_only && !is_instance_active(&instance.status, &instance.context) {
                    continue;
                }
                items.push(session_from_instance(instance));
            }
        }

        items.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));

        Ok(InternalListKycSessionsResponse::Status200_SessionsList(
            models::KycSessionListResponse::new(items),
        ))
    }
}
