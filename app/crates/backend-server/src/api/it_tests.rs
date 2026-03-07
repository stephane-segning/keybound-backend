use super::BackendApi;
use crate::file_storage::PresignedUpload;
use crate::state_machine::secrets::hash_secret;
use crate::state_machine::types::{
    ATTEMPT_STATUS_RUNNING, ATTEMPT_STATUS_SUCCEEDED, INSTANCE_STATUS_ACTIVE,
    INSTANCE_STATUS_COMPLETED, INSTANCE_STATUS_RUNNING, KIND_KYC_FIRST_DEPOSIT, KIND_KYC_PHONE_OTP,
    STEP_DEPOSIT_AWAIT_APPROVAL, STEP_DEPOSIT_AWAIT_PAYMENT, STEP_DEPOSIT_REGISTER_CUSTOMER,
    STEP_PHONE_ISSUE_OTP, STEP_PHONE_VERIFY_OTP,
};
use crate::test_utils::{
    MockDeviceRepo, MockMinioStorage, MockNotificationQueue, MockStateMachineQueue,
    MockStateMachineRepo, MockUserRepo, TestAppStateBuilder, create_fake_jwt,
};
use axum::routing::post;
use axum_extra::extract::CookieJar;
use backend_auth::{JwtToken, SignatureContext};
use backend_core::{Config, Error};
use chrono::{Duration, Utc};
use gen_oas_server_bff::apis::deposits::Deposits;
use gen_oas_server_bff::apis::email_magic::EmailMagic;
use gen_oas_server_bff::apis::phone_otp::PhoneOtp;
use gen_oas_server_bff::apis::sessions::Sessions;
use gen_oas_server_bff::apis::steps::Steps;
use gen_oas_server_bff::apis::uploads::Uploads;
use gen_oas_server_kc::apis::devices::Devices;
use gen_oas_server_kc::apis::enrollment::Enrollment;
use gen_oas_server_kc::apis::users::Users;
use gen_oas_server_staff::apis::kyc_state_machines::KycStateMachines;
use headers::Host;
use http::Method;
use http::uri::Authority;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

fn host() -> Host {
    Host::from(Authority::from_static("it.local"))
}

fn cookies() -> CookieJar {
    CookieJar::new()
}

fn claims(user_id: &str) -> JwtToken {
    create_fake_jwt(user_id)
}

fn signature_claims() -> SignatureContext {
    SignatureContext {}
}

fn base_config() -> Config {
    serde_yaml::from_str(
        r#"
server:
  address: "127.0.0.1"
  port: 8080
  tls:
    cert_path: "cert.pem"
    key_path: "key.pem"
logging:
  level: "info"
database:
  url: "postgres://localhost/test"
oauth2:
  issuer: "http://localhost:8081/realms/test"
kc:
  enabled: true
  base_path: "/kc"
  signature_secret: "test-secret"
  max_clock_skew_seconds: 30
  max_body_bytes: 1048576
bff:
  enabled: true
  base_path: "/bff"
staff:
  enabled: true
  base_path: "/staff"
cuss:
  api_url: "http://localhost:8082"
"#,
    )
    .expect("valid base config")
}

fn s3_config() -> Config {
    serde_yaml::from_str(
        r#"
server:
  address: "127.0.0.1"
  port: 8080
  tls:
    cert_path: "cert.pem"
    key_path: "key.pem"
logging:
  level: "info"
database:
  url: "postgres://localhost/test"
oauth2:
  issuer: "http://localhost:8081/realms/test"
s3:
  region: "eu-central-1"
  force_path_style: true
  bucket: "kyc-bucket"
  endpoint: "http://localhost:9000"
  presign_ttl_seconds: 600
kc:
  enabled: true
  base_path: "/kc"
  signature_secret: "test-secret"
  max_clock_skew_seconds: 30
  max_body_bytes: 1048576
bff:
  enabled: true
  base_path: "/bff"
staff:
  enabled: true
  base_path: "/staff"
cuss:
  api_url: "http://localhost:8082"
"#,
    )
    .expect("valid s3 config")
}

