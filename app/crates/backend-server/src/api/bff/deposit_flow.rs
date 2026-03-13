use super::super::BackendApi;
use super::shared::{
    DEPOSIT_STEP_TYPE, ensure_user_match, normalized_user_id, step_id, upsert_step_id_in_context,
    user_id_matches,
};
use crate::state_machine::engine::Engine;
use crate::state_machine::types::{KIND_KYC_FIRST_DEPOSIT, STEP_DEPOSIT_AWAIT_PAYMENT};
use backend_auth::JwtToken;
use backend_core::Error;
use chrono::{Duration, Utc};
use gen_oas_server_bff::apis::deposits::{
    InternalCreatePhoneDepositRequestResponse, InternalGetPhoneDepositRequestResponse,
};
use gen_oas_server_bff::models;
use serde_json::Value;
use tracing::instrument;

#[backend_core::async_trait]
pub(super) trait DepositFlow {
    async fn create_phone_deposit_request_flow(
        &self,
        claims: &JwtToken,
        body: &models::CreatePhoneDepositRequest,
    ) -> Result<InternalCreatePhoneDepositRequestResponse, Error>;

    async fn get_phone_deposit_request_flow(
        &self,
        claims: &JwtToken,
        path_params: &models::InternalGetPhoneDepositRequestPathParams,
    ) -> Result<InternalGetPhoneDepositRequestResponse, Error>;
}

#[backend_core::async_trait]
impl DepositFlow for BackendApi {
    #[instrument(skip(self))]
    async fn create_phone_deposit_request_flow(
        &self,
        claims: &JwtToken,
        body: &models::CreatePhoneDepositRequest,
    ) -> Result<InternalCreatePhoneDepositRequestResponse, Error> {
        ensure_user_match(claims, &body.user_id)?;
        let user_id = normalized_user_id(&body.user_id);

        let mut instance = self
            .state
            .sm
            .get_instance(&body.session_id)
            .await?
            .ok_or_else(|| Error::not_found("SESSION_NOT_FOUND", "Session not found"))?;

        if instance.kind != KIND_KYC_FIRST_DEPOSIT {
            return Err(Error::bad_request(
                "INVALID_SESSION_KIND",
                "Session is not a FIRST_DEPOSIT flow",
            ));
        }
        if !user_id_matches(instance.user_id.as_deref(), &user_id) {
            return Err(Error::unauthorized(
                "Session does not belong to authenticated user",
            ));
        }

        let mut context = instance.context.clone();
        let mut changed = false;

        let deposit_exists = context
            .get("deposit")
            .and_then(Value::as_object)
            .is_some_and(|deposit| !deposit.is_empty());

        if !deposit_exists {
            let user = self
                .state
                .user
                .get_user(&user_id)
                .await?
                .ok_or_else(|| Error::not_found("USER_NOT_FOUND", "User not found"))?;
            let user_phone_number = user.phone_number.ok_or_else(|| {
                Error::bad_request("USER_PHONE_REQUIRED", "User phone number is required")
            })?;
            let recipient = self
                .state
                .sm
                .select_deposit_recipient_contact(&user_phone_number, &body.currency)
                .await?;

            let expires_at = Utc::now() + Duration::hours(2);

            if !context.is_object() {
                context = Value::Object(Default::default());
            }

            if let Some(obj) = context.as_object_mut() {
                let contact_id =
                    format!("dep_recipient_{}", recipient.provider.to_ascii_lowercase());
                obj.insert(
                    "deposit".to_owned(),
                    serde_json::json!({
                        "amount": body.amount,
                        "currency": recipient.currency,
                        "reason": body.reason,
                        "reference": body.reference,
                        "provider": recipient.provider,
                        "status": "CONTACT_PROVIDED",
                        "expires_at": expires_at,
                        "contact": {
                            "staff_id": contact_id,
                            "full_name": recipient.full_name,
                            "phone_number": recipient.phone_number
                        }
                    }),
                );
                changed = true;
            }
        }

        let kyc_step_id = step_id(&instance.id, DEPOSIT_STEP_TYPE);
        if upsert_step_id_in_context(&mut context, &kyc_step_id) {
            changed = true;
        }

        if changed {
            self.state
                .sm
                .update_instance_context(&instance.id, context)
                .await?;
            instance = self
                .state
                .sm
                .get_instance(&instance.id)
                .await?
                .ok_or_else(|| Error::not_found("SESSION_NOT_FOUND", "Session not found"))?;
        }

        let engine = Engine::new(self.state.clone());
        engine
            .ensure_manual_step_running(&instance.id, STEP_DEPOSIT_AWAIT_PAYMENT)
            .await?;

        Ok(
            InternalCreatePhoneDepositRequestResponse::Status201_DepositRequestCreated(
                phone_deposit_from_instance(instance)?,
            ),
        )
    }

