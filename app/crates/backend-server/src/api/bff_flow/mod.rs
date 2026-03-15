mod handlers;
mod models;
mod service;

use axum::{Router, routing::get};

use super::BackendApi;

pub use models::*;

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        handlers::list_sessions,
        handlers::create_session,
        handlers::get_session,
        handlers::list_session_flows,
        handlers::add_flow_to_session,
        handlers::get_flow,
        handlers::list_flow_steps,
        handlers::get_step,
        handlers::submit_step,
    ),
    components(schemas(
        CreateSessionRequest,
        AddFlowRequest,
        SubmitStepRequest,
        SessionResponse,
        SessionDetailResponse,
        FlowResponse,
        FlowDetailResponse,
        StepResponse,
    ))
)]
pub struct BffFlowOpenApi;

pub fn router(api: BackendApi) -> Router {
    Router::new()
        .route(
            "/sessions",
            get(handlers::list_sessions).post(handlers::create_session),
        )
        .route("/sessions/{session_id}", get(handlers::get_session))
        .route(
            "/sessions/{session_id}/flows",
            get(handlers::list_session_flows).post(handlers::add_flow_to_session),
        )
        .route("/flows/{flow_id}", get(handlers::get_flow))
        .route("/flows/{flow_id}/steps", get(handlers::list_flow_steps))
        .route(
            "/steps/{step_id}",
            get(handlers::get_step).post(handlers::submit_step),
        )
        .with_state(api)
}
