use super::shared::{
    ensure_step_registered, parse_step_status, parse_step_type, split_step_id, user_id_matches,
};
use backend_auth::JwtToken;
use backend_core::Error;
use gen_oas_server_bff::apis::steps::InternalGetKycStepResponse;
use gen_oas_server_bff::models;
use tracing::instrument;

use super::super::BackendApi;

impl BackendApi {
    #[instrument(skip(self))]
    pub async fn get_kyc_step_flow(
        &self,
        claims: &JwtToken,
        path_params: &models::InternalGetKycStepPathParams,
    ) -> Result<InternalGetKycStepResponse, Error> {
        // Parse step ID to extract session and step type, validate format
        let (session_id, step_type) = split_step_id(&path_params.step_id)
            .ok_or_else(|| Error::bad_request("INVALID_STEP_ID", "Step id format is invalid"))?;

        let user_id = BackendApi::require_user_id(claims)?;
        let session = self
            .state
            .sm
            .get_instance(&session_id)
            .await?
            .ok_or_else(|| Error::not_found("SESSION_NOT_FOUND", "Session not found"))?;

        if !user_id_matches(session.user_id.as_deref(), &user_id) {
            return Err(Error::unauthorized(
                "Step does not belong to authenticated user",
            ));
        }
        ensure_step_registered(&session.context, &path_params.step_id)?;

        let attempts = self.state.sm.list_step_attempts(&session.id).await?;
        let status = parse_step_status(
            &session.kind,
            &session.status,
            &step_type,
            &attempts,
            &session.context,
        );

        Ok(InternalGetKycStepResponse::Status200_Step(
            models::KycStep {
                id: path_params.step_id.clone(),
                session_id: session.id,
                user_id,
                r_type: parse_step_type(&step_type)?,
                status,
                data: None,
                policy: None,
                created_at: session.created_at,
                updated_at: session.updated_at,
            },
        ))
    }
}
