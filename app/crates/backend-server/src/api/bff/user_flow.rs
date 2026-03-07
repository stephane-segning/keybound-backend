use super::super::BackendApi;
use super::shared::{
    KIND_KYC_FIRST_DEPOSIT, KIND_KYC_PHONE_OTP, ensure_user_match, parse_session_status,
    value_to_api_map,
};
use backend_auth::JwtToken;
use backend_core::Error;
use backend_model::db::{SmInstanceRow, UserRow};
use backend_repository::SmInstanceFilter;
use chrono::{DateTime, Utc};
use gen_oas_server_bff::apis::users::{
    InternalGetUserByIdResponse, InternalGetUserKycLevelResponse, InternalGetUserKycSummaryResponse,
};
use gen_oas_server_bff::models;

#[derive(Debug, Clone)]
struct UserKycProjection {
    level: Vec<models::UserKycLevel>,
    phone_otp_verified: bool,
    first_deposit_verified: bool,
    phone_otp_status: Option<models::KycSessionStatus>,
    first_deposit_status: Option<models::KycSessionStatus>,
    latest_session_updated_at: Option<DateTime<Utc>>,
}

#[backend_core::async_trait]
pub(super) trait UserFlow {
    async fn get_user_by_id_flow(
        &self,
        claims: &JwtToken,
        path_params: &models::InternalGetUserByIdPathParams,
    ) -> Result<InternalGetUserByIdResponse, Error>;

    async fn get_user_kyc_level_flow(
        &self,
        claims: &JwtToken,
        path_params: &models::InternalGetUserKycLevelPathParams,
    ) -> Result<InternalGetUserKycLevelResponse, Error>;

    async fn get_user_kyc_summary_flow(
        &self,
        claims: &JwtToken,
        path_params: &models::InternalGetUserKycSummaryPathParams,
    ) -> Result<InternalGetUserKycSummaryResponse, Error>;
}

#[backend_core::async_trait]
impl UserFlow for BackendApi {
    async fn get_user_by_id_flow(
        &self,
        claims: &JwtToken,
        path_params: &models::InternalGetUserByIdPathParams,
    ) -> Result<InternalGetUserByIdResponse, Error> {
        ensure_user_match(claims, &path_params.user_id)?;
        let row = require_user(self, &path_params.user_id).await?;

        Ok(InternalGetUserByIdResponse::Status200_UserRow(
            user_record_from_row(row),
        ))
    }

    async fn get_user_kyc_level_flow(
        &self,
        claims: &JwtToken,
        path_params: &models::InternalGetUserKycLevelPathParams,
    ) -> Result<InternalGetUserKycLevelResponse, Error> {
        ensure_user_match(claims, &path_params.user_id)?;
        require_user(self, &path_params.user_id).await?;

        let projection = build_user_kyc_projection(self, &path_params.user_id).await?;
        let payload = models::UserKycLevelResponse::new(
            path_params.user_id.clone(),
            projection.level,
            projection.phone_otp_verified,
            projection.first_deposit_verified,
        );

        Ok(InternalGetUserKycLevelResponse::Status200_KYCLevel(payload))
    }

    async fn get_user_kyc_summary_flow(
        &self,
        claims: &JwtToken,
        path_params: &models::InternalGetUserKycSummaryPathParams,
    ) -> Result<InternalGetUserKycSummaryResponse, Error> {
        ensure_user_match(claims, &path_params.user_id)?;
        require_user(self, &path_params.user_id).await?;

        let projection = build_user_kyc_projection(self, &path_params.user_id).await?;
        let mut payload =
            models::UserKycSummary::new(path_params.user_id.clone(), projection.level);
        payload.phone_otp_status = projection.phone_otp_status;
        payload.first_deposit_status = projection.first_deposit_status;
        payload.latest_session_updated_at = projection.latest_session_updated_at;

        Ok(InternalGetUserKycSummaryResponse::Status200_KYCSummary(
            payload,
        ))
    }
}

async fn require_user(api: &BackendApi, user_id: &str) -> Result<UserRow, Error> {
    api.state
        .user
        .get_user(user_id)
        .await?
        .ok_or_else(|| Error::not_found("USER_NOT_FOUND", "User not found"))
}

