use super::BackendApi;
use crate::state_machine::engine::Engine;
use crate::state_machine::types::*;
use axum_extra::extract::CookieJar;
use backend_auth::JwtToken;
use backend_core::Error;
use backend_repository::{SmInstanceFilter, SmStepAttemptCreateInput};
use gen_oas_server_staff::apis::kyc_state_machines::{
    KycStateMachines, StaffKycDepositsInstanceIdApprovePostResponse,
    StaffKycDepositsInstanceIdConfirmPaymentPostResponse, StaffKycInstancesGetResponse,
    StaffKycInstancesInstanceIdGetResponse, StaffKycInstancesInstanceIdRetryPostResponse,
    StaffKycReportsSummaryGetResponse,
};
use gen_oas_server_staff::models;
use headers::Host;
use http::Method;
use serde_json::{Value, json};

#[backend_core::async_trait]
impl KycStateMachines<Error> for BackendApi {
    type Claims = JwtToken;

    async fn staff_kyc_instances_get(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        query_params: &models::StaffKycInstancesGetQueryParams,
    ) -> Result<StaffKycInstancesGetResponse, Error> {
        let page = query_params.page.unwrap_or(1).max(1);
        let limit = query_params.limit.unwrap_or(20).clamp(1, 100);

        let filter = SmInstanceFilter {
            kind: query_params.kind.map(|k| k.to_string()),
            status: query_params.status.map(|s| s.to_string()),
            user_id: query_params.user_id.clone(),
            phone_number: query_params.phone_number.clone(),
            created_from: query_params.created_from,
            created_to: query_params.created_to,
            page,
            limit,
        };

        let (rows, total) = self.state.sm.list_instances(filter).await?;

        // Best-effort: compute current step + last error using attempts.
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let attempts = self.state.sm.list_step_attempts(&row.id).await?;
            let (current_step, last_error) = summarize_instance(&row.kind, &attempts);

            items.push(models::KycInstanceSummary {
                instance_id: row.id,
                kind: parse_kind(&row.kind),
                user_id: row.user_id,
                status: parse_status(&row.status),
                current_step,
                last_error: last_error.and_then(json_to_object),
                created_at: row.created_at,
                updated_at: row.updated_at,
            });
        }