fn build_api(
    sm: MockStateMachineRepo,
    user: MockUserRepo,
    device: MockDeviceRepo,
    sm_queue: MockStateMachineQueue,
    notification_queue: MockNotificationQueue,
    minio: MockMinioStorage,
    config: Option<Config>,
) -> BackendApi {
    let mut builder = TestAppStateBuilder::new()
        .with_sm(Arc::new(sm))
        .with_user(Arc::new(user))
        .with_device(Arc::new(device))
        .with_sm_queue(Arc::new(sm_queue))
        .with_notification_queue(Arc::new(notification_queue))
        .with_minio(Arc::new(minio));

    if let Some(cfg) = config {
        builder = builder.with_config(cfg);
    }

    let state = Arc::new(builder.build());
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
        full_name: Some("Alice Tester".to_owned()),
        email: Some("alice@example.test".to_owned()),
        email_verified: true,
        phone_number: Some("+4912345678".to_owned()),
        fineract_customer_id: Some("fin_001".to_owned()),
        disabled: false,
        attributes: Some(json!({"tier":"gold"})),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn device_row(user_id: &str) -> backend_model::db::DeviceRow {
    backend_model::db::DeviceRow {
        device_id: "dvc_001".to_owned(),
        user_id: user_id.to_owned(),
        jkt: "jkt-001".to_owned(),
        public_jwk: r#"{"kty":"EC","kid":"abc"}"#.to_owned(),
        device_record_id: "dvc_001:hash".to_owned(),
        status: "ACTIVE".to_owned(),
        label: Some("iPhone".to_owned()),
        created_at: Utc::now(),
        last_seen_at: Some(Utc::now()),
    }
}

fn sm_instance_row(
    id: &str,
    kind: &str,
    status: &str,
    user_id: Option<&str>,
    context: Value,
) -> backend_model::db::SmInstanceRow {
    backend_model::db::SmInstanceRow {
        id: id.to_owned(),
        kind: kind.to_owned(),
        user_id: user_id.map(str::to_owned),
        idempotency_key: format!("idem:{id}"),
        status: status.to_owned(),
        context,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        completed_at: None,
    }
}

fn sm_attempt_row(
    id: &str,
    instance_id: &str,
    step_name: &str,
    status: &str,
    attempt_no: i32,
    external_ref: Option<&str>,
    output: Option<Value>,
) -> backend_model::db::SmStepAttemptRow {
    backend_model::db::SmStepAttemptRow {
        id: id.to_owned(),
        instance_id: instance_id.to_owned(),
        step_name: step_name.to_owned(),
        attempt_no,
        status: status.to_owned(),
        external_ref: external_ref.map(str::to_owned),
        input: json!({}),
        output,
        error: None,
        queued_at: Some(Utc::now()),
        started_at: Some(Utc::now()),
        finished_at: Some(Utc::now()),
        next_retry_at: None,
    }
}

fn sm_event_row(instance_id: &str) -> backend_model::db::SmEventRow {
    backend_model::db::SmEventRow {
        id: "evt_001".to_owned(),
        instance_id: instance_id.to_owned(),
        kind: "TEST".to_owned(),
        actor_type: "SYSTEM".to_owned(),
        actor_id: None,
        payload: json!({}),
        created_at: Utc::now(),
    }
}

fn assert_http_error(error: Error, expected_status: u16, expected_key: &str) {
    match error {
        Error::Http {
            status_code,
            error_key,
            ..
        } => {
            assert_eq!(status_code, expected_status);
            assert_eq!(error_key, expected_key);
        }
        other => panic!("expected http error, got: {other:?}"),
    }
}

#[tokio::test]
async fn kc_user_crud_and_search_success() {
    let mut user = MockUserRepo::new();
    let created = user_row("usr_001");
    let updated = backend_model::db::UserRow {
        username: "alice-updated".to_owned(),
        ..created.clone()
    };

    user.expect_create_user()
        .times(1)
        .return_once(move |_| Ok(created));
    let updated_for_get = updated.clone();
    user.expect_get_user()
        .times(1)
        .return_once(move |_| Ok(Some(updated_for_get.clone())));
    let updated_for_update = updated.clone();
    user.expect_update_user()
        .times(1)
        .return_once(move |_, _| Ok(Some(updated_for_update.clone())));
    user.expect_delete_user().times(1).return_once(|_| Ok(1));
    user.expect_search_users()
        .times(1)
        .return_once(move |_| Ok(vec![updated]));

    let api = build_api(
        MockStateMachineRepo::new(),
        user,
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );

    let create_body =
        gen_oas_server_kc::models::UserUpsertRequest::new("realm-a".to_owned(), "alice".to_owned());
    let create_resp = api
        .create_user(
            &Method::POST,
            &host(),
            &cookies(),
            &signature_claims(),
            &create_body,
        )
        .await
        .expect("create user response");
    assert!(matches!(
        create_resp,
        gen_oas_server_kc::apis::users::CreateUserResponse::Status201_Created(_)
    ));

    let get_resp = api
        .get_user(
            &Method::GET,
            &host(),
            &cookies(),
            &signature_claims(),
            &gen_oas_server_kc::models::GetUserPathParams {
                user_id: "usr_001".to_owned(),
            },
        )
        .await
        .expect("get user response");
    assert!(matches!(
        get_resp,
        gen_oas_server_kc::apis::users::GetUserResponse::Status200_User(_)
    ));

    let update_body = gen_oas_server_kc::models::UserUpsertRequest::new(
        "realm-a".to_owned(),
        "alice-updated".to_owned(),
    );
    let update_resp = api
        .update_user(
            &Method::PUT,
            &host(),
            &cookies(),
            &signature_claims(),
            &gen_oas_server_kc::models::UpdateUserPathParams {
                user_id: "usr_001".to_owned(),
            },
            &update_body,
        )
        .await
        .expect("update user response");
    assert!(matches!(
        update_resp,
        gen_oas_server_kc::apis::users::UpdateUserResponse::Status200_Updated(_)
    ));

    let delete_resp = api
        .delete_user(
            &Method::DELETE,
            &host(),
            &cookies(),
            &signature_claims(),
            &gen_oas_server_kc::models::DeleteUserPathParams {
                user_id: "usr_001".to_owned(),
            },
        )
        .await
        .expect("delete user response");
    assert!(matches!(
        delete_resp,
        gen_oas_server_kc::apis::users::DeleteUserResponse::Status204_Deleted
    ));

    let search_body = gen_oas_server_kc::models::UserSearchRequest::new("realm-a".to_owned());
    let search_resp = api
        .search_users(
            &Method::POST,
            &host(),
            &cookies(),
            &signature_claims(),
            &search_body,
        )
        .await
        .expect("search users response");
    assert!(matches!(
        search_resp,
        gen_oas_server_kc::apis::users::SearchUsersResponse::Status200_SearchResults(_)
    ));
}

#[tokio::test]
async fn kc_lookup_and_enrollment_success() {
    let mut device = MockDeviceRepo::new();
    device
        .expect_lookup_device()
        .times(1)
        .return_once(|_| Ok(Some(device_row("usr_001"))));
    device
        .expect_find_device_binding()
        .times(1)
        .return_once(|_, _| Ok(None));
    device
        .expect_bind_device()
        .times(1)
        .return_once(|_| Ok("dvc_001:hash".to_owned()));

    let api = build_api(
        MockStateMachineRepo::new(),
        MockUserRepo::new(),
        device,
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );

    let mut lookup = gen_oas_server_kc::models::DeviceLookupRequest::new();
    lookup.device_id = Some("dvc_001".to_owned());
    let lookup_resp = api
        .lookup_device(
            &Method::POST,
            &host(),
            &cookies(),
            &signature_claims(),
            &lookup,
        )
        .await
        .expect("lookup response");
    assert!(matches!(
        lookup_resp,
        gen_oas_server_kc::apis::devices::LookupDeviceResponse::Status200_LookupResult(_)
    ));

    let mut public_jwk = HashMap::new();
    public_jwk.insert(
        "kty".to_owned(),
        gen_oas_server_kc::types::Object(json!("EC")),
    );

    let bind_body = gen_oas_server_kc::models::EnrollmentBindRequest::new(
        "realm-a".to_owned(),
        "client-a".to_owned(),
        "usr_001".to_owned(),
        "dvc_001".to_owned(),
        "jkt-001".to_owned(),
        public_jwk,
    );
    let bind_resp = api
        .enrollment_bind(
            &Method::POST,
            &host(),
            &cookies(),
            &signature_claims(),
            &gen_oas_server_kc::models::EnrollmentBindHeaderParams {
                idempotency_key: None,
            },
            &bind_body,
        )
        .await
        .expect("bind response");
    assert!(matches!(
        bind_resp,
        gen_oas_server_kc::apis::enrollment::EnrollmentBindResponse::Status200_Bound(_)
    ));
}

#[tokio::test]
async fn kc_lookup_device_not_found() {
    let mut device = MockDeviceRepo::new();
    device
        .expect_lookup_device()
        .times(1)
        .return_once(|_| Ok(None));

    let api = build_api(
        MockStateMachineRepo::new(),
        MockUserRepo::new(),
        device,
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );

    let lookup = gen_oas_server_kc::models::DeviceLookupRequest::new();
    let response = api
        .lookup_device(
            &Method::POST,
            &host(),
            &cookies(),
            &signature_claims(),
            &lookup,
        )
        .await
        .expect("lookup response");
    assert!(matches!(
        response,
        gen_oas_server_kc::apis::devices::LookupDeviceResponse::Status404_NotFound(_)
    ));
}

#[tokio::test]
async fn kc_enrollment_conflict() {
    let mut device = MockDeviceRepo::new();
    device
        .expect_find_device_binding()
        .times(1)
        .return_once(|_, _| Ok(Some(("usr_other".to_owned(), "rec_001".to_owned()))));

    let api = build_api(
        MockStateMachineRepo::new(),
        MockUserRepo::new(),
        device,
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );

    let mut public_jwk = HashMap::new();
    public_jwk.insert(
        "kty".to_owned(),
        gen_oas_server_kc::types::Object(json!("EC")),
    );
    let bind_body = gen_oas_server_kc::models::EnrollmentBindRequest::new(
        "realm-a".to_owned(),
        "client-a".to_owned(),
        "usr_001".to_owned(),
        "dvc_001".to_owned(),
        "jkt-001".to_owned(),
        public_jwk,
    );

    let response = api
        .enrollment_bind(
            &Method::POST,
            &host(),
            &cookies(),
            &signature_claims(),
            &gen_oas_server_kc::models::EnrollmentBindHeaderParams {
                idempotency_key: None,
            },
            &bind_body,
        )
        .await
        .expect("bind response");
    assert!(matches!(
        response,
        gen_oas_server_kc::apis::enrollment::EnrollmentBindResponse::Status409_DeviceAlreadyBoundToADifferentUser(_)
    ));
}

#[tokio::test]
async fn bff_session_and_step_success() {
    let session = sm_instance_row(
        "ins_otp_001",
        KIND_KYC_PHONE_OTP,
        INSTANCE_STATUS_ACTIVE,
        Some("usr_001"),
        json!({"step_ids": ["ins_otp_001__PHONE_OTP"]}),
    );

    let mut sm = MockStateMachineRepo::new();
    let for_start = session.clone();
    sm.expect_get_instance_by_idempotency_key()
        .times(1)
        .return_once(move |_| Ok(Some(for_start)));
    sm.expect_get_instance()
        .times(3)
        .returning(move |_| Ok(Some(session.clone())));
    sm.expect_update_instance_context()
        .times(1)
        .return_once(|_, _| Ok(()));
    sm.expect_list_step_attempts()
        .times(2)
        .returning(|_| Ok(vec![]));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );

    let start_resp = api
        .internal_create_kyc_session(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_001"),
            &gen_oas_server_bff::models::CreateKycSessionRequest::new(
                "usr_001".to_owned(),
                gen_oas_server_bff::models::KycFlowType::PhoneOtp,
            ),
        )
        .await
        .expect("start session");
    assert!(matches!(
        start_resp,
        gen_oas_server_bff::apis::sessions::InternalCreateKycSessionResponse::Status201_SessionCreatedOrResumed(_)
    ));

    let create_resp = api
        .internal_create_phone_otp_step(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_001"),
            &gen_oas_server_bff::models::CreateCaseStepRequest::new(
                "ins_otp_001".to_owned(),
                "usr_001".to_owned(),
            ),
        )
        .await
        .expect("create step");
    assert!(matches!(
        create_resp,
        gen_oas_server_bff::apis::phone_otp::InternalCreatePhoneOtpStepResponse::Status201_StepCreated(_)
    ));

    let get_resp = api
        .internal_get_kyc_step(
            &Method::GET,
            &host(),
            &cookies(),
            &claims("usr_001"),
            &gen_oas_server_bff::models::InternalGetKycStepPathParams {
                step_id: "ins_otp_001__PHONE_OTP".to_owned(),
            },
        )
        .await
        .expect("get step");
    assert!(matches!(
        get_resp,
        gen_oas_server_bff::apis::steps::InternalGetKycStepResponse::Status200_Step(_)
    ));
}

