use super::BackendApi;
use super::bff_flow::{StepResponse, SubmitStepRequest, service as bff_service};
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::{Json, Router, routing::get};
use backend_auth::JwtToken;
use backend_core::Error;
use backend_flow_sdk::{StepContext, StepOutcome};
use backend_repository::{FlowSessionFilter, FlowStepPatch};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;
use utoipa::{OpenApi, ToSchema};

#[derive(OpenApi)]
#[openapi(
    paths(list_admin_steps, get_admin_step, submit_admin_step),
    components(schemas(AdminStepQuery, SubmitStepRequest, StepResponse)),
    tags((name = "staff-flow", description = "Staff flow v2 endpoints"))
)]
pub struct StaffFlowOpenApi;

pub fn router(api: BackendApi) -> Router {
    Router::new()
        .route("/steps", get(list_admin_steps))
        .route(
            "/steps/{step_id}",
            get(get_admin_step).post(submit_admin_step),
        )
        .with_state(api)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminStepQuery {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub flow_type: Option<String>,
}

#[utoipa::path(
    get,
    path = "/flow/steps",
    params(
        ("status" = Option<String>, Query),
        ("userId" = Option<String>, Query),
        ("flowType" = Option<String>, Query)
    ),
    responses((status = 200, body = [StepResponse])),
    tag = "staff-flow",
    security(("bearerAuth" = []))
)]
#[instrument(skip(api, headers))]
async fn list_admin_steps(
    State(api): State<BackendApi>,
    headers: HeaderMap,
    Query(query): Query<AdminStepQuery>,
) -> Result<Json<Vec<StepResponse>>, Error> {
    let _token = require_staff_token(&api, &headers).await?;

    let (sessions, _) = api
        .state
        .flow
        .list_sessions(FlowSessionFilter {
            user_id: query.user_id.clone(),
            session_type: None,
            status: None,
            page: 1,
            limit: 500,
        })
        .await?;

    let mut steps: Vec<StepResponse> = Vec::new();
    for session in sessions {
        let flows = api.state.flow.list_flows_for_session(&session.id).await?;
        for flow in flows {
            if let Some(flow_type) = query.flow_type.as_deref()
                && !flow.flow_type.eq_ignore_ascii_case(flow_type)
            {
                continue;
            }

            let flow_steps = api.state.flow.list_steps_for_flow(&flow.id).await?;
            for step in flow_steps {
                if !step.actor.eq_ignore_ascii_case("ADMIN") {
                    continue;
                }
                if let Some(status) = query.status.as_deref()
                    && !step.status.eq_ignore_ascii_case(status)
                {
                    continue;
                }
                steps.push(step.into());
            }
        }
    }

    steps.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });

    Ok(Json(steps))
}