        Ok(StaffKycInstancesGetResponse::Status200_PageOfInstances(
            models::KycInstancesResponse {
                items,
                total: i32::try_from(total).unwrap_or(i32::MAX),
                page,
                page_size: limit,
            },
        ))
    }

    async fn staff_kyc_instances_instance_id_get(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        path_params: &models::StaffKycInstancesInstanceIdGetPathParams,
    ) -> Result<StaffKycInstancesInstanceIdGetResponse, Error> {
        let Some(instance) = self.state.sm.get_instance(&path_params.instance_id).await? else {
            return Ok(StaffKycInstancesInstanceIdGetResponse::Status404_InstanceNotFound);
        };

        let attempts = self.state.sm.list_step_attempts(&instance.id).await?;
        let events = self.state.sm.list_events(&instance.id).await?;

        let step_states = build_step_states(&instance.kind, &attempts)?;
        let event_dtos = events
            .into_iter()
            .map(|e| models::KycEvent {
                id: e.id,
                kind: e.kind,
                actor_type: e.actor_type,
                actor_id: e.actor_id,
                payload: json_to_object(e.payload).unwrap_or_default(),
                created_at: e.created_at,
            })
            .collect::<Vec<_>>();

        let context = json_to_object(instance.context).unwrap_or_default();

        Ok(
            StaffKycInstancesInstanceIdGetResponse::Status200_InstanceDetail(
                models::KycInstanceDetail {
                    instance_id: instance.id,
                    kind: parse_kind(&instance.kind),
                    user_id: instance.user_id,
                    status: parse_status(&instance.status),
                    context,
                    steps: step_states,
                    events: event_dtos,
                    created_at: instance.created_at,
                    updated_at: instance.updated_at,
                    completed_at: instance.completed_at,
                },
            ),
        )
    }

    async fn staff_kyc_instances_instance_id_retry_post(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
        path_params: &models::StaffKycInstancesInstanceIdRetryPostPathParams,
        body: &models::RetryRequest,
    ) -> Result<StaffKycInstancesInstanceIdRetryPostResponse, Error> {
        let Some(instance) = self.state.sm.get_instance(&path_params.instance_id).await? else {
            return Ok(StaffKycInstancesInstanceIdRetryPostResponse::Status404_InstanceNotFound);
        };

        let step_name = body.step_name.trim();
        if step_name.is_empty() {
            return Err(Error::bad_request("INVALID_STEP", "stepName is required"));
        }

        if !steps_for_kind(&instance.kind).contains(&step_name) {
            return Err(Error::bad_request(
                "INVALID_STEP",
                "stepName is not valid for this instance kind",
            ));
        }

        if !is_retryable_async_step(&instance.kind, step_name) {
            return Err(Error::bad_request(
                "INVALID_RETRY_STEP",
                "Only async steps can be retried with this endpoint",
            ));
        }

        let attempt_no = self
            .state
            .sm
            .next_attempt_no(&instance.id, step_name)
            .await?;
        let now = chrono::Utc::now();

        let attempt = self
            .state
            .sm
            .create_step_attempt(SmStepAttemptCreateInput {
                id: backend_id::sm_attempt_id()?,
                instance_id: instance.id.clone(),
                step_name: step_name.to_owned(),
                attempt_no,
                status: ATTEMPT_STATUS_QUEUED.to_owned(),
                external_ref: None,
                input: json!({"retry_mode": body.mode.to_string()}),
                output: None,
                error: None,
                queued_at: Some(now),
                started_at: None,
                finished_at: None,
                next_retry_at: None,
            })
            .await?;

        self.state
            .sm
            .cancel_other_attempts_for_step(&instance.id, step_name, &attempt.id)
            .await?;

        self.state
            .sm_queue
            .enqueue(crate::state_machine::jobs::StateMachineStepJob {
                instance_id: instance.id.clone(),
                step_name: step_name.to_owned(),
                attempt_id: attempt.id.clone(),
            })
            .await
            .map_err(|err| Error::internal("SM_ENQUEUE_FAILED", err.to_string()))?;

        Ok(
            StaffKycInstancesInstanceIdRetryPostResponse::Status200_RetryAccepted(
                models::RetryResponse {
                    instance_id: instance.id,
                    step_name: step_name.to_owned(),
                    attempt_id: attempt.id,
                },
            ),
        )
    }

    async fn staff_kyc_deposits_instance_id_confirm_payment_post(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        path_params: &models::StaffKycDepositsInstanceIdConfirmPaymentPostPathParams,
        body: &models::ConfirmPaymentRequest,
    ) -> Result<StaffKycDepositsInstanceIdConfirmPaymentPostResponse, Error> {
        let Some(instance) = self.state.sm.get_instance(&path_params.instance_id).await? else {
            return Ok(
                StaffKycDepositsInstanceIdConfirmPaymentPostResponse::Status404_InstanceNotFound,
            );
        };
        if instance.kind != KIND_KYC_FIRST_DEPOSIT {
            return Err(Error::bad_request(
                "INVALID_INSTANCE_KIND",
                "Instance is not a first-deposit KYC",
            ));
        }

        // Persist context update.
        let mut ctx = instance.context;
        let payment = json!({
            "confirmed_at": body.confirmed_at.unwrap_or_else(chrono::Utc::now),
            "note": body.note,
            "provider_txn_id": body.provider_txn_id,
        });
        if let Some(obj) = ctx.as_object_mut() {
            obj.insert("payment".to_owned(), payment.clone());
        }
        self.state
            .sm
            .update_instance_context(&instance.id, ctx)
            .await?;

        let reviewer_id = BackendApi::require_user_id(claims).ok();
        let engine = Engine::new(self.state.clone());
        engine
            .staff_confirm_deposit_payment(&instance.id, reviewer_id, payment)
            .await?;

        Ok(StaffKycDepositsInstanceIdConfirmPaymentPostResponse::Status200_PaymentConfirmationRecorded)
    }

    async fn staff_kyc_deposits_instance_id_approve_post(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        claims: &Self::Claims,
        path_params: &models::StaffKycDepositsInstanceIdApprovePostPathParams,
        body: &models::DepositApproveRequest,
    ) -> Result<StaffKycDepositsInstanceIdApprovePostResponse, Error> {
        let Some(instance) = self.state.sm.get_instance(&path_params.instance_id).await? else {
            return Ok(StaffKycDepositsInstanceIdApprovePostResponse::Status404_InstanceNotFound);
        };
        if instance.kind != KIND_KYC_FIRST_DEPOSIT {
            return Err(Error::bad_request(
                "INVALID_INSTANCE_KIND",
                "Instance is not a first-deposit KYC",
            ));
        }

        // Persist approval context.
        let mut ctx = instance.context;
        let approval = json!({
            "first_name": body.first_name,
            "last_name": body.last_name,
            "deposit_amount": body.deposit_amount,
        });
        if let Some(obj) = ctx.as_object_mut() {
            obj.insert("approval".to_owned(), approval.clone());
        }
        self.state
            .sm
            .update_instance_context(&instance.id, ctx)
            .await?;

        let reviewer_id = BackendApi::require_user_id(claims).ok();
        let engine = Engine::new(self.state.clone());
        engine
            .staff_approve_deposit(&instance.id, reviewer_id, approval)
            .await?;

        Ok(StaffKycDepositsInstanceIdApprovePostResponse::Status200_ApprovalRecorded)
    }

    async fn staff_kyc_reports_summary_get(
        &self,
        _method: &Method,
        _host: &Host,
        _cookies: &CookieJar,
        _claims: &Self::Claims,
    ) -> Result<StaffKycReportsSummaryGetResponse, Error> {
        // Minimal summary. (Future: SQL aggregation.)
        let (rows, _total) = self
            .state
            .sm
            .list_instances(SmInstanceFilter {
                kind: None,
                status: None,
                user_id: None,
                phone_number: None,
                created_from: None,
                created_to: None,
                page: 1,
                limit: 100,
            })
            .await?;

        let mut by_kind = std::collections::HashMap::<String, i32>::new();
        let mut by_status = std::collections::HashMap::<String, i32>::new();
        for row in rows {
            *by_kind.entry(row.kind).or_insert(0) += 1;
            *by_status.entry(row.status).or_insert(0) += 1;
        }

        Ok(StaffKycReportsSummaryGetResponse::Status200_SummaryReport(
            models::ReportSummary {
                by_kind,
                by_status,
                failures_last24h: 0,
            },
        ))
    }
}