#[tokio::test]
async fn bff_issue_and_verify_otp_success() {
    let session = sm_instance_row(
        "ins_otp_002",
        KIND_KYC_PHONE_OTP,
        INSTANCE_STATUS_RUNNING,
        Some("usr_001"),
        json!({"step_ids": ["ins_otp_002__PHONE_OTP"]}),
    );
    let otp_hash = hash_secret("123456").expect("otp hash");

    let issue_attempt = sm_attempt_row(
        "att_issue",
        "ins_otp_002",
        STEP_PHONE_ISSUE_OTP,
        ATTEMPT_STATUS_RUNNING,
        1,
        Some("otp_ref_001"),
        Some(json!({
            "otp_ref": "otp_ref_001",
            "expires_at": (Utc::now() + Duration::minutes(5)).to_rfc3339(),
            "tries_left": 5,
            "otp_hash": null
        })),
    );
    let verify_lookup = sm_attempt_row(
        "att_issue",
        "ins_otp_002",
        STEP_PHONE_ISSUE_OTP,
        ATTEMPT_STATUS_SUCCEEDED,
        1,
        Some("otp_ref_001"),
        Some(json!({
            "expires_at": (Utc::now() + Duration::minutes(5)).to_rfc3339(),
            "tries_left": 5,
            "otp_hash": otp_hash
        })),
    );

    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance()
        .times(2)
        .returning(move |_| Ok(Some(session.clone())));
    sm.expect_list_step_attempts()
        .times(1)
        .return_once(|_| Ok(vec![]));
    sm.expect_next_attempt_no()
        .times(1)
        .return_once(|_, _| Ok(1));
    sm.expect_create_step_attempt()
        .times(1)
        .return_once(move |_| Ok(issue_attempt));
    sm.expect_cancel_other_attempts_for_step()
        .times(1)
        .return_once(|_, _, _| Ok(()));
    sm.expect_update_instance_status()
        .times(2)
        .returning(|_, _, _| Ok(()));
    sm.expect_get_step_attempt_by_external_ref()
        .times(1)
        .return_once(|_, _, _| Ok(Some(verify_lookup)));
    sm.expect_append_event()
        .times(1)
        .return_once(|input| Ok(sm_event_row(&input.instance_id)));
    sm.expect_get_latest_step_attempt()
        .times(1)
        .return_once(|instance_id, step_name| {
            Ok(Some(sm_attempt_row(
                "att_verify",
                instance_id,
                step_name,
                ATTEMPT_STATUS_RUNNING,
                1,
                None,
                None,
            )))
        });
    sm.expect_patch_step_attempt()
        .times(1)
        .return_once(|attempt_id, patch| {
            Ok(sm_attempt_row(
                attempt_id,
                "ins_otp_002",
                STEP_PHONE_VERIFY_OTP,
                patch.status.as_deref().unwrap_or(ATTEMPT_STATUS_SUCCEEDED),
                1,
                None,
                None,
            ))
        });

    let mut sm_queue = MockStateMachineQueue::new();
    sm_queue.expect_enqueue().times(1).return_once(|_| Ok(()));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        sm_queue,
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );

    let issue_resp = api
        .internal_issue_phone_otp_challenge(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_001"),
            &gen_oas_server_bff::models::IssuePhoneOtpRequest::new(
                "ins_otp_002".to_owned(),
                "ins_otp_002__PHONE_OTP".to_owned(),
                "+491111111".to_owned(),
            ),
        )
        .await
        .expect("issue otp");
    assert!(matches!(
        issue_resp,
        gen_oas_server_bff::apis::phone_otp::InternalIssuePhoneOtpChallengeResponse::Status200_OTPChallenge(_)
    ));

    let verify_resp = api
        .internal_verify_phone_otp_challenge(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_001"),
            &gen_oas_server_bff::models::VerifyPhoneOtpRequest::new(
                "ins_otp_002".to_owned(),
                "ins_otp_002__PHONE_OTP".to_owned(),
                "otp_ref_001".to_owned(),
                "123456".to_owned(),
            ),
        )
        .await
        .expect("verify otp");
    assert!(matches!(
        verify_resp,
        gen_oas_server_bff::apis::phone_otp::InternalVerifyPhoneOtpChallengeResponse::Status200_VerificationOutcome(_)
    ));
}

