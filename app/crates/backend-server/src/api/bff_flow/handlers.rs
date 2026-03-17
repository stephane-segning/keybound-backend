use axum::Json;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::http::StatusCode;
use backend_core::Error;
use tracing::instrument;

use crate::api::BackendApi;

use super::models::{
    AddFlowRequest, CreateSessionRequest, FlowDetailResponse, FlowResponse, KycLevelResponse,
    SessionDetailResponse, SessionResponse, StepResponse, SubmitStepRequest, UserResponse,
};
use super::service;

#[utoipa::path(
    get,
    path = "/flow/users/{userId}",
    tag = "users",
    params(("userId" = String, Path)),
    responses((status = 200, body = UserResponse))
)]
#[instrument(skip(api, headers))]
pub async fn get_user(
    State(api): State<BackendApi>,
    Path(user_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<UserResponse>, Error> {
    let caller_id = service::require_user_id(&api, &headers).await?;
    let user = service::get_user(&api, user_id, caller_id).await?;
    Ok(Json(user))
}

#[utoipa::path(
    get,
    path = "/flow/users/{userId}/kyc-level",
    tag = "users",
    params(("userId" = String, Path)),
    responses((status = 200, body = KycLevelResponse))
)]
#[instrument(skip(api, headers))]
pub async fn get_kyc_level(
    State(api): State<BackendApi>,
    Path(user_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<KycLevelResponse>, Error> {
    let caller_id = service::require_user_id(&api, &headers).await?;
    let kyc_level = service::get_kyc_level(&api, user_id, caller_id).await?;
    Ok(Json(kyc_level))
}

#[utoipa::path(
    get,
    path = "/flow/sessions",
    tag = "sessions",
    responses((status = 200, body = [SessionResponse]))
)]
#[instrument(skip(api, headers))]
pub async fn list_sessions(
    State(api): State<BackendApi>,
    headers: HeaderMap,
) -> Result<Json<Vec<SessionResponse>>, Error> {
    let user_id = service::require_user_id(&api, &headers).await?;
    let sessions = service::list_sessions(&api, user_id).await?;
    Ok(Json(sessions))
}

#[utoipa::path(
    post,
    path = "/flow/sessions",
    tag = "sessions",
    request_body = CreateSessionRequest,
    responses((status = 201, body = SessionResponse))
)]
#[instrument(skip(api, headers))]
pub async fn create_session(
    State(api): State<BackendApi>,
    headers: HeaderMap,
    Json(body): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<SessionResponse>), Error> {
    let user_id = service::require_user_id(&api, &headers).await?;
    let session = service::create_session(&api, user_id, body).await?;
    Ok((StatusCode::CREATED, Json(session)))
}

#[utoipa::path(
    get,
    path = "/flow/sessions/{sessionId}",
    tag = "sessions",
    params(("sessionId" = String, Path)),
    responses((status = 200, body = SessionDetailResponse))
)]
#[instrument(skip(api, headers))]
pub async fn get_session(
    State(api): State<BackendApi>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<SessionDetailResponse>, Error> {
    let user_id = service::require_user_id(&api, &headers).await?;
    let payload = service::get_session(&api, session_id, user_id).await?;
    Ok(Json(payload))
}

#[utoipa::path(
    get,
    path = "/flow/sessions/{sessionId}/flows",
    tag = "sessions",
    params(("sessionId" = String, Path)),
    responses((status = 200, body = [FlowResponse]))
)]
#[instrument(skip(api, headers))]
pub async fn list_session_flows(
    State(api): State<BackendApi>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Vec<FlowResponse>>, Error> {
    let user_id = service::require_user_id(&api, &headers).await?;
    let flows = service::list_session_flows(&api, session_id, user_id).await?;
    Ok(Json(flows))
}

#[utoipa::path(
    post,
    path = "/flow/sessions/{sessionId}/flows",
    tag = "sessions",
    params(("sessionId" = String, Path)),
    request_body = AddFlowRequest,
    responses((status = 201, body = FlowResponse))
)]
#[instrument(skip(api, headers))]
pub async fn add_flow_to_session(
    State(api): State<BackendApi>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<AddFlowRequest>,
) -> Result<(StatusCode, Json<FlowResponse>), Error> {
    let user_id = service::require_user_id(&api, &headers).await?;
    let flow = service::add_flow_to_session(&api, session_id, user_id, body).await?;
    Ok((StatusCode::CREATED, Json(flow)))
}

#[utoipa::path(
    get,
    path = "/flow/flows/{flowId}",
    tag = "flows",
    params(("flowId" = String, Path)),
    responses((status = 200, body = FlowDetailResponse))
)]
#[instrument(skip(api, headers))]
pub async fn get_flow(
    State(api): State<BackendApi>,
    Path(flow_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<FlowDetailResponse>, Error> {
    let user_id = service::require_user_id(&api, &headers).await?;
    let flow = service::get_flow(&api, flow_id, user_id).await?;
    Ok(Json(flow))
}

#[utoipa::path(
    get,
    path = "/flow/flows/{flowId}/steps",
    tag = "flows",
    params(("flowId" = String, Path)),
    responses((status = 200, body = [StepResponse]))
)]
#[instrument(skip(api, headers))]
pub async fn list_flow_steps(
    State(api): State<BackendApi>,
    Path(flow_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Vec<StepResponse>>, Error> {
    let user_id = service::require_user_id(&api, &headers).await?;
    let steps = service::list_flow_steps(&api, flow_id, user_id).await?;
    Ok(Json(steps))
}

#[utoipa::path(
    get,
    path = "/flow/steps/{stepId}",
    tag = "steps",
    params(("stepId" = String, Path)),
    responses((status = 200, body = StepResponse))
)]
#[instrument(skip(api, headers))]
pub async fn get_step(
    State(api): State<BackendApi>,
    Path(step_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<StepResponse>, Error> {
    let user_id = service::require_user_id(&api, &headers).await?;
    let step = service::get_step(&api, step_id, user_id).await?;
    Ok(Json(step))
}

#[utoipa::path(
    post,
    path = "/flow/steps/{stepId}",
    tag = "steps",
    params(("stepId" = String, Path)),
    request_body = SubmitStepRequest,
    responses((status = 200, body = StepResponse))
)]
#[instrument(skip(api, headers))]
pub async fn submit_step(
    State(api): State<BackendApi>,
    Path(step_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<SubmitStepRequest>,
) -> Result<Json<StepResponse>, Error> {
    let user_id = service::require_user_id(&api, &headers).await?;
    let step = service::submit_step(&api, step_id, user_id, body).await?;
    Ok(Json(step))
}