fn parse_kind(raw: &str) -> models::InstanceKind {
    raw.parse().unwrap_or(models::InstanceKind::KycPhoneOtp)
}

fn parse_status(raw: &str) -> models::InstanceStatus {
    raw.parse().unwrap_or(models::InstanceStatus::Active)
}

fn json_to_object(
    value: Value,
) -> Option<std::collections::HashMap<String, gen_oas_server_staff::types::Object>> {
    let Value::Object(map) = value else {
        return None;
    };
    Some(
        map.into_iter()
            .map(|(k, v)| (k, gen_oas_server_staff::types::Object(v)))
            .collect(),
    )
}

fn summarize_instance(
    kind: &str,
    attempts: &[backend_model::db::SmStepAttemptRow],
) -> (Option<String>, Option<Value>) {
    let steps = steps_for_kind(kind);
    let mut last_error = None;
    for attempt in attempts.iter().rev() {
        if attempt.status == ATTEMPT_STATUS_FAILED {
            last_error = attempt.error.clone();
            break;
        }
    }

    for step in steps {
        let latest = attempts
            .iter()
            .filter(|a| a.step_name == step)
            .max_by_key(|a| a.attempt_no);
        if latest.is_none() {
            return (Some(step.to_owned()), last_error);
        }
        if let Some(latest) = latest
            && latest.status != ATTEMPT_STATUS_SUCCEEDED
        {
            return (Some(step.to_owned()), last_error);
        }
    }

    (None, last_error)
}

fn steps_for_kind(kind: &str) -> Vec<&'static str> {
    match kind {
        KIND_KYC_FIRST_DEPOSIT => vec![
            STEP_DEPOSIT_AWAIT_PAYMENT,
            STEP_DEPOSIT_AWAIT_APPROVAL,
            STEP_DEPOSIT_REGISTER_CUSTOMER,
            STEP_DEPOSIT_APPROVE_AND_DEPOSIT,
            STEP_MARK_COMPLETE,
        ],
        _ => vec![
            STEP_PHONE_ISSUE_OTP,
            STEP_PHONE_VERIFY_OTP,
            STEP_MARK_COMPLETE,
        ],
    }
}

fn is_retryable_async_step(kind: &str, step_name: &str) -> bool {
    match kind {
        KIND_KYC_PHONE_OTP => step_name == STEP_PHONE_ISSUE_OTP,
        KIND_KYC_FIRST_DEPOSIT => {
            step_name == STEP_DEPOSIT_REGISTER_CUSTOMER
                || step_name == STEP_DEPOSIT_APPROVE_AND_DEPOSIT
        }
        _ => false,
    }
}

fn build_step_states(
    kind: &str,
    attempts: &[backend_model::db::SmStepAttemptRow],
) -> Result<Vec<models::StepState>, Error> {
    let steps = steps_for_kind(kind);
    let mut out = Vec::with_capacity(steps.len());
    for step in steps {
        let mut step_attempts = attempts
            .iter()
            .filter(|a| a.step_name == step)
            .cloned()
            .collect::<Vec<_>>();
        step_attempts.sort_by_key(|a| a.attempt_no);
        let latest = step_attempts.last().cloned();

        out.push(models::StepState {
            step_name: step.to_owned(),
            latest_attempt: latest.clone().map(attempt_to_dto),
            attempts: step_attempts.into_iter().map(attempt_to_dto).collect(),
        });
    }
    Ok(out)
}

fn attempt_to_dto(row: backend_model::db::SmStepAttemptRow) -> models::StepAttempt {
    models::StepAttempt {
        id: row.id,
        step_name: row.step_name,
        attempt_no: row.attempt_no,
        status: row
            .status
            .parse()
            .unwrap_or(models::StepAttemptStatus::Queued),
        external_ref: row.external_ref,
        input: json_to_object(row.input).unwrap_or_default(),
        output: row.output.and_then(json_to_object),
        error: row.error.and_then(json_to_object),
        queued_at: row.queued_at,
        started_at: row.started_at,
        finished_at: row.finished_at,
        next_retry_at: row.next_retry_at,
    }
}