#[tokio::test]
async fn bff_magic_email_issue_success() {
    let session = sm_instance_row(
        "ins_otp_003",
        KIND_KYC_PHONE_OTP,
        INSTANCE_STATUS_RUNNING,
        Some("usr_001"),
        json!({"step_ids": ["ins_otp_003__EMAIL_MAGIC"]}),
    );
    let created = sm_attempt_row(
        "att_magic",
        "ins_otp_003",
        "ISSUE_MAGIC_EMAIL",
        ATTEMPT_STATUS_SUCCEEDED,
        1,
        Some("magic_ref_001"),
        Some(json!({})),
    );

    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance()
        .times(1)
        .return_once(move |_| Ok(Some(session)));
    sm.expect_list_step_attempts()
        .times(1)
        .return_once(|_| Ok(vec![]));
    sm.expect_next_attempt_no()
        .times(1)
        .return_once(|_, _| Ok(1));
    sm.expect_create_step_attempt()
        .times(1)
        .return_once(move |_| Ok(created));

    let mut notifications = MockNotificationQueue::new();
    notifications
        .expect_enqueue()
        .times(1)
        .return_once(|_| Ok(()));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        notifications,
        MockMinioStorage::new(),
        None,
    );

    let response = api
        .internal_issue_magic_email_challenge(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_001"),
            &gen_oas_server_bff::models::IssueMagicEmailRequest::new(
                "ins_otp_003".to_owned(),
                "ins_otp_003__EMAIL_MAGIC".to_owned(),
                "alice@example.test".to_owned(),
            ),
        )
        .await
        .expect("issue magic");
    assert!(matches!(
        response,
        gen_oas_server_bff::apis::email_magic::InternalIssueMagicEmailChallengeResponse::Status200_MagicEmailChallenge(_)
    ));
}

#[tokio::test]
async fn bff_magic_email_verify_success() {
    let token_hash = hash_secret("very-secret").expect("hash");
    let attempt = sm_attempt_row(
        "att_magic",
        "ins_otp_004",
        "ISSUE_MAGIC_EMAIL",
        ATTEMPT_STATUS_SUCCEEDED,
        1,
        Some("magic_ref_002"),
        Some(json!({
            "token_hash": token_hash,
            "expires_at": (Utc::now() + Duration::minutes(15)).to_rfc3339()
        })),
    );
    let session = sm_instance_row(
        "ins_otp_004",
        KIND_KYC_PHONE_OTP,
        INSTANCE_STATUS_RUNNING,
        Some("usr_001"),
        json!({"step_ids": ["ins_otp_004__EMAIL_MAGIC"]}),
    );

    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance()
        .times(1)
        .return_once(move |_| Ok(Some(session)));
    sm.expect_get_step_attempt_by_external_ref()
        .times(1)
        .return_once(move |_, _, _| Ok(Some(attempt)));
    sm.expect_append_event()
        .times(1)
        .withf(|input| {
            input.instance_id == "ins_otp_004"
                && input.kind == "MAGIC_EMAIL_VERIFIED"
                && input.actor_type == "USER"
                && input.actor_id.as_deref() == Some("usr_001")
                && input.payload.get("step_id").and_then(Value::as_str)
                    == Some("ins_otp_004__EMAIL_MAGIC")
                && input.payload.get("token_ref").and_then(Value::as_str) == Some("magic_ref_002")
        })
        .return_once(|input| Ok(sm_event_row(&input.instance_id)));
    sm.expect_update_instance_status()
        .times(1)
        .withf(|instance_id, status, completed_at| {
            instance_id == "ins_otp_004"
                && status == INSTANCE_STATUS_COMPLETED
                && completed_at.is_some()
        })
        .return_once(|_, _, _| Ok(()));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );

    let response = api
        .internal_verify_magic_email_challenge(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_001"),
            &gen_oas_server_bff::models::VerifyMagicEmailRequest::new(
                "ins_otp_004".to_owned(),
                "ins_otp_004__EMAIL_MAGIC".to_owned(),
                "magic_ref_002.very-secret".to_owned(),
            ),
        )
        .await
        .expect("verify magic");
    assert!(matches!(
        response,
        gen_oas_server_bff::apis::email_magic::InternalVerifyMagicEmailChallengeResponse::Status200_VerificationOutcome(_)
    ));
}

#[tokio::test]
async fn bff_phone_deposit_create_and_get_success() {
    let context = json!({
        "step_ids": ["dep_001__PHONE_DEPOSIT"],
        "deposit": {
            "status": "CONTACT_PROVIDED",
            "amount": 1250.0,
            "currency": "XAF",
            "expires_at": (Utc::now() + Duration::hours(2)).to_rfc3339(),
            "contact": {
                "staff_id": "usr_staff",
                "full_name": "Staff One",
                "phone_number": "+490000"
            }
        }
    });
    let instance = sm_instance_row(
        "dep_001",
        KIND_KYC_FIRST_DEPOSIT,
        INSTANCE_STATUS_ACTIVE,
        Some("usr_001"),
        context,
    );

    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance()
        .times(2)
        .returning(move |_| Ok(Some(instance.clone())));
    sm.expect_get_latest_step_attempt()
        .times(1)
        .return_once(|_, _| {
            Ok(Some(sm_attempt_row(
                "att_wait",
                "dep_001",
                STEP_DEPOSIT_AWAIT_PAYMENT,
                ATTEMPT_STATUS_RUNNING,
                1,
                None,
                None,
            )))
        });
    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );

    let create_resp = api
        .internal_create_phone_deposit_request(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_001"),
            &gen_oas_server_bff::models::CreatePhoneDepositRequest::new(
                "dep_001".to_owned(),
                "usr_001".to_owned(),
                1250.0,
                "XAF".to_owned(),
            ),
        )
        .await
        .expect("create deposit");
    assert!(matches!(
        create_resp,
        gen_oas_server_bff::apis::deposits::InternalCreatePhoneDepositRequestResponse::Status201_DepositRequestCreated(_)
    ));

    let get_resp = api
        .internal_get_phone_deposit_request(
            &Method::GET,
            &host(),
            &cookies(),
            &claims("usr_001"),
            &gen_oas_server_bff::models::InternalGetPhoneDepositRequestPathParams {
                deposit_request_id: "dep_001".to_owned(),
            },
        )
        .await
        .expect("get deposit");
    assert!(matches!(
        get_resp,
        gen_oas_server_bff::apis::deposits::InternalGetPhoneDepositRequestResponse::Status200_DepositRequest(_)
    ));
}