fn user_record_from_row(row: UserRow) -> models::InternalUserRecord {
    models::InternalUserRecord {
        user_id: row.user_id,
        realm: row.realm,
        username: row.username,
        full_name: row.full_name,
        email: row.email,
        email_verified: row.email_verified,
        phone_number: row.phone_number,
        fineract_customer_id: row.fineract_customer_id,
        disabled: row.disabled,
        attributes: row.attributes.and_then(|value| value_to_api_map(&value)),
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

async fn build_user_kyc_projection(
    api: &BackendApi,
    user_id: &str,
) -> Result<UserKycProjection, Error> {
    let phone_otp_instance = latest_instance_for_kind(api, user_id, KIND_KYC_PHONE_OTP).await?;
    let first_deposit_instance =
        latest_instance_for_kind(api, user_id, KIND_KYC_FIRST_DEPOSIT).await?;

    let phone_otp_status = phone_otp_instance
        .as_ref()
        .map(|instance| parse_session_status(&instance.status, &instance.context));
    let first_deposit_status = first_deposit_instance
        .as_ref()
        .map(|instance| parse_session_status(&instance.status, &instance.context));

    let phone_otp_verified = matches!(phone_otp_status, Some(models::KycSessionStatus::Completed));
    let first_deposit_verified = matches!(
        first_deposit_status,
        Some(models::KycSessionStatus::Completed)
    );

    let level = build_user_kyc_levels(phone_otp_verified, first_deposit_verified);

    let latest_session_updated_at = [phone_otp_instance, first_deposit_instance]
        .into_iter()
        .flatten()
        .map(|instance| instance.updated_at)
        .max();

    Ok(UserKycProjection {
        level,
        phone_otp_verified,
        first_deposit_verified,
        phone_otp_status,
        first_deposit_status,
        latest_session_updated_at,
    })
}

fn build_user_kyc_levels(
    phone_otp_verified: bool,
    first_deposit_verified: bool,
) -> Vec<models::UserKycLevel> {
    let mut levels = Vec::new();

    if phone_otp_verified {
        levels.push(models::UserKycLevel::PhoneOtpVerified);
    }
    if first_deposit_verified {
        levels.push(models::UserKycLevel::FirstDepositVerified);
    }
    if levels.is_empty() {
        levels.push(models::UserKycLevel::None);
    }

    levels
}

async fn latest_instance_for_kind(
    api: &BackendApi,
    user_id: &str,
    kind: &str,
) -> Result<Option<SmInstanceRow>, Error> {
    let (instances, _) = api
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
            limit: 1,
        })
        .await?;

    Ok(instances.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{
        MockStateMachineRepo, MockUserRepo, TestAppStateBuilder, create_fake_jwt,
    };
    use chrono::{TimeZone, Utc};
    use std::sync::Arc;

    fn build_api(sm: MockStateMachineRepo, user: MockUserRepo) -> BackendApi {
        let state = Arc::new(
            TestAppStateBuilder::new()
                .with_sm(Arc::new(sm))
                .with_user(Arc::new(user))
                .build(),
        );
        BackendApi::new(
            state.clone(),
            state.oidc_state.clone(),
            state.signature_state.clone(),
        )
    }

    fn user_row(user_id: &str) -> backend_model::db::UserRow {
        backend_model::db::UserRow {
            user_id: user_id.to_owned(),
            realm: "realm-a".to_owned(),
            username: "alice".to_owned(),
            full_name: Some("Alice".to_owned()),
            email: Some("alice@example.test".to_owned()),
            email_verified: true,
            phone_number: Some("+237690000001".to_owned()),
            fineract_customer_id: Some("fin_001".to_owned()),
            disabled: false,
            attributes: Some(serde_json::json!({ "tier": "gold" })),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn sm_instance_row(
        id: &str,
        kind: &str,
        status: &str,
        user_id: &str,
        updated_at: chrono::DateTime<Utc>,
    ) -> backend_model::db::SmInstanceRow {
        backend_model::db::SmInstanceRow {
            id: id.to_owned(),
            kind: kind.to_owned(),
            user_id: Some(user_id.to_owned()),
            idempotency_key: format!("idem:{id}"),
            status: status.to_owned(),
            context: serde_json::json!({}),
            created_at: updated_at,
            updated_at,
            completed_at: None,
        }
    }

    fn assert_http_error(err: Error, status: u16, key: &str) {
        match err {
            Error::Http {
                status_code,
                error_key,
                ..
            } => {
                assert_eq!(status_code, status);
                assert_eq!(error_key, key);
            }
            other => panic!("expected http error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn get_user_by_id_returns_user_row() {
        let mut user = MockUserRepo::new();
        user.expect_get_user()
            .times(1)
            .withf(|user_id| user_id == "usr_001")
            .return_once(|_| Ok(Some(user_row("usr_001"))));

        let api = build_api(MockStateMachineRepo::new(), user);
        let claims = create_fake_jwt("usr_001");

        let response = api
            .get_user_by_id_flow(
                &claims,
                &models::InternalGetUserByIdPathParams {
                    user_id: "usr_001".to_owned(),
                },
            )
            .await
            .expect("get user by id should succeed");

        let InternalGetUserByIdResponse::Status200_UserRow(payload) = response;
        assert_eq!(payload.user_id, "usr_001");
        assert_eq!(payload.realm, "realm-a");
        assert_eq!(payload.username, "alice");
        assert_eq!(payload.full_name.as_deref(), Some("Alice"));
        assert_eq!(payload.email.as_deref(), Some("alice@example.test"));
    }

    #[tokio::test]
    async fn get_user_kyc_level_prioritizes_first_deposit_over_phone_otp() {
        let mut user = MockUserRepo::new();
        user.expect_get_user()
            .times(1)
            .return_once(|_| Ok(Some(user_row("usr_001"))));

        let phone = sm_instance_row(
            "ins_phone",
            KIND_KYC_PHONE_OTP,
            "COMPLETED",
            "usr_001",
            Utc::now(),
        );
        let deposit = sm_instance_row(
            "ins_deposit",
            KIND_KYC_FIRST_DEPOSIT,
            "COMPLETED",
            "usr_001",
            Utc::now(),
        );

        let mut sm = MockStateMachineRepo::new();
        let phone_clone = phone.clone();
        sm.expect_list_instances()
            .times(1)
            .withf(|filter| filter.kind.as_deref() == Some(KIND_KYC_PHONE_OTP))
            .return_once(move |_| Ok((vec![phone_clone], 1)));
        sm.expect_list_instances()
            .times(1)
            .withf(|filter| filter.kind.as_deref() == Some(KIND_KYC_FIRST_DEPOSIT))
            .return_once(move |_| Ok((vec![deposit], 1)));

        let api = build_api(sm, user);
        let claims = create_fake_jwt("usr_001");

        let response = api
            .get_user_kyc_level_flow(
                &claims,
                &models::InternalGetUserKycLevelPathParams {
                    user_id: "usr_001".to_owned(),
                },
            )
            .await
            .expect("kyc level should succeed");

        let InternalGetUserKycLevelResponse::Status200_KYCLevel(payload) = response;
        assert_eq!(payload.user_id, "usr_001");
        assert_eq!(
            payload.level,
            vec![
                models::UserKycLevel::PhoneOtpVerified,
                models::UserKycLevel::FirstDepositVerified
            ]
        );
        assert!(payload.phone_otp_verified);
        assert!(payload.first_deposit_verified);
    }

    #[tokio::test]
    async fn get_user_kyc_summary_includes_latest_statuses_and_timestamp() {
        let mut user = MockUserRepo::new();
        user.expect_get_user()
            .times(1)
            .return_once(|_| Ok(Some(user_row("usr_001"))));

        let phone_updated = Utc
            .with_ymd_and_hms(2026, 1, 10, 10, 0, 0)
            .single()
            .expect("valid timestamp");
        let deposit_updated = Utc
            .with_ymd_and_hms(2026, 1, 11, 10, 0, 0)
            .single()
            .expect("valid timestamp");

        let phone = sm_instance_row(
            "ins_phone",
            KIND_KYC_PHONE_OTP,
            "ACTIVE",
            "usr_001",
            phone_updated,
        );
        let deposit = sm_instance_row(
            "ins_deposit",
            KIND_KYC_FIRST_DEPOSIT,
            "COMPLETED",
            "usr_001",
            deposit_updated,
        );

        let mut sm = MockStateMachineRepo::new();
        sm.expect_list_instances()
            .times(1)
            .withf(|filter| filter.kind.as_deref() == Some(KIND_KYC_PHONE_OTP))
            .return_once(move |_| Ok((vec![phone], 1)));
        sm.expect_list_instances()
            .times(1)
            .withf(|filter| filter.kind.as_deref() == Some(KIND_KYC_FIRST_DEPOSIT))
            .return_once(move |_| Ok((vec![deposit], 1)));

        let api = build_api(sm, user);
        let claims = create_fake_jwt("usr_001");

        let response = api
            .get_user_kyc_summary_flow(
                &claims,
                &models::InternalGetUserKycSummaryPathParams {
                    user_id: "usr_001".to_owned(),
                },
            )
            .await
            .expect("kyc summary should succeed");

        let InternalGetUserKycSummaryResponse::Status200_KYCSummary(payload) = response;
        assert_eq!(payload.user_id, "usr_001");
        assert_eq!(
            payload.level,
            vec![models::UserKycLevel::FirstDepositVerified]
        );
        assert_eq!(
            payload.phone_otp_status,
            Some(models::KycSessionStatus::Open)
        );
        assert_eq!(
            payload.first_deposit_status,
            Some(models::KycSessionStatus::Completed)
        );
        assert_eq!(payload.latest_session_updated_at, Some(deposit_updated));
    }

    #[tokio::test]
    async fn get_user_endpoints_reject_mismatched_claims() {
        let api = build_api(MockStateMachineRepo::new(), MockUserRepo::new());
        let claims = create_fake_jwt("usr_other");

        let err = api
            .get_user_by_id_flow(
                &claims,
                &models::InternalGetUserByIdPathParams {
                    user_id: "usr_001".to_owned(),
                },
            )
            .await
            .expect_err("mismatch should be rejected");

        assert_http_error(err, 401, "UNAUTHORIZED");
    }
}
