use super::BackendApi;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::{Json, Router, routing::get};
use backend_auth::JwtToken;
use backend_core::Error;
use backend_flow_sdk::HumanReadableId;
use backend_repository::{
    FlowInstanceCreateInput, FlowSessionCreateInput, FlowSessionFilter, FlowStepCreateInput,
    FlowStepPatch,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub fn router(api: BackendApi) -> Router {
    Router::new()
        .route("/sessions", get(list_sessions).post(create_session))
        .route("/sessions/{session_id}", get(get_session))
        .route(
            "/sessions/{session_id}/flows",
            get(list_session_flows).post(add_flow_to_session),
        )
        .route("/flows/{flow_id}", get(get_flow))
        .route("/flows/{flow_id}/steps", get(list_flow_steps))
        .route("/steps/{step_id}", get(get_step).post(submit_step))
        .with_state(api)
}

#[derive(Debug, Deserialize)]
struct CreateSessionRequest {
    session_type: String,
    #[serde(default)]
    human_id: Option<String>,
    #[serde(default)]
    context: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct AddFlowRequest {
    flow_type: String,
    #[serde(default)]
    human_id: Option<String>,
    #[serde(default)]
    context: Option<Value>,
    #[serde(default)]
    initial_step: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SubmitStepRequest {
    #[serde(default)]
    input: Value,
}

#[derive(Debug, Serialize)]
struct SessionResponse {
    id: String,
    human_id: String,
    session_type: String,
    status: String,
    user_id: Option<String>,
    context: Value,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct SessionDetailResponse {
    session: SessionResponse,
    flows: Vec<FlowResponse>,
}

#[derive(Debug, Serialize)]
struct FlowResponse {
    id: String,
    human_id: String,
    session_id: String,
    flow_type: String,
    status: String,
    current_step: Option<String>,
    step_ids: Value,
    context: Value,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct FlowDetailResponse {
    flow: FlowResponse,
    steps: Vec<StepResponse>,
}

#[derive(Debug, Serialize)]
struct StepResponse {
    id: String,
    human_id: String,
    flow_id: String,
    step_type: String,
    actor: String,
    status: String,
    attempt_no: i32,
    input: Option<Value>,
    output: Option<Value>,
    error: Option<Value>,
    next_retry_at: Option<chrono::DateTime<Utc>>,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
    finished_at: Option<chrono::DateTime<Utc>>,
}

async fn create_session(
    State(api): State<BackendApi>,
    headers: HeaderMap,
    Json(body): Json<CreateSessionRequest>,
) -> Result<Json<SessionResponse>, Error> {
    let user_id = require_user_id(&api, &headers).await?;
    let session_id = backend_id::flow_session_id()?;
    let human_id = normalize_or_default_human_id(
        body.human_id,
        format!("kyc.{}.{}", Utc::now().format("%Y-%m-%d"), session_id),
    )?;

    let mut context = body.context.unwrap_or_else(|| json!({}));
    if !context.is_object() {
        context = json!({});
    }

    let session = api
        .state
        .flow
        .create_session(FlowSessionCreateInput {
            id: session_id,
            human_id,
            user_id: Some(user_id),
            session_type: body.session_type,
            status: "OPEN".to_owned(),
            context,
        })
        .await?;

    Ok(Json(to_session_response(session)))
}

async fn list_sessions(
    State(api): State<BackendApi>,
    headers: HeaderMap,
) -> Result<Json<Vec<SessionResponse>>, Error> {
    let user_id = require_user_id(&api, &headers).await?;

    let (sessions, _) = api
        .state
        .flow
        .list_sessions(FlowSessionFilter {
            user_id: Some(user_id),
            session_type: None,
            status: None,
            page: 1,
            limit: 100,
        })
        .await?;

    Ok(Json(
        sessions
            .into_iter()
            .map(to_session_response)
            .collect::<Vec<_>>(),
    ))
}

async fn get_session(
    State(api): State<BackendApi>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<SessionDetailResponse>, Error> {
    let user_id = require_user_id(&api, &headers).await?;

    let session = api
        .state
        .flow
        .get_session(&session_id)
        .await?
        .ok_or_else(|| Error::not_found("SESSION_NOT_FOUND", "Session not found"))?;

    if session.user_id.as_deref() != Some(user_id.as_str()) {
        return Err(Error::unauthorized("Session does not belong to caller"));
    }

    let flows = api.state.flow.list_flows_for_session(&session.id).await?;

    Ok(Json(SessionDetailResponse {
        session: to_session_response(session),
        flows: flows.into_iter().map(to_flow_response).collect(),
    }))
}

async fn list_session_flows(
    State(api): State<BackendApi>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Vec<FlowResponse>>, Error> {
    let user_id = require_user_id(&api, &headers).await?;
    ensure_session_owner(&api, &session_id, &user_id).await?;

    let flows = api.state.flow.list_flows_for_session(&session_id).await?;
    Ok(Json(
        flows.into_iter().map(to_flow_response).collect::<Vec<_>>(),
    ))
}

async fn add_flow_to_session(
    State(api): State<BackendApi>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<AddFlowRequest>,
) -> Result<Json<FlowResponse>, Error> {
    let user_id = require_user_id(&api, &headers).await?;
    let session = ensure_session_owner(&api, &session_id, &user_id).await?;

    let flow_id = backend_id::flow_instance_id()?;
    let step_id = backend_id::flow_step_id()?;
    let flow_human_id = normalize_or_default_human_id(
        body.human_id,
        format!("{}.{}", session.human_id, body.flow_type.to_lowercase()),
    )?;

    let step_type = body
        .initial_step
        .clone()
        .unwrap_or_else(|| "START".to_owned());

    let flow = api
        .state
        .flow
        .create_flow(FlowInstanceCreateInput {
            id: flow_id.clone(),
            human_id: flow_human_id.clone(),
            session_id: session_id.clone(),
            flow_type: body.flow_type.clone(),
            status: "RUNNING".to_owned(),
            current_step: Some(step_type.clone()),
            step_ids: json!([step_id]),
            context: body.context.unwrap_or_else(|| json!({})),
        })
        .await?;

    api.state
        .flow
        .create_step(FlowStepCreateInput {
            id: step_id,
            human_id: format!("{}.{}", flow_human_id, step_type.to_lowercase()),
            flow_id,
            step_type,
            actor: "END_USER".to_owned(),
            status: "WAITING".to_owned(),
            attempt_no: 0,
            input: None,
            output: None,
            error: None,
            next_retry_at: None,
            finished_at: None,
        })
        .await?;

    Ok(Json(to_flow_response(flow)))
}

async fn get_flow(
    State(api): State<BackendApi>,
    Path(flow_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<FlowDetailResponse>, Error> {
    let user_id = require_user_id(&api, &headers).await?;

    let flow = api
        .state
        .flow
        .get_flow(&flow_id)
        .await?
        .ok_or_else(|| Error::not_found("FLOW_NOT_FOUND", "Flow not found"))?;
    ensure_flow_owner(&api, &flow, &user_id).await?;

    let steps = api.state.flow.list_steps_for_flow(&flow_id).await?;

    Ok(Json(FlowDetailResponse {
        flow: to_flow_response(flow),
        steps: steps.into_iter().map(to_step_response).collect(),
    }))
}

async fn list_flow_steps(
    State(api): State<BackendApi>,
    Path(flow_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Vec<StepResponse>>, Error> {
    let user_id = require_user_id(&api, &headers).await?;

    let flow = api
        .state
        .flow
        .get_flow(&flow_id)
        .await?
        .ok_or_else(|| Error::not_found("FLOW_NOT_FOUND", "Flow not found"))?;
    ensure_flow_owner(&api, &flow, &user_id).await?;

    let steps = api.state.flow.list_steps_for_flow(&flow_id).await?;
    Ok(Json(steps.into_iter().map(to_step_response).collect()))
}

async fn get_step(
    State(api): State<BackendApi>,
    Path(step_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<StepResponse>, Error> {
    let user_id = require_user_id(&api, &headers).await?;
    let step = api
        .state
        .flow
        .get_step(&step_id)
        .await?
        .ok_or_else(|| Error::not_found("STEP_NOT_FOUND", "Step not found"))?;

    let flow = api
        .state
        .flow
        .get_flow(&step.flow_id)
        .await?
        .ok_or_else(|| Error::not_found("FLOW_NOT_FOUND", "Flow not found"))?;
    ensure_flow_owner(&api, &flow, &user_id).await?;

    Ok(Json(to_step_response(step)))
}

async fn submit_step(
    State(api): State<BackendApi>,
    Path(step_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<SubmitStepRequest>,
) -> Result<Json<StepResponse>, Error> {
    let user_id = require_user_id(&api, &headers).await?;
    let step = api
        .state
        .flow
        .get_step(&step_id)
        .await?
        .ok_or_else(|| Error::not_found("STEP_NOT_FOUND", "Step not found"))?;

    let flow = api
        .state
        .flow
        .get_flow(&step.flow_id)
        .await?
        .ok_or_else(|| Error::not_found("FLOW_NOT_FOUND", "Flow not found"))?;
    ensure_flow_owner(&api, &flow, &user_id).await?;

    let updated = api
        .state
        .flow
        .patch_step(
            &step_id,
            FlowStepPatch::new()
                .status("COMPLETED")
                .input(body.input)
                .output(json!({"accepted": true}))
                .clear_error()
                .finished_at(Utc::now()),
        )
        .await?;

    let _ = api
        .state
        .flow
        .update_flow(
            &flow.id,
            Some("COMPLETED".to_owned()),
            Some(None),
            None,
            None,
        )
        .await;

    Ok(Json(to_step_response(updated)))
}

async fn require_user_id(api: &BackendApi, headers: &HeaderMap) -> Result<String, Error> {
    let auth = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| Error::unauthorized("Missing bearer token"))?;
    let token = auth
        .strip_prefix("Bearer ")
        .or_else(|| auth.strip_prefix("bearer "))
        .ok_or_else(|| Error::unauthorized("Missing bearer token"))?;

    let claims = JwtToken::verify(token, &api.oidc_state)
        .await
        .map_err(|error| Error::unauthorized(error.to_string()))?;
    BackendApi::require_user_id(&claims)
}

async fn ensure_session_owner(
    api: &BackendApi,
    session_id: &str,
    user_id: &str,
) -> Result<backend_model::db::FlowSessionRow, Error> {
    let session = api
        .state
        .flow
        .get_session(session_id)
        .await?
        .ok_or_else(|| Error::not_found("SESSION_NOT_FOUND", "Session not found"))?;

    if session.user_id.as_deref() != Some(user_id) {
        return Err(Error::unauthorized("Session does not belong to caller"));
    }

    Ok(session)
}

async fn ensure_flow_owner(
    api: &BackendApi,
    flow: &backend_model::db::FlowInstanceRow,
    user_id: &str,
) -> Result<(), Error> {
    let session = api
        .state
        .flow
        .get_session(&flow.session_id)
        .await?
        .ok_or_else(|| Error::not_found("SESSION_NOT_FOUND", "Session not found"))?;

    if session.user_id.as_deref() != Some(user_id) {
        return Err(Error::unauthorized("Flow does not belong to caller"));
    }

    Ok(())
}

fn normalize_or_default_human_id(value: Option<String>, fallback: String) -> Result<String, Error> {
    let candidate = value.unwrap_or(fallback);
    HumanReadableId::parse(candidate.clone())
        .map_err(|error| Error::bad_request("INVALID_HUMAN_ID", error.to_string()))?;
    Ok(candidate)
}

fn to_session_response(row: backend_model::db::FlowSessionRow) -> SessionResponse {
    SessionResponse {
        id: row.id,
        human_id: row.human_id,
        session_type: row.session_type,
        status: row.status,
        user_id: row.user_id,
        context: row.context,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn to_flow_response(row: backend_model::db::FlowInstanceRow) -> FlowResponse {
    FlowResponse {
        id: row.id,
        human_id: row.human_id,
        session_id: row.session_id,
        flow_type: row.flow_type,
        status: row.status,
        current_step: row.current_step,
        step_ids: row.step_ids,
        context: row.context,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn to_step_response(row: backend_model::db::FlowStepRow) -> StepResponse {
    StepResponse {
        id: row.id,
        human_id: row.human_id,
        flow_id: row.flow_id,
        step_type: row.step_type,
        actor: row.actor,
        status: row.status,
        attempt_no: row.attempt_no,
        input: row.input,
        output: row.output,
        error: row.error,
        next_retry_at: row.next_retry_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
        finished_at: row.finished_at,
    }
}