#[tokio::test]
async fn bff_upload_presign_and_complete_success() {
    let session = sm_instance_row(
        "ins_otp_001",
        KIND_KYC_PHONE_OTP,
        INSTANCE_STATUS_RUNNING,
        Some("usr_001"),
        json!({"step_ids": ["ins_otp_001__PHONE_OTP"]}),
    );

    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance()
        .times(2)
        .returning(move |_| Ok(Some(session.clone())));

    let mut minio = MockMinioStorage::new();
    minio
        .expect_upload_presigned()
        .times(1)
        .return_once(|_, _, _, _, _| {
            Ok(PresignedUpload {
                url: "https://upload.example.test".to_owned(),
                headers: HashMap::from([(
                    "x-amz-server-side-encryption".to_owned(),
                    "AES256".to_owned(),
                )]),
            })
        });
    minio
        .expect_head_object()
        .times(1)
        .return_once(|_, _| Ok(()));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        minio,
        Some(s3_config()),
    );

    let presign_resp = api
        .internal_presign_upload(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_001"),
            &gen_oas_server_bff::models::InternalPresignRequest::new(
                "ins_otp_001".to_owned(),
                "ins_otp_001__PHONE_OTP".to_owned(),
                "usr_001".to_owned(),
                gen_oas_server_bff::models::InternalUploadPurpose::KycIdentity,
                gen_oas_server_bff::models::IdentityAssetType::IdFront,
                "image/png".to_owned(),
                1024,
            ),
        )
        .await
        .expect("presign upload");
    assert!(matches!(
        presign_resp,
        gen_oas_server_bff::apis::uploads::InternalPresignUploadResponse::Status200_PresignedUploadResponse(
            _
        )
    ));

    let complete_resp = api
        .internal_complete_upload(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_001"),
            &gen_oas_server_bff::models::InternalCompleteUploadRequest::new(
                "ins_otp_001".to_owned(),
                "ins_otp_001__PHONE_OTP".to_owned(),
                "upl_001".to_owned(),
                "kyc-bucket".to_owned(),
                "uploads/usr_001/upl_001".to_owned(),
            ),
        )
        .await
        .expect("complete upload");
    assert!(matches!(
        complete_resp,
        gen_oas_server_bff::apis::uploads::InternalCompleteUploadResponse::Status200_EvidenceRegistered(_)
    ));
}

#[tokio::test]
async fn staff_instances_get_success() {
    let instance = sm_instance_row(
        "ins_staff_001",
        KIND_KYC_PHONE_OTP,
        INSTANCE_STATUS_RUNNING,
        Some("usr_001"),
        json!({}),
    );
    let mut sm = MockStateMachineRepo::new();
    sm.expect_list_instances()
        .times(1)
        .return_once(|_| Ok((vec![instance], 1)));
    sm.expect_list_step_attempts()
        .times(1)
        .return_once(|_| Ok(vec![]));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );

    let response = api
        .staff_kyc_instances_get(
            &Method::GET,
            &host(),
            &cookies(),
            &claims("usr_staff"),
            &gen_oas_server_staff::models::StaffKycInstancesGetQueryParams {
                kind: None,
                status: None,
                user_id: None,
                phone_number: None,
                created_from: None,
                created_to: None,
                page: Some(1),
                limit: Some(20),
            },
        )
        .await
        .expect("staff list");
    assert!(matches!(
        response,
        gen_oas_server_staff::apis::kyc_state_machines::StaffKycInstancesGetResponse::Status200_PageOfInstances(_)
    ));
}

#[tokio::test]
async fn staff_instance_detail_success() {
    let instance = sm_instance_row(
        "ins_staff_002",
        KIND_KYC_PHONE_OTP,
        INSTANCE_STATUS_RUNNING,
        Some("usr_001"),
        json!({}),
    );
    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance()
        .times(1)
        .return_once(move |_| Ok(Some(instance)));
    sm.expect_list_step_attempts()
        .times(1)
        .return_once(|_| Ok(vec![]));
    sm.expect_list_events()
        .times(1)
        .return_once(|_| Ok(vec![sm_event_row("ins_staff_002")]));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );

    let response = api
        .staff_kyc_instances_instance_id_get(
            &Method::GET,
            &host(),
            &cookies(),
            &claims("usr_staff"),
            &gen_oas_server_staff::models::StaffKycInstancesInstanceIdGetPathParams {
                instance_id: "ins_staff_002".to_owned(),
            },
        )
        .await
        .expect("instance detail");
    assert!(matches!(
        response,
        gen_oas_server_staff::apis::kyc_state_machines::StaffKycInstancesInstanceIdGetResponse::Status200_InstanceDetail(_)
    ));
}

#[tokio::test]
async fn staff_retry_success() {
    let instance = sm_instance_row(
        "ins_dep_retry",
        KIND_KYC_FIRST_DEPOSIT,
        INSTANCE_STATUS_RUNNING,
        Some("usr_001"),
        json!({}),
    );
    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance()
        .times(1)
        .return_once(move |_| Ok(Some(instance)));
    sm.expect_next_attempt_no()
        .times(1)
        .return_once(|_, _| Ok(2));
    sm.expect_create_step_attempt()
        .times(1)
        .return_once(|input| {
            Ok(sm_attempt_row(
                &input.id,
                &input.instance_id,
                &input.step_name,
                &input.status,
                input.attempt_no,
                None,
                None,
            ))
        });
    sm.expect_cancel_other_attempts_for_step()
        .times(1)
        .return_once(|_, _, _| Ok(()));

    let mut sm_queue = MockStateMachineQueue::new();
    sm_queue.expect_enqueue().times(1).return_once(|_| Ok(()));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        sm_queue,
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );

    let response = api
        .staff_kyc_instances_instance_id_retry_post(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_staff"),
            &gen_oas_server_staff::models::StaffKycInstancesInstanceIdRetryPostPathParams {
                instance_id: "ins_dep_retry".to_owned(),
            },
            &gen_oas_server_staff::models::RetryRequest::new(
                STEP_DEPOSIT_REGISTER_CUSTOMER.to_owned(),
                gen_oas_server_staff::models::RetryMode::NewAttempt,
            ),
        )
        .await
        .expect("retry response");
    assert!(matches!(
        response,
        gen_oas_server_staff::apis::kyc_state_machines::StaffKycInstancesInstanceIdRetryPostResponse::Status200_RetryAccepted(_)
    ));
}

