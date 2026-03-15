use axum::Json;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use backend_core::Error;

use crate::api::BackendApi;

use super::models::{
    AddFlowRequest, CreateSessionRequest, FlowDetailResponse, FlowResponse, SessionDetailResponse,
    SessionResponse, StepResponse, SubmitStepRequest,
};
use super::service;

#[utoipa::path(
    get,
    path = "/sessions",
    responses((status = 200, body = [SessionResponse]))
)]
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
    path = "/sessions",
    request_body = CreateSessionRequest,
    responses((status = 200, body = SessionResponse))
)]
pub async fn create_session(
    State(api): State<BackendApi>,
    headers: HeaderMap,
    Json(body): Json<CreateSessionRequest>,
) -> Result<Json<SessionResponse>, Error> {
    let user_id = service::require_user_id(&api, &headers).await?;
    let session = service::create_session(&api, user_id, body).await?;
    Ok(Json(session))
}

#[utoipa::path(
    get,
    path = "/sessions/{sessionId}",
    params(("sessionId" = String, Path)),
    responses((status = 200, body = SessionDetailResponse))
)]
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
    path = "/sessions/{sessionId}/flows",
    params(("sessionId" = String, Path)),
    responses((status = 200, body = [FlowResponse]))
)]
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
    path = "/sessions/{sessionId}/flows",
    params(("sessionId" = String, Path)),
    request_body = AddFlowRequest,
    responses((status = 200, body = FlowResponse))
)]
pub async fn add_flow_to_session(
    State(api): State<BackendApi>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<AddFlowRequest>,
) -> Result<Json<FlowResponse>, Error> {
    let user_id = service::require_user_id(&api, &headers).await?;
    let flow = service::add_flow_to_session(&api, session_id, user_id, body).await?;
    Ok(Json(flow))
}

#[utoipa::path(
    get,
    path = "/flows/{flowId}",
    params(("flowId" = String, Path)),
    responses((status = 200, body = FlowDetailResponse))
)]
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
    path = "/flows/{flowId}/steps",
    params(("flowId" = String, Path)),
    responses((status = 200, body = [StepResponse]))
)]
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
    path = "/steps/{stepId}",
    params(("stepId" = String, Path)),
    responses((status = 200, body = StepResponse))
)]
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
    path = "/steps/{stepId}",
    params(("stepId" = String, Path)),
    request_body = SubmitStepRequest,
    responses((status = 200, body = StepResponse))
)]
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