    #[instrument(skip(self))]
    async fn get_phone_deposit_request_flow(
        &self,
        claims: &JwtToken,
        path_params: &models::InternalGetPhoneDepositRequestPathParams,
    ) -> Result<InternalGetPhoneDepositRequestResponse, Error> {
        let user_id = BackendApi::require_user_id(claims)?;

        let instance = self
            .state
            .sm
            .get_instance(&path_params.deposit_request_id)
            .await?
            .ok_or_else(|| Error::not_found("DEPOSIT_NOT_FOUND", "Deposit request not found"))?;

        if instance.kind != KIND_KYC_FIRST_DEPOSIT {
            return Err(Error::not_found(
                "DEPOSIT_NOT_FOUND",
                "Deposit request not found",
            ));
        }
        if !user_id_matches(instance.user_id.as_deref(), &user_id) {
            return Err(Error::unauthorized(
                "Deposit request does not belong to authenticated user",
            ));
        }

        Ok(
            InternalGetPhoneDepositRequestResponse::Status200_DepositRequest(
                phone_deposit_from_instance(instance)?,
            ),
        )
    }
}

fn parse_deposit_status(raw: &str) -> Result<models::DepositStatus, Error> {
    raw.parse::<models::DepositStatus>().map_err(|_| {
        Error::internal(
            "INVALID_DEPOSIT_STATUS",
            format!("Unsupported deposit status: {raw}"),
        )
    })
}

fn parse_deposit_provider(raw: &str) -> Result<models::DepositProvider, Error> {
    raw.parse::<models::DepositProvider>().map_err(|_| {
        Error::internal(
            "INVALID_DEPOSIT_PROVIDER",
            format!("Unsupported deposit provider: {raw}"),
        )
    })
}

fn phone_deposit_from_instance(
    instance: backend_model::db::SmInstanceRow,
) -> Result<models::PhoneDepositResponse, Error> {
    let deposit = instance
        .context
        .get("deposit")
        .cloned()
        .unwrap_or(Value::Null);

    let raw_status = deposit
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("CONTACT_PROVIDED");
    let amount = deposit.get("amount").and_then(Value::as_f64).unwrap_or(0.0);
    let provider = deposit
        .get("provider")
        .and_then(Value::as_str)
        .map(parse_deposit_provider)
        .transpose()?;
    let currency = deposit
        .get("currency")
        .and_then(Value::as_str)
        .unwrap_or("XAF")
        .to_owned();

    let contact = deposit.get("contact").cloned().unwrap_or(Value::Null);
    let staff_id = contact
        .get("staff_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let full_name = contact
        .get("full_name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let phone_number = contact
        .get("phone_number")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();

    let expires_at = deposit
        .get("expires_at")
        .and_then(Value::as_str)
        .and_then(|raw| chrono::DateTime::parse_from_rfc3339(raw).ok())
        .map(|parsed| parsed.with_timezone(&Utc));
    let effective_status = if matches!(raw_status, "CREATED" | "CONTACT_PROVIDED")
        && expires_at.map(|date| date < Utc::now()).unwrap_or(false)
    {
        "EXPIRED"
    } else {
        raw_status
    };

    Ok(models::PhoneDepositResponse {
        deposit_request_id: instance.id.clone(),
        session_id: instance.id.clone(),
        step_id: Some(step_id(&instance.id, DEPOSIT_STEP_TYPE)),
        status: parse_deposit_status(effective_status)?,
        provider,
        amount,
        currency,
        contact: models::StaffContact {
            staff_id,
            full_name,
            phone_number,
        },
        expires_at,
        created_at: instance.created_at,
    })
}