#[tokio::test]
async fn staff_confirm_payment_success() {
    let instance = sm_instance_row(
        "ins_dep_confirm",
        KIND_KYC_FIRST_DEPOSIT,
        INSTANCE_STATUS_RUNNING,
        Some("usr_001"),
        json!({}),
    );
    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance()
        .times(1)
        .return_once(move |_| Ok(Some(instance)));
    sm.expect_update_instance_context()
        .times(1)
        .return_once(|_, _| Ok(()));
    sm.expect_append_event()
        .times(1)
        .return_once(|input| Ok(sm_event_row(&input.instance_id)));
    sm.expect_get_latest_step_attempt()
        .times(2)
        .returning(|instance_id, step_name| {
            if step_name == STEP_DEPOSIT_AWAIT_PAYMENT {
                Ok(Some(sm_attempt_row(
                    "att_payment",
                    instance_id,
                    step_name,
                    ATTEMPT_STATUS_RUNNING,
                    1,
                    None,
                    None,
                )))
            } else {
                Ok(None)
            }
        });
    sm.expect_patch_step_attempt()
        .times(1)
        .return_once(|attempt_id, _| {
            Ok(sm_attempt_row(
                attempt_id,
                "ins_dep_confirm",
                STEP_DEPOSIT_AWAIT_PAYMENT,
                ATTEMPT_STATUS_SUCCEEDED,
                1,
                None,
                None,
            ))
        });
    sm.expect_create_step_attempt()
        .times(1)
        .return_once(|input| {
            Ok(sm_attempt_row(
                &input.id,
                &input.instance_id,
                &input.step_name,
                &input.status,
                input.attempt_no,
                None,
                None,
            ))
        });
    sm.expect_update_instance_status()
        .times(1)
        .return_once(|_, _, _| Ok(()));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );

    let response = api
        .staff_kyc_deposits_instance_id_confirm_payment_post(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_staff"),
            &gen_oas_server_staff::models::StaffKycDepositsInstanceIdConfirmPaymentPostPathParams {
                instance_id: "ins_dep_confirm".to_owned(),
            },
            &gen_oas_server_staff::models::ConfirmPaymentRequest::new(),
        )
        .await
        .expect("confirm payment");
    assert!(matches!(
        response,
        gen_oas_server_staff::apis::kyc_state_machines::StaffKycDepositsInstanceIdConfirmPaymentPostResponse::Status200_PaymentConfirmationRecorded
    ));
}

#[tokio::test]
async fn staff_approve_success() {
    let instance = sm_instance_row(
        "ins_dep_approve",
        KIND_KYC_FIRST_DEPOSIT,
        INSTANCE_STATUS_RUNNING,
        Some("usr_001"),
        json!({}),
    );
    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance()
        .times(1)
        .return_once(move |_| Ok(Some(instance)));
    sm.expect_update_instance_context()
        .times(1)
        .return_once(|_, _| Ok(()));
    sm.expect_append_event()
        .times(1)
        .return_once(|input| Ok(sm_event_row(&input.instance_id)));
    sm.expect_get_latest_step_attempt()
        .times(2)
        .returning(|instance_id, step_name| {
            if step_name == STEP_DEPOSIT_REGISTER_CUSTOMER {
                Ok(None)
            } else if step_name == STEP_DEPOSIT_AWAIT_APPROVAL {
                Ok(Some(sm_attempt_row(
                    "att_approval",
                    instance_id,
                    step_name,
                    ATTEMPT_STATUS_RUNNING,
                    1,
                    None,
                    None,
                )))
            } else {
                Ok(None)
            }
        });
    sm.expect_patch_step_attempt()
        .times(1)
        .return_once(|attempt_id, _| {
            Ok(sm_attempt_row(
                attempt_id,
                "ins_dep_approve",
                STEP_DEPOSIT_AWAIT_APPROVAL,
                ATTEMPT_STATUS_SUCCEEDED,
                1,
                None,
                None,
            ))
        });
    sm.expect_next_attempt_no()
        .times(1)
        .return_once(|_, _| Ok(1));
    sm.expect_create_step_attempt()
        .times(1)
        .return_once(|input| {
            Ok(sm_attempt_row(
                &input.id,
                &input.instance_id,
                &input.step_name,
                &input.status,
                input.attempt_no,
                None,
                None,
            ))
        });
    sm.expect_update_instance_status()
        .times(1)
        .return_once(|_, _, _| Ok(()));

    let mut sm_queue = MockStateMachineQueue::new();
    sm_queue.expect_enqueue().times(1).return_once(|_| Ok(()));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        sm_queue,
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );

    let approve_body =
        gen_oas_server_staff::models::DepositApproveRequest::new("Alice Tester".to_owned(), 2000.0);
    let response = api
        .staff_kyc_deposits_instance_id_approve_post(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_staff"),
            &gen_oas_server_staff::models::StaffKycDepositsInstanceIdApprovePostPathParams {
                instance_id: "ins_dep_approve".to_owned(),
            },
            &approve_body,
        )
        .await
        .expect("approve response");
    assert!(matches!(
        response,
        gen_oas_server_staff::apis::kyc_state_machines::StaffKycDepositsInstanceIdApprovePostResponse::Status200_ApprovalRecorded
    ));
}

#[tokio::test]
async fn staff_reports_summary_success() {
    let rows = vec![
        sm_instance_row(
            "ins_sum_1",
            KIND_KYC_PHONE_OTP,
            INSTANCE_STATUS_RUNNING,
            Some("usr_001"),
            json!({}),
        ),
        sm_instance_row(
            "ins_sum_2",
            KIND_KYC_FIRST_DEPOSIT,
            INSTANCE_STATUS_COMPLETED,
            Some("usr_002"),
            json!({}),
        ),
    ];
    let mut sm = MockStateMachineRepo::new();
    sm.expect_list_instances()
        .times(1)
        .return_once(move |_| Ok((rows, 2)));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );

    let response = api
        .staff_kyc_reports_summary_get(&Method::GET, &host(), &cookies(), &claims("usr_staff"))
        .await
        .expect("summary response");
    assert!(matches!(
        response,
        gen_oas_server_staff::apis::kyc_state_machines::StaffKycReportsSummaryGetResponse::Status200_SummaryReport(_)
    ));
}

#[tokio::test]
async fn bff_start_session_user_mismatch_is_unauthorized() {
    let api = build_api(
        MockStateMachineRepo::new(),
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );
    let error = api
        .internal_create_kyc_session(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_auth"),
            &gen_oas_server_bff::models::CreateKycSessionRequest::new(
                "usr_other".to_owned(),
                gen_oas_server_bff::models::KycFlowType::PhoneOtp,
            ),
        )
        .await
        .expect_err("expected unauthorized");
    assert_http_error(error, 401, "UNAUTHORIZED");
}

#[tokio::test]
async fn bff_create_step_session_not_found() {
    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance().times(1).return_once(|_| Ok(None));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );
    let error = api
        .internal_create_phone_otp_step(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_001"),
            &gen_oas_server_bff::models::CreateCaseStepRequest::new(
                "ins_missing".to_owned(),
                "usr_001".to_owned(),
            ),
        )
        .await
        .expect_err("expected session not found");
    assert_http_error(error, 404, "SESSION_NOT_FOUND");
}

#[tokio::test]
async fn bff_create_email_step_success() {
    let session = sm_instance_row(
        "ins_otp_005",
        KIND_KYC_PHONE_OTP,
        INSTANCE_STATUS_RUNNING,
        Some("usr_001"),
        json!({}),
    );
    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance()
        .times(1)
        .return_once(move |_| Ok(Some(session)));
    sm.expect_update_instance_context()
        .times(1)
        .return_once(|_, _| Ok(()));
    sm.expect_list_step_attempts()
        .times(1)
        .return_once(|_| Ok(vec![]));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );
    let response = api
        .internal_create_email_magic_step(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_001"),
            &gen_oas_server_bff::models::CreateCaseStepRequest::new(
                "ins_otp_005".to_owned(),
                "usr_001".to_owned(),
            ),
        )
        .await
        .expect("expected email step to be created");
    assert!(matches!(
        response,
        gen_oas_server_bff::apis::email_magic::InternalCreateEmailMagicStepResponse::Status201_StepCreated(_)
    ));
}