#[utoipa::path(
    get,
    path = "/flow/steps/{step_id}",
    params(("step_id" = String, Path)),
    responses((status = 200, body = StepResponse)),
    tag = "staff-flow",
    security(("bearerAuth" = []))
)]
#[instrument(skip(api, headers))]
async fn get_admin_step(
    State(api): State<BackendApi>,
    Path(step_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<StepResponse>, Error> {
    let _token = require_staff_token(&api, &headers).await?;
    let step = get_admin_step_row(&api, &step_id).await?;
    Ok(Json(step.into()))
}

#[utoipa::path(
    post,
    path = "/flow/steps/{step_id}",
    params(("step_id" = String, Path)),
    request_body = SubmitStepRequest,
    responses((status = 200, body = StepResponse)),
    tag = "staff-flow",
    security(("bearerAuth" = []))
)]
#[instrument(skip(api, headers))]
async fn submit_admin_step(
    State(api): State<BackendApi>,
    Path(step_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<SubmitStepRequest>,
) -> Result<Json<StepResponse>, Error> {
    let _token = require_staff_token(&api, &headers).await?;

    let step = get_admin_step_row(&api, &step_id).await?;
    if !step.status.eq_ignore_ascii_case("WAITING") {
        return Err(Error::conflict(
            "STEP_NOT_WAITING",
            "Admin step is not waiting for input",
        ));
    }
    let flow = api
        .state
        .flow
        .get_flow(&step.flow_id)
        .await?
        .ok_or_else(|| Error::not_found("FLOW_NOT_FOUND", "Flow not found"))?;
    let session = api
        .state
        .flow
        .get_session(&flow.session_id)
        .await?
        .ok_or_else(|| Error::not_found("SESSION_NOT_FOUND", "Session not found"))?;

    let flow_definition = bff_service::get_flow_definition(&api, &flow.flow_type)?;
    let step_definition = bff_service::get_step_definition(flow_definition, &step.step_type)?;

    step_definition
        .validate_input(&body.input)
        .await
        .map_err(bff_service::flow_error_to_http)?;

    let verify_context = StepContext {
        session_id: session.id.clone(),
        session_user_id: session.user_id.clone(),
        flow_id: flow.id.clone(),
        step_id: step.id.clone(),
        input: body.input.clone(),
        session_context: session.context.clone(),
        flow_context: flow.context.clone(),
        services: crate::flows::runtime::step_services(api.state.user.clone()),
    };

    let verify_outcome = step_definition
        .verify_input(&verify_context, &body.input)
        .await
        .map_err(bff_service::flow_error_to_http)?;

    let (output_value, context_updates, branch, status) = match verify_outcome {
        StepOutcome::Done { output, updates } => (
            output.unwrap_or_else(|| json!({"verified": true})),
            updates,
            None,
            "COMPLETED",
        ),
        StepOutcome::Branched {
            branch,
            output,
            updates,
        } => (
            output.unwrap_or_else(|| json!({"verified": true})),
            updates,
            Some(branch),
            "COMPLETED",
        ),
        StepOutcome::Failed { error, retryable } => {
            let session_id = flow.session_id.clone();
            let updated = api
                .state
                .flow
                .patch_step(
                    &step_id,
                    FlowStepPatch::new()
                        .status("FAILED")
                        .input(body.input.clone())
                        .error(json!({"error": error, "retryable": retryable}))
                        .finished_at(Utc::now()),
                )
                .await?;

            if let Some(next_step) = crate::flows::runtime::resolve_transition(
                flow_definition,
                &step.step_type,
                None,
                true,
            ) {
                if bff_service::has_flow_step(flow_definition, &next_step) {
                    bff_service::create_step_chain(&api, &session, flow, next_step, None).await?;
                } else {
                    bff_service::finalize_flow(
                        &api,
                        &flow,
                        bff_service::terminal_status(&next_step),
                    )
                    .await?;
                }
                bff_service::refresh_session_status(&api, &session_id).await?;
            } else {
                bff_service::finalize_flow(&api, &flow, "FAILED").await?;
            }

            return Ok(Json(updated.into()));
        }
        StepOutcome::Waiting { .. } | StepOutcome::Retry { .. } => {
            return Err(Error::conflict(
                "INVALID_ADMIN_STEP_OUTCOME",
                "Admin submission must resolve to a terminal verification outcome",
            ));
        }
    };

    let mut updated_flow_context =
        bff_service::store_step_output(flow.context.clone(), &step.step_type, &body.input);
    if let Some(updates) = context_updates {
        if let Some(flow_patch) = updates.flow_context_patch.as_ref() {
            updated_flow_context =
                crate::flows::runtime::merged_json(updated_flow_context, flow_patch);
        }
        bff_service::apply_context_updates(&api, &session, updates).await?;
    }

    let updated_step = api
        .state
        .flow
        .patch_step(
            &step_id,
            FlowStepPatch::new()
                .status(status)
                .input(body.input.clone())
                .output(output_value)
                .clear_error()
                .finished_at(Utc::now()),
        )
        .await?;

    let mut current_flow = api
        .state
        .flow
        .update_flow(&flow.id, None, None, None, Some(updated_flow_context))
        .await?;

    if let Some(next_step) = crate::flows::runtime::resolve_transition(
        flow_definition,
        &updated_step.step_type,
        branch.as_deref(),
        false,
    ) {
        if bff_service::has_flow_step(flow_definition, &next_step) {
            current_flow =
                bff_service::create_step_chain(&api, &session, current_flow, next_step, None)
                    .await?;
        } else {
            current_flow = bff_service::finalize_flow(
                &api,
                &current_flow,
                bff_service::terminal_status(&next_step),
            )
            .await?;
        }
    } else {
        current_flow = bff_service::finalize_flow(&api, &current_flow, "COMPLETED").await?;
    }

    bff_service::refresh_session_status(&api, &current_flow.session_id).await?;
    Ok(Json(updated_step.into()))
}

async fn get_admin_step_row(
    api: &BackendApi,
    step_id: &str,
) -> Result<backend_model::db::FlowStepRow, Error> {
    let step = api
        .state
        .flow
        .get_step(step_id)
        .await?
        .ok_or_else(|| Error::not_found("STEP_NOT_FOUND", "Step not found"))?;

    if !step.actor.eq_ignore_ascii_case("ADMIN") {
        return Err(Error::bad_request(
            "STEP_NOT_ADMIN",
            "Step is not an admin-managed step",
        ));
    }

    Ok(step)
}

async fn require_staff_token(api: &BackendApi, headers: &HeaderMap) -> Result<JwtToken, Error> {
    if !api.state.config.staff.enabled {
        return Ok(JwtToken::new(backend_auth::Claims {
            sub: "usr_auth_disabled".to_owned(),
            name: Some("auth-disabled".to_owned()),
            iss: api.state.config.oauth2.issuer.clone(),
            exp: usize::MAX,
            preferred_username: Some("auth-disabled".to_owned()),
        }));
    }

    let auth_header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    if !auth_header.to_ascii_lowercase().starts_with("bearer ") {
        return Err(Error::unauthorized("Missing bearer token"));
    }

    JwtToken::verify(&auth_header[7..], &api.oidc_state).await
}