#[tokio::test]
async fn bff_get_step_invalid_step_id() {
    let api = build_api(
        MockStateMachineRepo::new(),
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );
    let error = api
        .internal_get_kyc_step(
            &Method::GET,
            &host(),
            &cookies(),
            &claims("usr_001"),
            &gen_oas_server_bff::models::InternalGetKycStepPathParams {
                step_id: "bad-format".to_owned(),
            },
        )
        .await
        .expect_err("expected invalid step id");
    assert_http_error(error, 400, "INVALID_STEP_ID");
}

#[tokio::test]
async fn bff_issue_otp_rate_limited() {
    let session = sm_instance_row(
        "ins_otp_006",
        KIND_KYC_PHONE_OTP,
        INSTANCE_STATUS_RUNNING,
        Some("usr_001"),
        json!({"step_ids": ["ins_otp_006__PHONE_OTP"]}),
    );
    let recent_attempt = sm_attempt_row(
        "att_rate",
        "ins_otp_006",
        STEP_PHONE_ISSUE_OTP,
        ATTEMPT_STATUS_SUCCEEDED,
        1,
        Some("otp_ref"),
        None,
    );

    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance()
        .times(1)
        .return_once(move |_| Ok(Some(session)));
    sm.expect_list_step_attempts()
        .times(1)
        .return_once(move |_| Ok(vec![recent_attempt; 5]));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );

    let error = api
        .internal_issue_phone_otp_challenge(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_001"),
            &gen_oas_server_bff::models::IssuePhoneOtpRequest::new(
                "ins_otp_006".to_owned(),
                "ins_otp_006__PHONE_OTP".to_owned(),
                "+49111111".to_owned(),
            ),
        )
        .await
        .expect_err("expected rate limit");
    assert_http_error(error, 429, "OTP_RATE_LIMITED");
}

#[tokio::test]
async fn bff_verify_otp_missing_attempt_returns_invalid_outcome() {
    let session = sm_instance_row(
        "ins_otp_007",
        KIND_KYC_PHONE_OTP,
        INSTANCE_STATUS_RUNNING,
        Some("usr_001"),
        json!({"step_ids": ["ins_otp_007__PHONE_OTP"]}),
    );
    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance()
        .times(1)
        .return_once(move |_| Ok(Some(session)));
    sm.expect_get_step_attempt_by_external_ref()
        .times(1)
        .return_once(|_, _, _| Ok(None));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );
    let response = api
        .internal_verify_phone_otp_challenge(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_001"),
            &gen_oas_server_bff::models::VerifyPhoneOtpRequest::new(
                "ins_otp_007".to_owned(),
                "ins_otp_007__PHONE_OTP".to_owned(),
                "missing_ref".to_owned(),
                "000000".to_owned(),
            ),
        )
        .await
        .expect("verify otp response");
    assert!(matches!(
        response,
        gen_oas_server_bff::apis::phone_otp::InternalVerifyPhoneOtpChallengeResponse::Status200_VerificationOutcome(_)
    ));
}

#[tokio::test]
async fn bff_magic_verify_invalid_token_returns_failed_outcome() {
    let session = sm_instance_row(
        "ins_otp_008",
        KIND_KYC_PHONE_OTP,
        INSTANCE_STATUS_RUNNING,
        Some("usr_001"),
        json!({"step_ids": ["ins_otp_008__EMAIL_MAGIC"]}),
    );
    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance()
        .times(1)
        .return_once(move |_| Ok(Some(session)));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );
    let response = api
        .internal_verify_magic_email_challenge(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_001"),
            &gen_oas_server_bff::models::VerifyMagicEmailRequest::new(
                "ins_otp_008".to_owned(),
                "ins_otp_008__EMAIL_MAGIC".to_owned(),
                "invalid-token-without-dot".to_owned(),
            ),
        )
        .await
        .expect("verify magic response");
    assert!(matches!(
        response,
        gen_oas_server_bff::apis::email_magic::InternalVerifyMagicEmailChallengeResponse::Status200_VerificationOutcome(_)
    ));
}

#[tokio::test]
async fn bff_get_phone_deposit_not_found() {
    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance().times(1).return_once(|_| Ok(None));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );
    let error = api
        .internal_get_phone_deposit_request(
            &Method::GET,
            &host(),
            &cookies(),
            &claims("usr_001"),
            &gen_oas_server_bff::models::InternalGetPhoneDepositRequestPathParams {
                deposit_request_id: "dep_missing".to_owned(),
            },
        )
        .await
        .expect_err("expected not found");
    assert_http_error(error, 404, "DEPOSIT_NOT_FOUND");
}

#[tokio::test]
async fn bff_presign_upload_user_mismatch() {
    let api = build_api(
        MockStateMachineRepo::new(),
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        Some(s3_config()),
    );
    let error = api
        .internal_presign_upload(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_auth"),
            &gen_oas_server_bff::models::InternalPresignRequest::new(
                "ins__PHONE".to_owned(),
                "ins__PHONE__PHONE_OTP".to_owned(),
                "usr_other".to_owned(),
                gen_oas_server_bff::models::InternalUploadPurpose::KycIdentity,
                gen_oas_server_bff::models::IdentityAssetType::IdBack,
                "image/jpeg".to_owned(),
                200,
            ),
        )
        .await
        .expect_err("expected unauthorized");
    assert_http_error(error, 401, "UNAUTHORIZED");
}

#[tokio::test]
async fn bff_presign_upload_without_storage_config_fails() {
    let session = sm_instance_row(
        "ins_otp_009",
        KIND_KYC_PHONE_OTP,
        INSTANCE_STATUS_RUNNING,
        Some("usr_001"),
        json!({"step_ids": ["ins_otp_009__PHONE_OTP"]}),
    );
    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance()
        .times(1)
        .return_once(move |_| Ok(Some(session)));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        Some(base_config()),
    );
    let error = api
        .internal_presign_upload(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_001"),
            &gen_oas_server_bff::models::InternalPresignRequest::new(
                "ins_otp_009".to_owned(),
                "ins_otp_009__PHONE_OTP".to_owned(),
                "usr_001".to_owned(),
                gen_oas_server_bff::models::InternalUploadPurpose::KycIdentity,
                gen_oas_server_bff::models::IdentityAssetType::SelfieCloseup,
                "image/jpeg".to_owned(),
                200,
            ),
        )
        .await
        .expect_err("expected storage config error");
    assert_http_error(error, 500, "S3_NOT_CONFIGURED");
}

#[tokio::test]
async fn staff_instance_detail_not_found() {
    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance().times(1).return_once(|_| Ok(None));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );
    let response = api
        .staff_kyc_instances_instance_id_get(
            &Method::GET,
            &host(),
            &cookies(),
            &claims("usr_staff"),
            &gen_oas_server_staff::models::StaffKycInstancesInstanceIdGetPathParams {
                instance_id: "missing".to_owned(),
            },
        )
        .await
        .expect("detail response");
    assert!(matches!(
        response,
        gen_oas_server_staff::apis::kyc_state_machines::StaffKycInstancesInstanceIdGetResponse::Status404_InstanceNotFound
    ));
}

#[tokio::test]
async fn staff_retry_instance_not_found() {
    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance().times(1).return_once(|_| Ok(None));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );
    let response = api
        .staff_kyc_instances_instance_id_retry_post(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_staff"),
            &gen_oas_server_staff::models::StaffKycInstancesInstanceIdRetryPostPathParams {
                instance_id: "missing".to_owned(),
            },
            &gen_oas_server_staff::models::RetryRequest::new(
                STEP_PHONE_ISSUE_OTP.to_owned(),
                gen_oas_server_staff::models::RetryMode::NewAttempt,
            ),
        )
        .await
        .expect("retry response");
    assert!(matches!(
        response,
        gen_oas_server_staff::apis::kyc_state_machines::StaffKycInstancesInstanceIdRetryPostResponse::Status404_InstanceNotFound
    ));
}

#[tokio::test]
async fn staff_retry_rejects_invalid_step() {
    let instance = sm_instance_row(
        "ins_retry_invalid",
        KIND_KYC_PHONE_OTP,
        INSTANCE_STATUS_RUNNING,
        Some("usr_001"),
        json!({}),
    );
    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance()
        .times(1)
        .return_once(move |_| Ok(Some(instance)));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );
    let error = api
        .staff_kyc_instances_instance_id_retry_post(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_staff"),
            &gen_oas_server_staff::models::StaffKycInstancesInstanceIdRetryPostPathParams {
                instance_id: "ins_retry_invalid".to_owned(),
            },
            &gen_oas_server_staff::models::RetryRequest::new(
                "NOT_A_STEP".to_owned(),
                gen_oas_server_staff::models::RetryMode::NewAttempt,
            ),
        )
        .await
        .expect_err("expected invalid step");
    assert_http_error(error, 400, "INVALID_STEP");
}

#[tokio::test]
async fn staff_retry_rejects_non_retryable_step() {
    let instance = sm_instance_row(
        "ins_retry_non",
        KIND_KYC_FIRST_DEPOSIT,
        INSTANCE_STATUS_RUNNING,
        Some("usr_001"),
        json!({}),
    );
    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance()
        .times(1)
        .return_once(move |_| Ok(Some(instance)));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );
    let error = api
        .staff_kyc_instances_instance_id_retry_post(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_staff"),
            &gen_oas_server_staff::models::StaffKycInstancesInstanceIdRetryPostPathParams {
                instance_id: "ins_retry_non".to_owned(),
            },
            &gen_oas_server_staff::models::RetryRequest::new(
                STEP_DEPOSIT_AWAIT_APPROVAL.to_owned(),
                gen_oas_server_staff::models::RetryMode::ResetAttempt,
            ),
        )
        .await
        .expect_err("expected non retryable step");
    assert_http_error(error, 400, "INVALID_RETRY_STEP");
}

#[tokio::test]
async fn staff_confirm_rejects_non_deposit_instance() {
    let instance = sm_instance_row(
        "ins_not_dep",
        KIND_KYC_PHONE_OTP,
        INSTANCE_STATUS_RUNNING,
        Some("usr_001"),
        json!({}),
    );
    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance()
        .times(1)
        .return_once(move |_| Ok(Some(instance)));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );
    let error = api
        .staff_kyc_deposits_instance_id_confirm_payment_post(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_staff"),
            &gen_oas_server_staff::models::StaffKycDepositsInstanceIdConfirmPaymentPostPathParams {
                instance_id: "ins_not_dep".to_owned(),
            },
            &gen_oas_server_staff::models::ConfirmPaymentRequest::new(),
        )
        .await
        .expect_err("expected invalid kind");
    assert_http_error(error, 400, "INVALID_INSTANCE_KIND");
}

#[tokio::test]
async fn staff_approve_rejects_non_deposit_instance() {
    let instance = sm_instance_row(
        "ins_not_dep_2",
        KIND_KYC_PHONE_OTP,
        INSTANCE_STATUS_RUNNING,
        Some("usr_001"),
        json!({}),
    );
    let mut sm = MockStateMachineRepo::new();
    sm.expect_get_instance()
        .times(1)
        .return_once(move |_| Ok(Some(instance)));

    let api = build_api(
        sm,
        MockUserRepo::new(),
        MockDeviceRepo::new(),
        MockStateMachineQueue::new(),
        MockNotificationQueue::new(),
        MockMinioStorage::new(),
        None,
    );
    let body = gen_oas_server_staff::models::DepositApproveRequest::new("A".to_owned(), 10.0);
    let error = api
        .staff_kyc_deposits_instance_id_approve_post(
            &Method::POST,
            &host(),
            &cookies(),
            &claims("usr_staff"),
            &gen_oas_server_staff::models::StaffKycDepositsInstanceIdApprovePostPathParams {
                instance_id: "ins_not_dep_2".to_owned(),
            },
            &body,
        )
        .await
        .expect_err("expected invalid kind");
    assert_http_error(error, 400, "INVALID_INSTANCE_KIND");
}

#[tokio::test]
async fn cuss_oas_client_register_success_and_error() {
    let app = axum::Router::new()
        .route(
            "/api/registration/register",
            post(|| async {
                axum::Json(json!({
                    "success": true,
                    "status": "OK",
                    "fineractClientId": 77,
                    "savingsAccountId": 88
                }))
            }),
        )
        .route(
            "/api/registration/approve-and-deposit",
            post(|| async {
                (
                    axum::http::StatusCode::BAD_REQUEST,
                    axum::Json(json!({"error":"bad request"})),
                )
            }),
        );

    let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            // Sandboxed environments may block loopback binding.
            return;
        }
        Err(error) => panic!("bind listener: {error}"),
    };
    let addr = listener.local_addr().expect("local addr");
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app.into_make_service()).await;
    });

    let mut config = gen_oas_client_cuss::apis::configuration::Configuration::new();
    config.base_path = format!("http://{addr}");

    let register_resp = gen_oas_client_cuss::apis::registration_api::register_customer(
        &config,
        gen_oas_client_cuss::models::RegistrationRequest {
            full_name: "Alice Tester".to_owned(),
            email: None,
            phone: "+49111111".to_owned(),
            external_id: "usr_001".to_owned(),
            date_of_birth: None,
        },
    )
    .await
    .expect("register customer");
    assert_eq!(register_resp.savings_account_id, Some(88));

    let err = gen_oas_client_cuss::apis::registration_api::approve_and_deposit(
        &config,
        gen_oas_client_cuss::models::ApproveAndDepositRequest {
            savings_account_id: 88,
            deposit_amount: Some(1000.0),
        },
    )
    .await
    .expect_err("expected response error");

    match err {
        gen_oas_client_cuss::apis::Error::ResponseError(response) => {
            assert_eq!(response.status, reqwest::StatusCode::BAD_REQUEST);
        }
        other => panic!("unexpected error variant: {other:?}"),
    }

    server.abort();
}
