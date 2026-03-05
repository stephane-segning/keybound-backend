mod common;

use anyhow::{Result, anyhow};
use argon2::password_hash::{PasswordHasher, SaltString};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use common::{
    Env, JsonResponse, create_foreign_deposit_fixture, ensure_bff_fixtures,
    get_client_token_and_subject, http_client, require_json_field, reset_sms_sink, send_json,
    wait_for_otp,
};
use hmac::{Hmac, Mac};
use reqwest::Method;
use serde_json::{Value, json};
use serial_test::file_serial;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tokio_postgres::NoTls;

fn test_context() -> Result<(Env, reqwest::Client)> {
    let env = Env::from_env()?;
    let client = http_client()?;
    Ok((env, client))
}

#[tokio::test]
#[file_serial]
async fn full_01_auth_enforcement() -> Result<()> {
    let (env, client) = test_context()?;
    scenario_auth_enforcement(&client, &env).await?;
    Ok(())
}

#[tokio::test]
#[file_serial]
async fn full_02_auth_bypass_outside_protected_paths() -> Result<()> {
    let (env, client) = test_context()?;
    scenario_auth_bypass_outside_protected_paths(&client, &env).await?;
    Ok(())
}

#[tokio::test]
#[file_serial]
async fn full_03_bff_deposit_and_otp_flow() -> Result<()> {
    let (env, client) = test_context()?;
    scenario_bff_deposit_and_otp_flow(&client, &env).await?;
    Ok(())
}

#[tokio::test]
#[file_serial]
async fn full_04_bff_deposit_expiry_behavior() -> Result<()> {
    let (env, client) = test_context()?;
    scenario_bff_deposit_expiry_behavior(&client, &env).await?;
    Ok(())
}

#[tokio::test]
#[file_serial]
async fn full_05_bff_session_resume_and_otp_limits() -> Result<()> {
    let (env, client) = test_context()?;
    scenario_bff_session_resume_and_otp_limits(&client, &env).await?;
    Ok(())
}

#[tokio::test]
#[file_serial]
async fn full_06_bff_email_magic_and_uploads() -> Result<()> {
    let (env, client) = test_context()?;
    scenario_bff_email_magic_and_uploads(&client, &env).await?;
    Ok(())
}

#[tokio::test]
#[file_serial]
async fn full_07_bff_deposit_denies_non_owner() -> Result<()> {
    let (env, client) = test_context()?;
    scenario_bff_deposit_denies_non_owner(&client, &env).await?;
    Ok(())
}

#[tokio::test]
#[file_serial]
async fn full_08_kc_signature_and_surface() -> Result<()> {
    let (env, client) = test_context()?;
    scenario_kc_signature_and_surface(&client, &env).await?;
    Ok(())
}

#[tokio::test]
#[file_serial]
async fn full_09_staff_instance_detail_and_retry() -> Result<()> {
    let (env, client) = test_context()?;
    scenario_staff_instance_detail_and_retry(&client, &env).await?;
    Ok(())
}

#[tokio::test]
#[file_serial]
async fn full_10_staff_summary_and_instances() -> Result<()> {
    let (env, client) = test_context()?;
    scenario_staff_summary_and_instances(&client, &env).await?;
    Ok(())
}

#[tokio::test]
#[file_serial]
async fn full_11_staff_deposit_flow_triggers_worker_and_cuss() -> Result<()> {
    let (env, client) = test_context()?;
    scenario_staff_deposit_flow_triggers_worker_and_cuss(&client, &env).await?;
    Ok(())
}

#[tokio::test]
#[file_serial]
async fn full_12_staff_deposit_approve_idempotency() -> Result<()> {
    let (env, client) = test_context()?;
    scenario_staff_deposit_approve_idempotency(&client, &env).await?;
    Ok(())
}

#[tokio::test]
#[file_serial]
async fn full_13_worker_cuss_failures_and_manual_retries() -> Result<()> {
    let (env, client) = test_context()?;
    scenario_worker_cuss_failures_and_manual_retries(&client, &env).await?;
    Ok(())
}

#[tokio::test]
#[file_serial]
async fn full_14_error_mapping_representative() -> Result<()> {
    let (env, client) = test_context()?;
    scenario_error_mapping_representative(&client, &env).await?;
    Ok(())
}

#[tokio::test]
#[file_serial]
async fn full_15_auth_blank_base_path_does_not_protect_unrelated_routes() -> Result<()> {
    let (env, client) = test_context()?;
    scenario_auth_blank_base_path_bypass(&client, &env).await?;
    Ok(())
}

#[tokio::test]
#[file_serial]
async fn full_16_auth_disabled_bypasses_bearer_layer() -> Result<()> {
    let (env, client) = test_context()?;
    scenario_auth_disabled_bypass(&client, &env).await?;
    Ok(())
}

#[tokio::test]
#[file_serial]
async fn full_17_worker_single_consumer_lock_enforced() -> Result<()> {
    let (env, client) = test_context()?;
    scenario_worker_single_consumer_lock_enforced(&client, &env).await?;
    Ok(())
}

#[tokio::test]
#[file_serial]
async fn full_18_sms_transient_error_retries_until_success() -> Result<()> {
    let (env, client) = test_context()?;
    scenario_sms_transient_error_retries_until_success(&client, &env).await?;
    Ok(())
}

#[tokio::test]
#[file_serial]
async fn full_19_sms_permanent_error_terminal_no_infinite_retries() -> Result<()> {
    let (env, client) = test_context()?;
    scenario_sms_permanent_error_terminal_no_infinite_retries(&client, &env).await?;
    Ok(())
}

async fn scenario_auth_bypass_outside_protected_paths(
    client: &reqwest::Client,
    env: &Env,
) -> Result<()> {
    let health_no_auth = client
        .get(format!("{}/health", env.user_storage_url))
        .send()
        .await?;
    assert_eq!(health_no_auth.status().as_u16(), 200);

    let health_with_invalid_bearer = client
        .get(format!("{}/health", env.user_storage_url))
        .header("Authorization", "Bearer definitely-invalid-token")
        .send()
        .await?;
    assert_eq!(health_with_invalid_bearer.status().as_u16(), 200);

    let missing_with_invalid_bearer = client
        .get(format!("{}/does-not-exist-e2e", env.user_storage_url))
        .header("Authorization", "Bearer definitely-invalid-token")
        .send()
        .await?;
    assert_eq!(missing_with_invalid_bearer.status().as_u16(), 404);

    Ok(())
}

async fn scenario_auth_blank_base_path_bypass(client: &reqwest::Client, env: &Env) -> Result<()> {
    let base_url = env.user_storage_blank_base_url.as_deref().ok_or_else(|| {
        anyhow!("USER_STORAGE_BLANK_BASE_URL is required for blank base path test")
    })?;

    let health_no_auth = client.get(format!("{base_url}/health")).send().await?;
    assert_eq!(health_no_auth.status().as_u16(), 200);

    let health_with_invalid_bearer = client
        .get(format!("{base_url}/health"))
        .header("Authorization", "Bearer definitely-invalid-token")
        .send()
        .await?;
    assert_eq!(health_with_invalid_bearer.status().as_u16(), 200);

    let staff_no_auth = client
        .get(format!("{base_url}/staff/api/kyc/instances"))
        .send()
        .await?;
    assert_eq!(staff_no_auth.status().as_u16(), 401);

    Ok(())
}

async fn scenario_auth_disabled_bypass(client: &reqwest::Client, env: &Env) -> Result<()> {
    let base_url = env
        .user_storage_auth_disabled_url
        .as_deref()
        .ok_or_else(|| {
            anyhow!("USER_STORAGE_AUTH_DISABLED_URL is required for auth-disabled test")
        })?;

    let user_id = "usr_auth_disabled";
    ensure_bff_fixtures(&env.database_url, user_id).await?;

    let bff_no_auth = send_json(
        client,
        Method::POST,
        &format!("{}/bff/internal/deposits/phone", base_url),
        None,
        Some(json!({
            "userId": user_id,
            "amount": 1000,
            "currency": "XAF",
            "provider": "MTN_CM",
            "reason": "auth-disabled-bypass"
        })),
    )
    .await?;
    assert_ne!(bff_no_auth.status, 401, "{}", bff_no_auth.text);

    let staff_no_auth = send_json(
        client,
        Method::GET,
        &format!("{}/staff/api/kyc/instances?page=1&limit=1", base_url),
        None,
        None,
    )
    .await?;
    assert_ne!(staff_no_auth.status, 401, "{}", staff_no_auth.text);

    Ok(())
}

async fn scenario_worker_single_consumer_lock_enforced(
    client: &reqwest::Client,
    env: &Env,
) -> Result<()> {
    let primary = env
        .worker_primary_url
        .as_deref()
        .ok_or_else(|| anyhow!("WORKER_PRIMARY_URL is required for worker lock test"))?;
    let secondary = env
        .worker_secondary_url
        .as_deref()
        .ok_or_else(|| anyhow!("WORKER_SECONDARY_URL is required for worker lock test"))?;

    let deadline = Instant::now() + Duration::from_secs(20);
    let (mut primary_ok, mut secondary_ok) = (false, false);

    while Instant::now() < deadline {
        primary_ok = worker_health_ok(client, primary).await;
        secondary_ok = worker_health_ok(client, secondary).await;
        if primary_ok ^ secondary_ok {
            break;
        }
        sleep(Duration::from_millis(500)).await;
    }

    assert!(
        primary_ok ^ secondary_ok,
        "expected exactly one worker health endpoint to be available with single-consumer lock; primary_ok={primary_ok}, secondary_ok={secondary_ok}"
    );

    Ok(())
}

async fn scenario_sms_transient_error_retries_until_success(
    client: &reqwest::Client,
    env: &Env,
) -> Result<()> {
    let (token, subject) = get_client_token_and_subject(client, env).await?;
    ensure_bff_fixtures(&env.database_url, &subject).await?;
    reset_sms_sink(client, env).await?;

    let sms_fault = send_json(
        client,
        Method::POST,
        &format!("{}/__admin/faults", env.sms_sink_url),
        None,
        Some(json!({
            "status": 503,
            "body": { "error": "transient upstream" },
            "count": 1
        })),
    )
    .await?;
    assert_eq!(sms_fault.status, 200, "{}", sms_fault.text);

    let bff_base = format!("{}/bff", env.user_storage_url);
    let staff_base = format!("{}/staff", env.user_storage_url);
    let msisdn = "+237690000099";
    let (session_id, _step_id, _otp_ref) =
        create_phone_step_and_issue_otp(client, &bff_base, &token, &subject, msisdn, 120).await?;

    wait_for_step_status(
        client,
        &staff_base,
        &token,
        &session_id,
        "ISSUE_OTP",
        "SUCCEEDED",
        Duration::from_secs(45),
    )
    .await?;

    let detail = send_json(
        client,
        Method::GET,
        &format!("{}/api/kyc/instances/{}", staff_base, session_id),
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(detail.status, 200, "{}", detail.text);
    assert!(
        step_attempts_count(&detail.body, "ISSUE_OTP") >= 2,
        "transient SMS failure should create at least one retry attempt"
    );

    let _otp = wait_for_otp(client, env, msisdn, Duration::from_secs(30)).await?;

    Ok(())
}

async fn scenario_sms_permanent_error_terminal_no_infinite_retries(
    client: &reqwest::Client,
    env: &Env,
) -> Result<()> {
    let (token, subject) = get_client_token_and_subject(client, env).await?;
    ensure_bff_fixtures(&env.database_url, &subject).await?;
    reset_sms_sink(client, env).await?;

    let sms_fault = send_json(
        client,
        Method::POST,
        &format!("{}/__admin/faults", env.sms_sink_url),
        None,
        Some(json!({
            "status": 400,
            "body": { "error": "invalid msisdn" },
            "count": 5
        })),
    )
    .await?;
    assert_eq!(sms_fault.status, 200, "{}", sms_fault.text);

    let bff_base = format!("{}/bff", env.user_storage_url);
    let staff_base = format!("{}/staff", env.user_storage_url);
    let (session_id, _step_id, _otp_ref) =
        create_phone_step_and_issue_otp(client, &bff_base, &token, &subject, "+237690000100", 120)
            .await?;

    wait_for_step_status(
        client,
        &staff_base,
        &token,
        &session_id,
        "ISSUE_OTP",
        "FAILED",
        Duration::from_secs(30),
    )
    .await?;

    sleep(Duration::from_secs(4)).await;

    let detail = send_json(
        client,
        Method::GET,
        &format!("{}/api/kyc/instances/{}", staff_base, session_id),
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(detail.status, 200, "{}", detail.text);
    assert_eq!(
        step_attempts_count(&detail.body, "ISSUE_OTP"),
        1,
        "permanent SMS failures should not create retry attempts"
    );

    let sms_messages = send_json(
        client,
        Method::GET,
        &format!("{}/__admin/messages", env.sms_sink_url),
        None,
        None,
    )
    .await?;
    assert_eq!(sms_messages.status, 200, "{}", sms_messages.text);
    let delivered = sms_messages
        .body
        .as_ref()
        .and_then(Value::as_array)
        .map(|items| items.len())
        .unwrap_or(0);
    assert_eq!(delivered, 0, "permanent SMS failure should not deliver OTP");

    Ok(())
}

async fn worker_health_ok(client: &reqwest::Client, base_url: &str) -> bool {
    match client.get(format!("{base_url}/health")).send().await {
        Ok(response) => response.status().as_u16() == 200,
        Err(_) => false,
    }
}

async fn scenario_auth_enforcement(client: &reqwest::Client, env: &Env) -> Result<()> {
    let bff_base = format!("{}/bff", env.user_storage_url);
    let staff_base = format!("{}/staff", env.user_storage_url);

    let no_auth = client
        .post(format!("{}/internal/deposits/phone", bff_base))
        .json(&json!({
            "userId": "usr_auth_probe",
            "amount": 100,
            "currency": "XAF",
            "provider": "MTN_CM",
            "reason": "auth probe"
        }))
        .send()
        .await?;
    assert_eq!(no_auth.status().as_u16(), 401);

    let non_bearer = client
        .post(format!("{}/internal/deposits/phone", bff_base))
        .header("Authorization", "Basic dGVzdDp0ZXN0")
        .json(&json!({
            "userId": "usr_auth_probe",
            "amount": 100,
            "currency": "XAF",
            "provider": "MTN_CM",
            "reason": "auth probe"
        }))
        .send()
        .await?;
    assert_eq!(non_bearer.status().as_u16(), 401);

    let invalid = client
        .post(format!("{}/internal/deposits/phone", bff_base))
        .header("Authorization", "Bearer definitely-invalid-token")
        .json(&json!({
            "userId": "usr_auth_probe",
            "amount": 100,
            "currency": "XAF",
            "provider": "MTN_CM",
            "reason": "auth probe"
        }))
        .send()
        .await?;
    assert_eq!(invalid.status().as_u16(), 401);

    let staff_missing = client
        .get(format!("{}/api/kyc/instances", staff_base))
        .send()
        .await?;
    assert_eq!(staff_missing.status().as_u16(), 401);

    let (token, subject) = get_client_token_and_subject(client, env).await?;
    ensure_bff_fixtures(&env.database_url, &subject).await?;

    let valid = client
        .post(format!("{}/internal/deposits/phone", bff_base))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "userId": subject,
            "amount": 100,
            "currency": "XAF",
            "provider": "MTN_CM",
            "reason": "auth probe"
        }))
        .send()
        .await?;
    assert_ne!(valid.status().as_u16(), 401);

    Ok(())
}

async fn scenario_bff_deposit_and_otp_flow(client: &reqwest::Client, env: &Env) -> Result<()> {
    let (token, subject) = get_client_token_and_subject(client, env).await?;
    ensure_bff_fixtures(&env.database_url, &subject).await?;
    reset_sms_sink(client, env).await?;

    let bff_base = format!("{}/bff", env.user_storage_url);
    let staff_base = format!("{}/staff", env.user_storage_url);

    let deposit_response = send_json(
        client,
        Method::POST,
        &format!("{}/internal/deposits/phone", bff_base),
        Some(&token),
        Some(json!({
            "userId": subject.clone(),
            "amount": 150000,
            "currency": "XAF",
            "provider": "MTN_CM",
            "reason": "e2e test"
        })),
    )
    .await?;
    assert_eq!(deposit_response.status, 201, "{}", deposit_response.text);

    let deposit_id = require_json_field(&deposit_response.body, "depositId")?
        .as_str()
        .ok_or_else(|| anyhow!("depositId must be a string"))?;

    let lookup = send_json(
        client,
        Method::GET,
        &format!("{}/internal/deposits/{}", bff_base, deposit_id),
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(lookup.status, 200, "{}", lookup.text);
    assert_eq!(
        lookup
            .body
            .as_ref()
            .and_then(|body| body.get("status"))
            .and_then(Value::as_str),
        Some("CONTACT_PROVIDED")
    );

    let session = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/sessions", bff_base),
        Some(&token),
        Some(json!({ "userId": subject })),
    )
    .await?;
    assert_eq!(session.status, 201, "{}", session.text);
    let session_id = require_json_field(&session.body, "id")?
        .as_str()
        .ok_or_else(|| anyhow!("session id must be a string"))?
        .to_owned();

    let step = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/steps", bff_base),
        Some(&token),
        Some(json!({
            "sessionId": session_id,
            "userId": subject,
            "type": "PHONE",
            "policy": {}
        })),
    )
    .await?;
    assert_eq!(step.status, 201, "{}", step.text);
    let step_id = require_json_field(&step.body, "id")?
        .as_str()
        .ok_or_else(|| anyhow!("step id must be a string"))?
        .to_owned();

    let step_before_issue = send_json(
        client,
        Method::GET,
        &format!("{}/internal/kyc/steps/{}", bff_base, step_id),
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(step_before_issue.status, 200, "{}", step_before_issue.text);
    assert_eq!(
        step_before_issue
            .body
            .as_ref()
            .and_then(|body| body.get("status"))
            .and_then(Value::as_str),
        Some("NOT_STARTED")
    );

    let issue = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/phone/otp/issue", bff_base),
        Some(&token),
        Some(json!({
            "stepId": step_id,
            "msisdn": "+237690000033",
            "channel": "SMS",
            "ttlSeconds": 120
        })),
    )
    .await?;
    assert_eq!(issue.status, 200, "{}", issue.text);
    let otp_ref = require_json_field(&issue.body, "otpRef")?
        .as_str()
        .ok_or_else(|| anyhow!("otpRef must be a string"))?
        .to_owned();

    let step_after_issue = send_json(
        client,
        Method::GET,
        &format!("{}/internal/kyc/steps/{}", bff_base, step_id),
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(step_after_issue.status, 200, "{}", step_after_issue.text);
    assert_eq!(
        step_after_issue
            .body
            .as_ref()
            .and_then(|body| body.get("status"))
            .and_then(Value::as_str),
        Some("IN_PROGRESS")
    );

    let otp = wait_for_otp(client, env, "+237690000033", Duration::from_secs(30)).await?;
    let wrong_code = if otp == "000000" { "000001" } else { "000000" };

    let detail_before_wrong_verify = send_json(
        client,
        Method::GET,
        &format!("{}/api/kyc/instances/{}", staff_base, session_id),
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(
        detail_before_wrong_verify.status, 200,
        "{}",
        detail_before_wrong_verify.text
    );
    let wrong_verify_attempts_before =
        step_attempts_count(&detail_before_wrong_verify.body, "VERIFY_OTP_ATTEMPT");

    let wrong_verify = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/phone/otp/verify", bff_base),
        Some(&token),
        Some(json!({
            "stepId": step_id,
            "otpRef": otp_ref,
            "code": wrong_code
        })),
    )
    .await?;
    assert_eq!(wrong_verify.status, 200, "{}", wrong_verify.text);
    assert_eq!(
        wrong_verify
            .body
            .as_ref()
            .and_then(|body| body.get("ok"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        wrong_verify
            .body
            .as_ref()
            .and_then(|body| body.get("reason"))
            .and_then(Value::as_str),
        Some("INVALID")
    );
    assert_eq!(
        wrong_verify
            .body
            .as_ref()
            .and_then(|body| body.get("stepStatus"))
            .and_then(Value::as_str),
        Some("IN_PROGRESS")
    );

    let detail_after_wrong_verify = send_json(
        client,
        Method::GET,
        &format!("{}/api/kyc/instances/{}", staff_base, session_id),
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(
        detail_after_wrong_verify.status, 200,
        "{}",
        detail_after_wrong_verify.text
    );
    assert!(
        step_attempts_count(&detail_after_wrong_verify.body, "VERIFY_OTP_ATTEMPT")
            > wrong_verify_attempts_before,
        "wrong OTP verification should append VERIFY_OTP_ATTEMPT entry"
    );

    let verify = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/phone/otp/verify", bff_base),
        Some(&token),
        Some(json!({
            "stepId": step_id,
            "otpRef": otp_ref,
            "code": otp
        })),
    )
    .await?;
    assert_eq!(verify.status, 200, "{}", verify.text);
    assert_eq!(
        verify
            .body
            .as_ref()
            .and_then(|body| body.get("ok"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        verify
            .body
            .as_ref()
            .and_then(|body| body.get("reason"))
            .and_then(Value::as_str),
        Some("VERIFIED")
    );
    assert_eq!(
        verify
            .body
            .as_ref()
            .and_then(|body| body.get("stepStatus"))
            .and_then(Value::as_str),
        Some("VERIFIED")
    );

    let step_after_verify = send_json(
        client,
        Method::GET,
        &format!("{}/internal/kyc/steps/{}", bff_base, step_id),
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(step_after_verify.status, 200, "{}", step_after_verify.text);
    assert_eq!(
        step_after_verify
            .body
            .as_ref()
            .and_then(|body| body.get("status"))
            .and_then(Value::as_str),
        Some("VERIFIED")
    );

    Ok(())
}

async fn scenario_bff_deposit_expiry_behavior(client: &reqwest::Client, env: &Env) -> Result<()> {
    let (token, subject) = get_client_token_and_subject(client, env).await?;
    ensure_bff_fixtures(&env.database_url, &subject).await?;

    let bff_base = format!("{}/bff", env.user_storage_url);

    let created = send_json(
        client,
        Method::POST,
        &format!("{}/internal/deposits/phone", bff_base),
        Some(&token),
        Some(json!({
            "userId": subject,
            "amount": 4200,
            "currency": "XAF",
            "provider": "MTN_CM",
            "reason": "expiry-check"
        })),
    )
    .await?;
    assert_eq!(created.status, 201, "{}", created.text);
    let deposit_id = require_json_field(&created.body, "depositId")?
        .as_str()
        .ok_or_else(|| anyhow!("depositId must be a string"))?
        .to_owned();

    force_deposit_expiry(
        &env.database_url,
        &deposit_id,
        chrono::Utc::now() - chrono::Duration::hours(3),
    )
    .await?;

    let fetched = send_json(
        client,
        Method::GET,
        &format!("{}/internal/deposits/{}", bff_base, deposit_id),
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(fetched.status, 200, "{}", fetched.text);
    assert_eq!(
        fetched
            .body
            .as_ref()
            .and_then(|body| body.get("status"))
            .and_then(Value::as_str),
        Some("EXPIRED")
    );

    Ok(())
}

async fn scenario_bff_session_resume_and_otp_limits(
    client: &reqwest::Client,
    env: &Env,
) -> Result<()> {
    let (token, subject) = get_client_token_and_subject(client, env).await?;
    ensure_bff_fixtures(&env.database_url, &subject).await?;
    reset_sms_sink(client, env).await?;

    let bff_base = format!("{}/bff", env.user_storage_url);

    let session_one = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/sessions", bff_base),
        Some(&token),
        Some(json!({ "userId": subject })),
    )
    .await?;
    assert_eq!(session_one.status, 201, "{}", session_one.text);
    let session_id_one = require_json_field(&session_one.body, "id")?
        .as_str()
        .ok_or_else(|| anyhow!("session id must be a string"))?
        .to_owned();

    let session_two = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/sessions", bff_base),
        Some(&token),
        Some(json!({ "userId": subject })),
    )
    .await?;
    assert_eq!(session_two.status, 201, "{}", session_two.text);
    let session_id_two = require_json_field(&session_two.body, "id")?
        .as_str()
        .ok_or_else(|| anyhow!("session id must be a string"))?
        .to_owned();
    assert_eq!(
        session_id_one, session_id_two,
        "session create/resume should be deterministic for a user"
    );

    let phone_step = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/steps", bff_base),
        Some(&token),
        Some(json!({
            "sessionId": session_id_one,
            "userId": subject,
            "type": "PHONE",
            "policy": {}
        })),
    )
    .await?;
    assert_eq!(phone_step.status, 201, "{}", phone_step.text);
    let phone_step_id = require_json_field(&phone_step.body, "id")?
        .as_str()
        .ok_or_else(|| anyhow!("phone step id must be a string"))?
        .to_owned();

    let email_step = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/steps", bff_base),
        Some(&token),
        Some(json!({
            "sessionId": session_id_two,
            "userId": subject,
            "type": "EMAIL",
            "policy": {}
        })),
    )
    .await?;
    assert_eq!(email_step.status, 201, "{}", email_step.text);
    let email_step_id = require_json_field(&email_step.body, "id")?
        .as_str()
        .ok_or_else(|| anyhow!("email step id must be a string"))?
        .to_owned();
    assert!(
        email_step_id.ends_with("__EMAIL"),
        "expected deterministic EMAIL step id"
    );

    let address_step = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/steps", bff_base),
        Some(&token),
        Some(json!({
            "sessionId": session_id_two,
            "userId": subject,
            "type": "ADDRESS",
            "policy": {}
        })),
    )
    .await?;
    assert_eq!(address_step.status, 201, "{}", address_step.text);
    let address_step_id = require_json_field(&address_step.body, "id")?
        .as_str()
        .ok_or_else(|| anyhow!("address step id must be a string"))?
        .to_owned();
    assert!(
        address_step_id.ends_with("__ADDRESS"),
        "expected deterministic ADDRESS step id"
    );

    let identity_step = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/steps", bff_base),
        Some(&token),
        Some(json!({
            "sessionId": session_id_two,
            "userId": subject,
            "type": "IDENTITY",
            "policy": {}
        })),
    )
    .await?;
    assert_eq!(identity_step.status, 201, "{}", identity_step.text);
    let identity_step_id = require_json_field(&identity_step.body, "id")?
        .as_str()
        .ok_or_else(|| anyhow!("identity step id must be a string"))?
        .to_owned();
    assert!(
        identity_step_id.ends_with("__IDENTITY"),
        "expected deterministic IDENTITY step id"
    );

    let mut last_otp_ref = String::new();
    for attempt in 0..5 {
        let issue = send_json(
            client,
            Method::POST,
            &format!("{}/internal/kyc/phone/otp/issue", bff_base),
            Some(&token),
            Some(json!({
                "stepId": phone_step_id,
                "msisdn": "+237690000077",
                "channel": "SMS",
                "ttlSeconds": 30
            })),
        )
        .await?;
        assert_eq!(
            issue.status,
            200,
            "issue attempt {} failed: {}",
            attempt + 1,
            issue.text
        );
        last_otp_ref = require_json_field(&issue.body, "otpRef")?
            .as_str()
            .ok_or_else(|| anyhow!("otpRef must be a string"))?
            .to_owned();
    }

    let limited = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/phone/otp/issue", bff_base),
        Some(&token),
        Some(json!({
            "stepId": phone_step_id,
            "msisdn": "+237690000077",
            "channel": "SMS",
            "ttlSeconds": 30
        })),
    )
    .await?;
    assert_eq!(limited.status, 429, "{}", limited.text);
    assert_eq!(
        limited
            .body
            .as_ref()
            .and_then(|body| body.get("error_key"))
            .and_then(Value::as_str),
        Some("OTP_RATE_LIMITED")
    );

    let otp = wait_for_otp(client, env, "+237690000077", Duration::from_secs(30)).await?;
    sleep(Duration::from_secs(31)).await;

    let expired = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/phone/otp/verify", bff_base),
        Some(&token),
        Some(json!({
            "stepId": phone_step_id,
            "otpRef": last_otp_ref,
            "code": otp
        })),
    )
    .await?;
    assert_eq!(expired.status, 200, "{}", expired.text);
    assert_eq!(
        expired
            .body
            .as_ref()
            .and_then(|body| body.get("ok"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        expired
            .body
            .as_ref()
            .and_then(|body| body.get("reason"))
            .and_then(Value::as_str),
        Some("EXPIRED")
    );
    assert_eq!(
        expired
            .body
            .as_ref()
            .and_then(|body| body.get("stepStatus"))
            .and_then(Value::as_str),
        Some("FAILED")
    );

    Ok(())
}

async fn scenario_bff_email_magic_and_uploads(client: &reqwest::Client, env: &Env) -> Result<()> {
    let (token, subject) = get_client_token_and_subject(client, env).await?;
    ensure_bff_fixtures(&env.database_url, &subject).await?;

    let bff_base = format!("{}/bff", env.user_storage_url);
    let session = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/sessions", bff_base),
        Some(&token),
        Some(json!({ "userId": subject })),
    )
    .await?;
    assert_eq!(session.status, 201, "{}", session.text);
    let session_id = require_json_field(&session.body, "id")?
        .as_str()
        .ok_or_else(|| anyhow!("session id must be a string"))?
        .to_owned();

    let email_step = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/steps", bff_base),
        Some(&token),
        Some(json!({
            "sessionId": session_id,
            "userId": subject,
            "type": "EMAIL",
            "policy": {}
        })),
    )
    .await?;
    assert_eq!(email_step.status, 201, "{}", email_step.text);
    let email_step_id = require_json_field(&email_step.body, "id")?
        .as_str()
        .ok_or_else(|| anyhow!("email step id must be a string"))?
        .to_owned();

    let issue_magic = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/email/magic/issue", bff_base),
        Some(&token),
        Some(json!({
            "stepId": email_step_id,
            "email": "e2e-magic@example.com",
            "ttlSeconds": 120
        })),
    )
    .await?;
    assert_eq!(issue_magic.status, 200, "{}", issue_magic.text);
    let token_ref = require_json_field(&issue_magic.body, "tokenRef")?
        .as_str()
        .ok_or_else(|| anyhow!("tokenRef must be a string"))?;

    let invalid_magic = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/email/magic/verify", bff_base),
        Some(&token),
        Some(json!({
            "token": format!("{token_ref}.invalid-secret")
        })),
    )
    .await?;
    assert_eq!(invalid_magic.status, 200, "{}", invalid_magic.text);
    assert_eq!(
        invalid_magic
            .body
            .as_ref()
            .and_then(|body| body.get("ok"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        invalid_magic
            .body
            .as_ref()
            .and_then(|body| body.get("reason"))
            .and_then(Value::as_str),
        Some("INVALID")
    );

    let known_token_ref = format!("mef_e2e_{}", chrono::Utc::now().timestamp_millis());
    let known_secret = "e2e-magic-known-secret";
    let known_hash = hash_argon2_secret(known_secret)?;
    insert_magic_email_attempt(
        &env.database_url,
        &session_id,
        &known_token_ref,
        &known_hash,
        chrono::Utc::now() + chrono::Duration::minutes(5),
    )
    .await?;

    let valid_magic = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/email/magic/verify", bff_base),
        Some(&token),
        Some(json!({
            "token": format!("{known_token_ref}.{known_secret}")
        })),
    )
    .await?;
    assert_eq!(valid_magic.status, 200, "{}", valid_magic.text);
    assert_eq!(
        valid_magic
            .body
            .as_ref()
            .and_then(|body| body.get("ok"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        valid_magic
            .body
            .as_ref()
            .and_then(|body| body.get("reason"))
            .and_then(Value::as_str),
        Some("VERIFIED")
    );
    assert_eq!(
        valid_magic
            .body
            .as_ref()
            .and_then(|body| body.get("stepStatus"))
            .and_then(Value::as_str),
        Some("VERIFIED")
    );

    let presign = send_json(
        client,
        Method::POST,
        &format!("{}/internal/uploads/presign", bff_base),
        Some(&token),
        Some(json!({
            "stepId": email_step_id,
            "userId": subject,
            "purpose": "KYC_IDENTITY",
            "assetType": "ID_FRONT",
            "mime": "image/jpeg",
            "sizeBytes": 12
        })),
    )
    .await?;
    assert_eq!(presign.status, 200, "{}", presign.text);

    let upload_id = require_json_field(&presign.body, "uploadId")?
        .as_str()
        .ok_or_else(|| anyhow!("uploadId must be a string"))?;
    let bucket = require_json_field(&presign.body, "bucket")?
        .as_str()
        .ok_or_else(|| anyhow!("bucket must be a string"))?;
    let object_key = require_json_field(&presign.body, "objectKey")?
        .as_str()
        .ok_or_else(|| anyhow!("objectKey must be a string"))?;
    let upload_url = require_json_field(&presign.body, "url")?
        .as_str()
        .ok_or_else(|| anyhow!("url must be a string"))?;
    let (upload_url, host_override) = if upload_url.contains("://minio:9000") {
        (
            upload_url.replace("://minio:9000", "://127.0.0.1:9000"),
            Some("minio:9000"),
        )
    } else {
        (upload_url.to_owned(), None)
    };
    assert_eq!(
        presign
            .body
            .as_ref()
            .and_then(|body| body.get("method"))
            .and_then(Value::as_str),
        Some("PUT")
    );

    let mut upload_req = client.put(&upload_url);
    if let Some(host) = host_override {
        upload_req = upload_req.header("Host", host);
    }
    if let Some(headers) = presign
        .body
        .as_ref()
        .and_then(|body| body.get("headers"))
        .and_then(Value::as_object)
    {
        for (key, value) in headers {
            if let Some(value) = value.as_str() {
                upload_req = upload_req.header(key, value);
            }
        }
    }
    let upload_resp = upload_req
        .body("hello-upload")
        .send()
        .await
        .map_err(|error| anyhow!("upload PUT request failed: {error}"))?;
    let upload_status = upload_resp.status();
    let upload_body = upload_resp.text().await.unwrap_or_default();
    assert!(
        upload_status.is_success(),
        "upload PUT should succeed, got {} body={}",
        upload_status,
        upload_body
    );

    let complete = send_json(
        client,
        Method::POST,
        &format!("{}/internal/uploads/complete", bff_base),
        Some(&token),
        Some(json!({
            "uploadId": upload_id,
            "bucket": bucket,
            "objectKey": object_key
        })),
    )
    .await?;
    assert_eq!(complete.status, 200, "{}", complete.text);
    require_json_field(&complete.body, "evidenceId")?
        .as_str()
        .ok_or_else(|| anyhow!("evidenceId must be a string"))?;

    let invalid_complete = send_json(
        client,
        Method::POST,
        &format!("{}/internal/uploads/complete", bff_base),
        Some(&token),
        Some(json!({
            "uploadId": "upl_invalid_e2e",
            "bucket": bucket,
            "objectKey": format!("{object_key}.missing")
        })),
    )
    .await?;
    assert_eq!(invalid_complete.status, 404, "{}", invalid_complete.text);
    assert_eq!(
        invalid_complete
            .body
            .as_ref()
            .and_then(|body| body.get("error_key"))
            .and_then(Value::as_str),
        Some("UPLOAD_NOT_FOUND")
    );

    Ok(())
}

async fn scenario_bff_deposit_denies_non_owner(client: &reqwest::Client, env: &Env) -> Result<()> {
    let (token, subject) = get_client_token_and_subject(client, env).await?;
    ensure_bff_fixtures(&env.database_url, &subject).await?;

    let foreign_deposit_id =
        create_foreign_deposit_fixture(&env.database_url, "usr_e2e_foreign_owner_001").await?;
    let bff_base = format!("{}/bff", env.user_storage_url);

    let lookup = send_json(
        client,
        Method::GET,
        &format!("{}/internal/deposits/{}", bff_base, foreign_deposit_id),
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(lookup.status, 401, "{}", lookup.text);
    assert_eq!(
        lookup
            .body
            .as_ref()
            .and_then(|body| body.get("error_key"))
            .and_then(Value::as_str),
        Some("UNAUTHORIZED")
    );

    Ok(())
}

async fn scenario_staff_instance_detail_and_retry(
    client: &reqwest::Client,
    env: &Env,
) -> Result<()> {
    let (token, subject) = get_client_token_and_subject(client, env).await?;
    ensure_bff_fixtures(&env.database_url, &subject).await?;
    reset_sms_sink(client, env).await?;

    let bff_base = format!("{}/bff", env.user_storage_url);
    let staff_base = format!("{}/staff", env.user_storage_url);

    let (session_id, _step_id, _) =
        create_phone_step_and_issue_otp(client, &bff_base, &token, &subject, "+237690000055", 120)
            .await?;

    let detail = send_json(
        client,
        Method::GET,
        &format!("{}/api/kyc/instances/{}", staff_base, session_id),
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(detail.status, 200, "{}", detail.text);
    assert_eq!(
        detail
            .body
            .as_ref()
            .and_then(|body| body.get("instanceId"))
            .and_then(Value::as_str),
        Some(session_id.as_str())
    );

    let issue_attempts_before = step_attempts_count(&detail.body, "ISSUE_OTP");
    assert!(
        issue_attempts_before >= 1,
        "expected at least one ISSUE_OTP attempt"
    );

    let retry = send_json(
        client,
        Method::POST,
        &format!("{}/api/kyc/instances/{}/retry", staff_base, session_id),
        Some(&token),
        Some(json!({
            "stepName": "ISSUE_OTP",
            "mode": "NEW_ATTEMPT"
        })),
    )
    .await?;
    assert_eq!(retry.status, 200, "{}", retry.text);
    assert_eq!(
        retry
            .body
            .as_ref()
            .and_then(|body| body.get("stepName"))
            .and_then(Value::as_str),
        Some("ISSUE_OTP")
    );
    require_json_field(&retry.body, "attemptId")?
        .as_str()
        .ok_or_else(|| anyhow!("retry response attemptId must be a string"))?;

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut updated = false;
    while Instant::now() < deadline {
        let current = send_json(
            client,
            Method::GET,
            &format!("{}/api/kyc/instances/{}", staff_base, session_id),
            Some(&token),
            None,
        )
        .await?;
        if current.status == 200
            && step_attempts_count(&current.body, "ISSUE_OTP") > issue_attempts_before
        {
            updated = true;
            break;
        }
        sleep(Duration::from_millis(500)).await;
    }
    assert!(
        updated,
        "retry should create an additional ISSUE_OTP attempt for instance {}",
        session_id
    );

    Ok(())
}

async fn scenario_staff_summary_and_instances(client: &reqwest::Client, env: &Env) -> Result<()> {
    let (token, subject) = get_client_token_and_subject(client, env).await?;
    ensure_bff_fixtures(&env.database_url, &subject).await?;
    reset_sms_sink(client, env).await?;

    let bff_base = format!("{}/bff", env.user_storage_url);
    let staff_base = format!("{}/staff", env.user_storage_url);

    let deposit = send_json(
        client,
        Method::POST,
        &format!("{}/internal/deposits/phone", bff_base),
        Some(&token),
        Some(json!({
            "userId": subject.clone(),
            "amount": 1200,
            "currency": "XAF",
            "provider": "MTN_CM",
            "reason": "summary-fixture"
        })),
    )
    .await?;
    assert_eq!(deposit.status, 201, "{}", deposit.text);

    let _ =
        create_phone_step_and_issue_otp(client, &bff_base, &token, &subject, "+237690000066", 120)
            .await?;

    let summary = send_json(
        client,
        Method::GET,
        &format!("{}/api/kyc/reports/summary", staff_base),
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(summary.status, 200, "{}", summary.text);

    let by_kind = summary
        .body
        .as_ref()
        .and_then(|body| body.get("byKind"))
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("summary.byKind should be an object"))?;
    assert!(
        by_kind
            .get("KYC_PHONE_OTP")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            >= 1
    );
    assert!(
        by_kind
            .get("KYC_FIRST_DEPOSIT")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            >= 1
    );

    summary
        .body
        .as_ref()
        .and_then(|body| body.get("byStatus"))
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("summary.byStatus should be an object"))?;

    let filtered = send_json(
        client,
        Method::GET,
        &format!(
            "{}/api/kyc/instances?kind=KYC_PHONE_OTP&userId={}&page=1&limit=10",
            staff_base, subject
        ),
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(filtered.status, 200, "{}", filtered.text);
    let filtered_items = filtered
        .body
        .as_ref()
        .and_then(|body| body.get("items"))
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("filtered instances.items should be an array"))?;
    assert!(
        !filtered_items.is_empty(),
        "expected at least one filtered item"
    );
    assert!(filtered_items.iter().all(|item| {
        item.get("kind")
            .and_then(Value::as_str)
            .map(|kind| kind == "KYC_PHONE_OTP")
            .unwrap_or(false)
    }));

    let paged = send_json(
        client,
        Method::GET,
        &format!("{}/api/kyc/instances?page=1&limit=1", staff_base),
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(paged.status, 200, "{}", paged.text);
    assert_eq!(
        paged
            .body
            .as_ref()
            .and_then(|body| body.get("page"))
            .and_then(Value::as_i64),
        Some(1)
    );
    assert_eq!(
        paged
            .body
            .as_ref()
            .and_then(|body| body.get("pageSize"))
            .and_then(Value::as_i64),
        Some(1)
    );

    let missing = send_json(
        client,
        Method::GET,
        &format!("{}/api/kyc/instances/missing-instance", staff_base),
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(missing.status, 404, "{}", missing.text);

    Ok(())
}

async fn scenario_staff_deposit_flow_triggers_worker_and_cuss(
    client: &reqwest::Client,
    env: &Env,
) -> Result<()> {
    let (token, subject) = get_client_token_and_subject(client, env).await?;
    ensure_bff_fixtures(&env.database_url, &subject).await?;

    let bff_base = format!("{}/bff", env.user_storage_url);
    let staff_base = format!("{}/staff", env.user_storage_url);

    let reset = send_json(
        client,
        Method::POST,
        &format!("{}/__admin/reset", env.cuss_url),
        None,
        Some(json!({})),
    )
    .await?;
    assert_eq!(reset.status, 200, "{}", reset.text);

    let deposit_response = send_json(
        client,
        Method::POST,
        &format!("{}/internal/deposits/phone", bff_base),
        Some(&token),
        Some(json!({
            "userId": subject,
            "amount": 2500,
            "currency": "XAF",
            "provider": "MTN_CM",
            "reason": "deposit-worker-flow"
        })),
    )
    .await?;
    assert_eq!(deposit_response.status, 201, "{}", deposit_response.text);
    let instance_id = require_json_field(&deposit_response.body, "depositId")?
        .as_str()
        .ok_or_else(|| anyhow!("depositId must be a string"))?;

    let confirm = send_json(
        client,
        Method::POST,
        &format!(
            "{}/api/kyc/deposits/{}/confirm-payment",
            staff_base, instance_id
        ),
        Some(&token),
        Some(json!({
            "note": "received",
            "providerTxnId": "txn-e2e-1"
        })),
    )
    .await?;
    assert_eq!(confirm.status, 200, "{}", confirm.text);

    let approve = send_json(
        client,
        Method::POST,
        &format!("{}/api/kyc/deposits/{}/approve", staff_base, instance_id),
        Some(&token),
        Some(json!({
            "firstName": "E2E",
            "lastName": "Runner",
            "depositAmount": 2500
        })),
    )
    .await?;
    assert_eq!(approve.status, 200, "{}", approve.text);

    let cuss_deadline = Instant::now() + Duration::from_secs(30);
    let mut register_seen = false;
    let mut approve_seen = false;
    while Instant::now() < cuss_deadline {
        let recorded = send_json(
            client,
            Method::GET,
            &format!("{}/__admin/requests", env.cuss_url),
            None,
            None,
        )
        .await?;
        assert_eq!(recorded.status, 200, "{}", recorded.text);

        if let Some(items) = recorded.body.as_ref().and_then(Value::as_array) {
            register_seen = items.iter().any(|item| {
                item.get("endpoint")
                    .and_then(Value::as_str)
                    .map(|endpoint| endpoint == "register")
                    .unwrap_or(false)
            });
            approve_seen = items.iter().any(|item| {
                item.get("endpoint")
                    .and_then(Value::as_str)
                    .map(|endpoint| endpoint == "approve")
                    .unwrap_or(false)
            });
        }

        if register_seen && approve_seen {
            break;
        }
        sleep(Duration::from_millis(500)).await;
    }
    assert!(register_seen, "expected cuss register call");
    assert!(approve_seen, "expected cuss approve-and-deposit call");

    let status_deadline = Instant::now() + Duration::from_secs(45);
    let mut completed = false;
    while Instant::now() < status_deadline {
        let detail = send_json(
            client,
            Method::GET,
            &format!("{}/api/kyc/instances/{}", staff_base, instance_id),
            Some(&token),
            None,
        )
        .await?;
        assert_eq!(detail.status, 200, "{}", detail.text);

        let status = detail
            .body
            .as_ref()
            .and_then(|body| body.get("status"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let register_status = latest_attempt_status(&detail.body, "REGISTER_CUSTOMER");
        let approve_status = latest_attempt_status(&detail.body, "APPROVE_AND_DEPOSIT");

        if status == "COMPLETED"
            && register_status.as_deref() == Some("SUCCEEDED")
            && approve_status.as_deref() == Some("SUCCEEDED")
        {
            completed = true;
            break;
        }

        sleep(Duration::from_millis(500)).await;
    }
    assert!(
        completed,
        "instance {} should reach COMPLETED with succeeded async deposit steps",
        instance_id
    );

    Ok(())
}

async fn create_phone_step_and_issue_otp(
    client: &reqwest::Client,
    bff_base: &str,
    token: &str,
    subject: &str,
    msisdn: &str,
    ttl_seconds: i64,
) -> Result<(String, String, String)> {
    let session = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/sessions", bff_base),
        Some(token),
        Some(json!({ "userId": subject })),
    )
    .await?;
    assert_eq!(session.status, 201, "{}", session.text);
    let session_id = require_json_field(&session.body, "id")?
        .as_str()
        .ok_or_else(|| anyhow!("session id must be a string"))?
        .to_owned();

    let step = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/steps", bff_base),
        Some(token),
        Some(json!({
            "sessionId": session_id,
            "userId": subject,
            "type": "PHONE",
            "policy": {}
        })),
    )
    .await?;
    assert_eq!(step.status, 201, "{}", step.text);
    let step_id = require_json_field(&step.body, "id")?
        .as_str()
        .ok_or_else(|| anyhow!("step id must be a string"))?
        .to_owned();

    let issue = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/phone/otp/issue", bff_base),
        Some(token),
        Some(json!({
            "stepId": step_id,
            "msisdn": msisdn,
            "channel": "SMS",
            "ttlSeconds": ttl_seconds
        })),
    )
    .await?;
    assert_eq!(issue.status, 200, "{}", issue.text);
    let otp_ref = require_json_field(&issue.body, "otpRef")?
        .as_str()
        .ok_or_else(|| anyhow!("otpRef must be a string"))?
        .to_owned();

    Ok((session_id, step_id, otp_ref))
}

fn hash_argon2_secret(secret: &str) -> Result<String> {
    let salt = SaltString::encode_b64(b"e2e-magic-token-salt")
        .map_err(|error| anyhow!("failed to build magic token salt: {error}"))?;
    let hash = argon2::Argon2::default()
        .hash_password(secret.as_bytes(), &salt)
        .map_err(|error| anyhow!("failed to hash magic token secret: {error}"))?;
    Ok(hash.to_string())
}

async fn force_deposit_expiry(
    database_url: &str,
    instance_id: &str,
    expires_at: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    let (client, connection) = tokio_postgres::connect(database_url, NoTls)
        .await
        .map_err(|error| anyhow!("failed to connect to postgres: {error}"))?;

    tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("postgres connection task failed: {error}");
        }
    });

    client
        .execute(
            r#"
            UPDATE sm_instance
            SET context = jsonb_set(
                context,
                '{deposit,expires_at}',
                to_jsonb($2::text),
                true
            )
            WHERE id = $1
            "#,
            &[&instance_id, &expires_at.to_rfc3339()],
        )
        .await
        .map_err(|error| anyhow!("failed to force deposit expiry: {error}"))?;

    Ok(())
}

async fn insert_magic_email_attempt(
    database_url: &str,
    session_id: &str,
    token_ref: &str,
    token_hash: &str,
    expires_at: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    let (client, connection) = tokio_postgres::connect(database_url, NoTls)
        .await
        .map_err(|error| anyhow!("failed to connect to postgres: {error}"))?;

    tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("postgres connection task failed: {error}");
        }
    });

    let attempt_id = format!("sma_e2e_magic_{}", chrono::Utc::now().timestamp_millis());
    let output = json!({
        "token_ref": token_ref,
        "expires_at": expires_at,
        "token_hash": token_hash
    });
    let input = json!({
        "email": "e2e-magic@example.com",
        "ttl_seconds": 300
    });

    client
        .execute(
            r#"
            INSERT INTO sm_step_attempt (
                id,
                instance_id,
                step_name,
                attempt_no,
                status,
                external_ref,
                input,
                output,
                error,
                queued_at,
                started_at,
                finished_at,
                next_retry_at
            ) VALUES (
                $1,
                $2,
                'ISSUE_MAGIC_EMAIL',
                900,
                'SUCCEEDED',
                $3,
                $4::text::jsonb,
                $5::text::jsonb,
                NULL,
                NOW(),
                NOW(),
                NOW(),
                NULL
            )
            ON CONFLICT (id) DO NOTHING
            "#,
            &[
                &attempt_id,
                &session_id,
                &token_ref,
                &input.to_string(),
                &output.to_string(),
            ],
        )
        .await
        .map_err(|error| anyhow!("failed to insert magic email attempt fixture: {error}"))?;

    Ok(())
}

fn step_attempts_count(detail_body: &Option<Value>, step_name: &str) -> usize {
    detail_body
        .as_ref()
        .and_then(|body| body.get("steps"))
        .and_then(Value::as_array)
        .and_then(|steps| {
            steps.iter().find(|step| {
                step.get("stepName")
                    .and_then(Value::as_str)
                    .map(|name| name == step_name)
                    .unwrap_or(false)
            })
        })
        .and_then(|step| step.get("attempts"))
        .and_then(Value::as_array)
        .map(|attempts| attempts.len())
        .unwrap_or(0)
}

fn latest_attempt_status(detail_body: &Option<Value>, step_name: &str) -> Option<String> {
    detail_body
        .as_ref()
        .and_then(|body| body.get("steps"))
        .and_then(Value::as_array)
        .and_then(|steps| {
            steps.iter().find(|step| {
                step.get("stepName")
                    .and_then(Value::as_str)
                    .map(|name| name == step_name)
                    .unwrap_or(false)
            })
        })
        .and_then(|step| step.get("latestAttempt"))
        .and_then(|attempt| attempt.get("status"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

async fn scenario_kc_signature_and_surface(client: &reqwest::Client, env: &Env) -> Result<()> {
    let realm = "e2e-testing";
    let users_search_path = "/kc/v1/users/search";
    let search_body = json!({ "realm": realm });
    let search_body_str = serde_json::to_string(&search_body)
        .map_err(|error| anyhow!("failed to serialize search body: {error}"))?;
    let now = chrono::Utc::now().timestamp();

    let missing_timestamp = send_kc_custom(
        client,
        env,
        Method::POST,
        users_search_path,
        Some(search_body_str.clone()),
        KcSignatureOptions {
            include_timestamp: false,
            include_signature: true,
            timestamp: now,
            sign_method: "POST",
            sign_path: users_search_path,
            sign_body: &search_body_str,
            signature_override: Some("bogus-signature"),
        },
    )
    .await?;
    assert_eq!(missing_timestamp.status, 401, "{}", missing_timestamp.text);
    assert!(
        response_message(&missing_timestamp)
            .unwrap_or_default()
            .contains("Missing x-kc-timestamp")
    );

    let missing_signature = send_kc_custom(
        client,
        env,
        Method::POST,
        users_search_path,
        Some(search_body_str.clone()),
        KcSignatureOptions {
            include_timestamp: true,
            include_signature: false,
            timestamp: now,
            sign_method: "POST",
            sign_path: users_search_path,
            sign_body: &search_body_str,
            signature_override: None,
        },
    )
    .await?;
    assert_eq!(missing_signature.status, 401, "{}", missing_signature.text);
    assert!(
        response_message(&missing_signature)
            .unwrap_or_default()
            .contains("Missing x-kc-signature")
    );

    let skewed = send_kc_custom(
        client,
        env,
        Method::POST,
        users_search_path,
        Some(search_body_str.clone()),
        KcSignatureOptions {
            include_timestamp: true,
            include_signature: true,
            timestamp: now - 3600,
            sign_method: "POST",
            sign_path: users_search_path,
            sign_body: &search_body_str,
            signature_override: None,
        },
    )
    .await?;
    assert_eq!(skewed.status, 401, "{}", skewed.text);
    assert!(
        response_message(&skewed)
            .unwrap_or_default()
            .contains("Timestamp out of skew")
    );

    let invalid_signature = send_kc_custom(
        client,
        env,
        Method::POST,
        users_search_path,
        Some(search_body_str.clone()),
        KcSignatureOptions {
            include_timestamp: true,
            include_signature: true,
            timestamp: now,
            sign_method: "POST",
            sign_path: users_search_path,
            sign_body: &search_body_str,
            signature_override: Some("definitely-invalid"),
        },
    )
    .await?;
    assert_eq!(invalid_signature.status, 401, "{}", invalid_signature.text);
    assert!(
        response_message(&invalid_signature)
            .unwrap_or_default()
            .contains("Invalid signature")
    );

    let method_mismatch = send_kc_custom(
        client,
        env,
        Method::POST,
        users_search_path,
        Some(search_body_str.clone()),
        KcSignatureOptions {
            include_timestamp: true,
            include_signature: true,
            timestamp: now,
            sign_method: "GET",
            sign_path: users_search_path,
            sign_body: &search_body_str,
            signature_override: None,
        },
    )
    .await?;
    assert_eq!(method_mismatch.status, 401, "{}", method_mismatch.text);
    assert!(
        response_message(&method_mismatch)
            .unwrap_or_default()
            .contains("Invalid signature")
    );

    let path_mismatch = send_kc_custom(
        client,
        env,
        Method::POST,
        users_search_path,
        Some(search_body_str.clone()),
        KcSignatureOptions {
            include_timestamp: true,
            include_signature: true,
            timestamp: now,
            sign_method: "POST",
            sign_path: "/kc/v1/users",
            sign_body: &search_body_str,
            signature_override: None,
        },
    )
    .await?;
    assert_eq!(path_mismatch.status, 401, "{}", path_mismatch.text);
    assert!(
        response_message(&path_mismatch)
            .unwrap_or_default()
            .contains("Invalid signature")
    );

    let body_mismatch = send_kc_custom(
        client,
        env,
        Method::POST,
        users_search_path,
        Some(search_body_str.clone()),
        KcSignatureOptions {
            include_timestamp: true,
            include_signature: true,
            timestamp: now,
            sign_method: "POST",
            sign_path: users_search_path,
            sign_body: "{}",
            signature_override: None,
        },
    )
    .await?;
    assert_eq!(body_mismatch.status, 401, "{}", body_mismatch.text);
    assert!(
        response_message(&body_mismatch)
            .unwrap_or_default()
            .contains("Invalid signature")
    );

    let huge_username = "u".repeat(270_000);
    let huge_body = format!(
        r#"{{"realm":"{realm}","username":"{huge_username}"}}"#,
        realm = realm
    );
    let too_large = send_kc_custom(
        client,
        env,
        Method::POST,
        "/kc/v1/users",
        Some(huge_body.clone()),
        KcSignatureOptions {
            include_timestamp: true,
            include_signature: true,
            timestamp: now,
            sign_method: "POST",
            sign_path: "/kc/v1/users",
            sign_body: &huge_body,
            signature_override: None,
        },
    )
    .await?;
    assert_eq!(too_large.status, 401, "{}", too_large.text);
    let too_large_message = response_message(&too_large).unwrap_or_default();
    assert!(
        too_large_message.contains("invalid request body")
            || too_large_message.contains("Body too large")
    );

    let encoded_path = "/kc/v1/users/%6Dissing-kc-user";
    let encoded_ok = send_kc_signed(client, env, Method::GET, encoded_path, None).await?;
    assert_eq!(encoded_ok.status, 404, "{}", encoded_ok.text);

    let username = format!("kc-e2e-{}", chrono::Utc::now().timestamp_millis());
    let create_user = send_kc_signed(
        client,
        env,
        Method::POST,
        "/kc/v1/users",
        Some(json!({
            "realm": realm,
            "username": username,
            "first_name": "KC",
            "last_name": "User",
            "enabled": true
        })),
    )
    .await?;
    assert_eq!(create_user.status, 201, "{}", create_user.text);
    assert_eq!(
        create_user
            .body
            .as_ref()
            .and_then(|body| body.get("username"))
            .and_then(Value::as_str),
        Some(username.as_str())
    );
    let user_id = require_json_field(&create_user.body, "user_id")?
        .as_str()
        .ok_or_else(|| anyhow!("kc create user response missing user_id"))?
        .to_owned();

    let get_existing = send_kc_signed(
        client,
        env,
        Method::GET,
        &format!("/kc/v1/users/{}", user_id),
        None,
    )
    .await?;
    assert_eq!(get_existing.status, 200, "{}", get_existing.text);

    let get_missing = send_kc_signed(
        client,
        env,
        Method::GET,
        "/kc/v1/users/missing-kc-user",
        None,
    )
    .await?;
    assert_eq!(get_missing.status, 404, "{}", get_missing.text);

    let update_existing = send_kc_signed(
        client,
        env,
        Method::PUT,
        &format!("/kc/v1/users/{}", user_id),
        Some(json!({
            "realm": realm,
            "username": username,
            "first_name": "KC-UPDATED",
            "enabled": true
        })),
    )
    .await?;
    assert_eq!(update_existing.status, 200, "{}", update_existing.text);

    let update_missing = send_kc_signed(
        client,
        env,
        Method::PUT,
        "/kc/v1/users/missing-kc-user",
        Some(json!({
            "realm": realm,
            "username": "missing-user",
            "enabled": true
        })),
    )
    .await?;
    assert_eq!(update_missing.status, 404, "{}", update_missing.text);

    let search = send_kc_signed(
        client,
        env,
        Method::POST,
        "/kc/v1/users/search",
        Some(json!({
            "realm": realm,
            "username": username,
            "exact": true
        })),
    )
    .await?;
    assert_eq!(search.status, 200, "{}", search.text);
    let found_user = search
        .body
        .as_ref()
        .and_then(|body| body.get("users"))
        .and_then(Value::as_array)
        .map(|users| {
            users.iter().any(|entry| {
                entry
                    .get("user_id")
                    .and_then(Value::as_str)
                    .map(|value| value == user_id)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    assert!(found_user, "expected created user in search result");

    let user_a = create_kc_user(client, env, "kc-a").await?;
    let user_b = create_kc_user(client, env, "kc-b").await?;
    let bind_device_id = format!("dvc_e2e_{}", chrono::Utc::now().timestamp_millis());
    let bind_jkt = format!("jkt_e2e_{}", chrono::Utc::now().timestamp_millis());
    let public_jwk = json!({
        "kty": "EC",
        "y": "cN7YwN2Y5vX6",
        "crv": "P-256",
        "x": "f83OJ3D2xF4"
    });

    let bind_one = send_kc_signed(
        client,
        env,
        Method::POST,
        "/kc/v1/enrollments/bind",
        Some(json!({
            "realm": realm,
            "client_id": "resource-server",
            "user_id": user_a.clone(),
            "device_id": bind_device_id,
            "jkt": bind_jkt,
            "public_jwk": public_jwk.clone()
        })),
    )
    .await?;
    assert_eq!(bind_one.status, 200, "{}", bind_one.text);
    let first_record_id = require_json_field(&bind_one.body, "device_record_id")?
        .as_str()
        .ok_or_else(|| anyhow!("bind response missing device_record_id"))?
        .to_owned();
    let expected_record_id = expected_device_record_id(&bind_device_id, &public_jwk)?;
    assert_eq!(first_record_id, expected_record_id);

    let bind_same = send_kc_signed(
        client,
        env,
        Method::POST,
        "/kc/v1/enrollments/bind",
        Some(json!({
            "realm": realm,
            "client_id": "resource-server",
            "user_id": user_a.clone(),
            "device_id": bind_device_id,
            "jkt": bind_jkt,
            "public_jwk": public_jwk.clone()
        })),
    )
    .await?;
    assert_eq!(bind_same.status, 200, "{}", bind_same.text);
    let second_record_id = require_json_field(&bind_same.body, "device_record_id")?
        .as_str()
        .ok_or_else(|| anyhow!("second bind response missing device_record_id"))?;
    assert_eq!(second_record_id, expected_record_id);

    let lookup_missing = send_kc_signed(
        client,
        env,
        Method::POST,
        "/kc/v1/devices/lookup",
        Some(json!({
            "device_id": "dvc_missing_e2e"
        })),
    )
    .await?;
    assert_eq!(lookup_missing.status, 404, "{}", lookup_missing.text);

    let lookup_one = send_kc_signed(
        client,
        env,
        Method::POST,
        "/kc/v1/devices/lookup",
        Some(json!({
            "device_id": bind_device_id,
            "jkt": bind_jkt
        })),
    )
    .await?;
    assert_eq!(lookup_one.status, 200, "{}", lookup_one.text);
    assert_eq!(
        lookup_one
            .body
            .as_ref()
            .and_then(|body| body.get("found"))
            .and_then(Value::as_bool),
        Some(true)
    );
    let last_seen_first = lookup_one
        .body
        .as_ref()
        .and_then(|body| body.get("device"))
        .and_then(|device| device.get("last_seen_at"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("lookup response missing device.last_seen_at"))?;
    sleep(Duration::from_secs(1)).await;

    let lookup_two = send_kc_signed(
        client,
        env,
        Method::POST,
        "/kc/v1/devices/lookup",
        Some(json!({
            "device_id": bind_device_id,
            "jkt": bind_jkt
        })),
    )
    .await?;
    assert_eq!(lookup_two.status, 200, "{}", lookup_two.text);
    let last_seen_second = lookup_two
        .body
        .as_ref()
        .and_then(|body| body.get("device"))
        .and_then(|device| device.get("last_seen_at"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("second lookup response missing device.last_seen_at"))?;
    let parsed_first = chrono::DateTime::parse_from_rfc3339(last_seen_first)
        .map_err(|error| anyhow!("invalid first last_seen_at timestamp: {error}"))?;
    let parsed_second = chrono::DateTime::parse_from_rfc3339(last_seen_second)
        .map_err(|error| anyhow!("invalid second last_seen_at timestamp: {error}"))?;
    assert!(parsed_second >= parsed_first);

    let conflict_same_device = send_kc_signed(
        client,
        env,
        Method::POST,
        "/kc/v1/enrollments/bind",
        Some(json!({
            "realm": realm,
            "client_id": "resource-server",
            "user_id": user_b.clone(),
            "device_id": bind_device_id,
            "jkt": bind_jkt,
            "public_jwk": public_jwk.clone()
        })),
    )
    .await?;
    assert_eq!(
        conflict_same_device.status, 409,
        "{}",
        conflict_same_device.text
    );

    let conflict_same_jkt = send_kc_signed(
        client,
        env,
        Method::POST,
        "/kc/v1/enrollments/bind",
        Some(json!({
            "realm": realm,
            "client_id": "resource-server",
            "user_id": user_b.clone(),
            "device_id": format!("{}_other", bind_device_id),
            "jkt": bind_jkt,
            "public_jwk": public_jwk.clone()
        })),
    )
    .await?;
    assert_eq!(conflict_same_jkt.status, 409, "{}", conflict_same_jkt.text);

    let race_device = format!("dvc_e2e_race_{}", chrono::Utc::now().timestamp_millis());
    let race_jkt = format!("jkt_e2e_race_{}", chrono::Utc::now().timestamp_millis());
    let race_a = send_kc_signed(
        client,
        env,
        Method::POST,
        "/kc/v1/enrollments/bind",
        Some(json!({
            "realm": realm,
            "client_id": "resource-server",
            "user_id": user_a.clone(),
            "device_id": race_device,
            "jkt": race_jkt,
            "public_jwk": public_jwk.clone()
        })),
    );
    let race_b = send_kc_signed(
        client,
        env,
        Method::POST,
        "/kc/v1/enrollments/bind",
        Some(json!({
            "realm": realm,
            "client_id": "resource-server",
            "user_id": user_b.clone(),
            "device_id": race_device,
            "jkt": race_jkt,
            "public_jwk": public_jwk.clone()
        })),
    );
    let (race_a_resp, race_b_resp) = tokio::join!(race_a, race_b);
    let race_a_resp = race_a_resp?;
    let race_b_resp = race_b_resp?;
    let mut statuses = vec![race_a_resp.status, race_b_resp.status];
    statuses.sort_unstable();
    assert_eq!(
        statuses,
        vec![200, 409],
        "expected race result to be one success and one conflict"
    );

    let delete_existing = send_kc_signed(
        client,
        env,
        Method::DELETE,
        &format!("/kc/v1/users/{}", user_id),
        None,
    )
    .await?;
    assert_eq!(delete_existing.status, 204, "{}", delete_existing.text);

    let delete_missing = send_kc_signed(
        client,
        env,
        Method::DELETE,
        "/kc/v1/users/missing-kc-user",
        None,
    )
    .await?;
    assert_eq!(delete_missing.status, 404, "{}", delete_missing.text);

    Ok(())
}

async fn scenario_staff_deposit_approve_idempotency(
    client: &reqwest::Client,
    env: &Env,
) -> Result<()> {
    let (token, subject) = get_client_token_and_subject(client, env).await?;
    ensure_bff_fixtures(&env.database_url, &subject).await?;

    let bff_base = format!("{}/bff", env.user_storage_url);
    let staff_base = format!("{}/staff", env.user_storage_url);

    let reset = send_json(
        client,
        Method::POST,
        &format!("{}/__admin/reset", env.cuss_url),
        None,
        Some(json!({})),
    )
    .await?;
    assert_eq!(reset.status, 200, "{}", reset.text);

    let instance_id = create_confirm_and_approve_deposit_instance(
        client,
        &env.database_url,
        &bff_base,
        &staff_base,
        &token,
        &subject,
    )
    .await?;
    wait_for_completed_deposit_instance(
        client,
        &staff_base,
        &token,
        &instance_id,
        Duration::from_secs(45),
    )
    .await?;

    let approve_calls_before =
        count_cuss_endpoint_requests(client, &env.cuss_url, "approve").await?;
    assert!(
        approve_calls_before >= 1,
        "expected at least one approve-and-deposit call before idempotency probe"
    );

    let approve_again = send_json(
        client,
        Method::POST,
        &format!("{}/api/kyc/deposits/{}/approve", staff_base, instance_id),
        Some(&token),
        Some(json!({
            "firstName": "Retry",
            "lastName": "Flow",
            "depositAmount": 3600
        })),
    )
    .await?;
    assert_eq!(approve_again.status, 409, "{}", approve_again.text);
    assert_eq!(
        approve_again
            .body
            .as_ref()
            .and_then(|body| body.get("error_key"))
            .and_then(Value::as_str),
        Some("DEPOSIT_ALREADY_APPROVED")
    );

    sleep(Duration::from_secs(2)).await;
    let approve_calls_after =
        count_cuss_endpoint_requests(client, &env.cuss_url, "approve").await?;
    assert_eq!(
        approve_calls_after, approve_calls_before,
        "repeated approve must not produce an additional approve-and-deposit call"
    );

    Ok(())
}

async fn scenario_worker_cuss_failures_and_manual_retries(
    client: &reqwest::Client,
    env: &Env,
) -> Result<()> {
    let (token, subject) = get_client_token_and_subject(client, env).await?;
    ensure_bff_fixtures(&env.database_url, &subject).await?;

    let bff_base = format!("{}/bff", env.user_storage_url);
    let staff_base = format!("{}/staff", env.user_storage_url);

    let reset = send_json(
        client,
        Method::POST,
        &format!("{}/__admin/reset", env.cuss_url),
        None,
        Some(json!({})),
    )
    .await?;
    assert_eq!(reset.status, 200, "{}", reset.text);

    let register_fault = send_json(
        client,
        Method::POST,
        &format!("{}/__admin/faults", env.cuss_url),
        None,
        Some(json!({
            "endpoint": "register",
            "status": 500,
            "body": { "error": "register failure" },
            "count": 1
        })),
    )
    .await?;
    assert_eq!(register_fault.status, 200, "{}", register_fault.text);

    let register_failed_instance = create_confirm_and_approve_deposit_instance(
        client,
        &env.database_url,
        &bff_base,
        &staff_base,
        &token,
        &subject,
    )
    .await?;
    wait_for_step_status(
        client,
        &staff_base,
        &token,
        &register_failed_instance,
        "REGISTER_CUSTOMER",
        "FAILED",
        Duration::from_secs(30),
    )
    .await?;

    let retry_register = send_json(
        client,
        Method::POST,
        &format!(
            "{}/api/kyc/instances/{}/retry",
            staff_base, register_failed_instance
        ),
        Some(&token),
        Some(json!({
            "stepName": "REGISTER_CUSTOMER",
            "mode": "NEW_ATTEMPT"
        })),
    )
    .await?;
    assert_eq!(retry_register.status, 200, "{}", retry_register.text);

    wait_for_completed_deposit_instance(
        client,
        &staff_base,
        &token,
        &register_failed_instance,
        Duration::from_secs(45),
    )
    .await?;

    let approve_fault = send_json(
        client,
        Method::POST,
        &format!("{}/__admin/faults", env.cuss_url),
        None,
        Some(json!({
            "endpoint": "approve",
            "status": 500,
            "body": { "error": "approve failure" },
            "count": 1
        })),
    )
    .await?;
    assert_eq!(approve_fault.status, 200, "{}", approve_fault.text);

    let approve_failed_instance = create_confirm_and_approve_deposit_instance(
        client,
        &env.database_url,
        &bff_base,
        &staff_base,
        &token,
        &subject,
    )
    .await?;
    wait_for_step_status(
        client,
        &staff_base,
        &token,
        &approve_failed_instance,
        "APPROVE_AND_DEPOSIT",
        "FAILED",
        Duration::from_secs(30),
    )
    .await?;

    let retry_approve = send_json(
        client,
        Method::POST,
        &format!(
            "{}/api/kyc/instances/{}/retry",
            staff_base, approve_failed_instance
        ),
        Some(&token),
        Some(json!({
            "stepName": "APPROVE_AND_DEPOSIT",
            "mode": "NEW_ATTEMPT"
        })),
    )
    .await?;
    assert_eq!(retry_approve.status, 200, "{}", retry_approve.text);

    wait_for_completed_deposit_instance(
        client,
        &staff_base,
        &token,
        &approve_failed_instance,
        Duration::from_secs(45),
    )
    .await?;

    Ok(())
}

async fn scenario_error_mapping_representative(client: &reqwest::Client, env: &Env) -> Result<()> {
    let (token, subject) = get_client_token_and_subject(client, env).await?;
    ensure_bff_fixtures(&env.database_url, &subject).await?;

    let bff_base = format!("{}/bff", env.user_storage_url);
    let staff_base = format!("{}/staff", env.user_storage_url);

    let bad_step_id = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/phone/otp/issue", bff_base),
        Some(&token),
        Some(json!({
            "stepId": "invalid-step-id",
            "msisdn": "+237690000088"
        })),
    )
    .await?;
    assert_eq!(bad_step_id.status, 400, "{}", bad_step_id.text);
    assert_eq!(
        bad_step_id
            .body
            .as_ref()
            .and_then(|body| body.get("error_key"))
            .and_then(Value::as_str),
        Some("INVALID_STEP_ID")
    );

    let missing_deposit = send_json(
        client,
        Method::GET,
        &format!("{}/internal/deposits/missing-deposit", bff_base),
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(missing_deposit.status, 404, "{}", missing_deposit.text);
    assert_eq!(
        missing_deposit
            .body
            .as_ref()
            .and_then(|body| body.get("error_key"))
            .and_then(Value::as_str),
        Some("DEPOSIT_NOT_FOUND")
    );

    let retry_missing = send_json(
        client,
        Method::POST,
        &format!("{}/api/kyc/instances/missing-instance/retry", staff_base),
        Some(&token),
        Some(json!({
            "stepName": "REGISTER_CUSTOMER",
            "mode": "NEW_ATTEMPT"
        })),
    )
    .await?;
    assert_eq!(retry_missing.status, 404, "{}", retry_missing.text);

    let session = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/sessions", bff_base),
        Some(&token),
        Some(json!({ "userId": subject })),
    )
    .await?;
    assert_eq!(session.status, 201, "{}", session.text);
    let session_id = require_json_field(&session.body, "id")?
        .as_str()
        .ok_or_else(|| anyhow!("session id must be a string"))?;

    let internal_step_type = send_json(
        client,
        Method::GET,
        &format!(
            "{}/internal/kyc/steps/{}__UNSUPPORTED_TYPE",
            bff_base, session_id
        ),
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(
        internal_step_type.status, 500,
        "{}",
        internal_step_type.text
    );
    assert_eq!(
        internal_step_type
            .body
            .as_ref()
            .and_then(|body| body.get("error_key"))
            .and_then(Value::as_str),
        Some("INVALID_STEP_TYPE")
    );

    let staff_retry_validation = send_json(
        client,
        Method::POST,
        &format!("{}/api/kyc/instances/{}/retry", staff_base, session_id),
        Some(&token),
        Some(json!({
            "stepName": "",
            "mode": "NEW_ATTEMPT"
        })),
    )
    .await?;
    assert_eq!(
        staff_retry_validation.status, 400,
        "{}",
        staff_retry_validation.text
    );
    assert_eq!(
        staff_retry_validation
            .body
            .as_ref()
            .and_then(|body| body.get("error_key"))
            .and_then(Value::as_str),
        Some("INVALID_STEP")
    );

    Ok(())
}

#[derive(Clone, Copy)]
struct KcSignatureOptions<'a> {
    include_timestamp: bool,
    include_signature: bool,
    timestamp: i64,
    sign_method: &'a str,
    sign_path: &'a str,
    sign_body: &'a str,
    signature_override: Option<&'a str>,
}

async fn send_kc_signed(
    client: &reqwest::Client,
    env: &Env,
    method: Method,
    request_path: &str,
    body: Option<Value>,
) -> Result<JsonResponse> {
    let body_str = match body {
        Some(body) => Some(
            serde_json::to_string(&body)
                .map_err(|error| anyhow!("failed to serialize kc request body: {error}"))?,
        ),
        None => None,
    };
    let body_for_signature = body_str.as_deref().unwrap_or_default().to_owned();

    send_kc_custom(
        client,
        env,
        method.clone(),
        request_path,
        body_str,
        KcSignatureOptions {
            include_timestamp: true,
            include_signature: true,
            timestamp: chrono::Utc::now().timestamp(),
            sign_method: method.as_str(),
            sign_path: request_path,
            sign_body: &body_for_signature,
            signature_override: None,
        },
    )
    .await
}

async fn send_kc_custom(
    client: &reqwest::Client,
    env: &Env,
    method: Method,
    request_path: &str,
    body_str: Option<String>,
    options: KcSignatureOptions<'_>,
) -> Result<JsonResponse> {
    let timestamp_str = options.timestamp.to_string();
    let signature = options
        .signature_override
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            kc_signature(
                "some-very-long-secret",
                &timestamp_str,
                options.sign_method,
                options.sign_path,
                options.sign_body,
            )
        });

    let mut request = client.request(method, format!("{}{}", env.user_storage_url, request_path));
    if options.include_timestamp {
        request = request.header("x-kc-timestamp", &timestamp_str);
    }
    if options.include_signature {
        request = request.header("x-kc-signature", &signature);
    }
    if let Some(body_str) = body_str {
        request = request
            .header("content-type", "application/json")
            .body(body_str);
    }

    let response = request.send().await.map_err(|error| {
        anyhow!("kc request failed for {request_path}: {error}; debug={error:?}")
    })?;
    let status = response.status().as_u16();
    let text = response
        .text()
        .await
        .map_err(|error| anyhow!("failed reading kc response body: {error}"))?;
    let body = if text.is_empty() {
        None
    } else {
        serde_json::from_str::<Value>(&text).ok()
    };

    Ok(JsonResponse { status, body, text })
}

fn kc_signature(secret: &str, timestamp: &str, method: &str, path: &str, body: &str) -> String {
    let canonical_payload = format!(
        "{}\n{}\n{}\n{}",
        timestamp,
        method.to_uppercase(),
        path,
        body
    );
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("valid hmac key");
    mac.update(canonical_payload.as_bytes());
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

fn response_message(response: &JsonResponse) -> Option<String> {
    response
        .body
        .as_ref()
        .and_then(|body| body.get("message"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

async fn create_kc_user(client: &reqwest::Client, env: &Env, suffix: &str) -> Result<String> {
    let username = format!("{}-{}", suffix, chrono::Utc::now().timestamp_micros());
    let create = send_kc_signed(
        client,
        env,
        Method::POST,
        "/kc/v1/users",
        Some(json!({
            "realm": "e2e-testing",
            "username": username,
            "enabled": true
        })),
    )
    .await?;
    assert_eq!(create.status, 201, "{}", create.text);
    require_json_field(&create.body, "user_id")?
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow!("create user response missing user_id"))
}

fn expected_device_record_id(device_id: &str, public_jwk: &Value) -> Result<String> {
    let map = public_jwk
        .as_object()
        .ok_or_else(|| anyhow!("public_jwk must be a JSON object"))?;
    let mut sorted = std::collections::BTreeMap::<String, Value>::new();
    for (key, value) in map {
        sorted.insert(key.clone(), value.clone());
    }

    let jwk_serialized = serde_json::to_string(&sorted)
        .map_err(|error| anyhow!("failed to serialize sorted public_jwk: {error}"))?;
    let mut hasher = Sha256::new();
    hasher.update(jwk_serialized.as_bytes());
    let digest = hasher.finalize();
    Ok(format!("{device_id}:{:x}", digest))
}

async fn create_confirm_and_approve_deposit_instance(
    client: &reqwest::Client,
    database_url: &str,
    bff_base: &str,
    staff_base: &str,
    token: &str,
    subject: &str,
) -> Result<String> {
    clear_first_deposit_instances(database_url, subject).await?;

    let deposit_response = send_json(
        client,
        Method::POST,
        &format!("{}/internal/deposits/phone", bff_base),
        Some(token),
        Some(json!({
            "userId": subject,
            "amount": 3600,
            "currency": "XAF",
            "provider": "MTN_CM",
            "reason": "worker-failure-flow"
        })),
    )
    .await?;
    assert_eq!(deposit_response.status, 201, "{}", deposit_response.text);
    let instance_id = require_json_field(&deposit_response.body, "depositId")?
        .as_str()
        .ok_or_else(|| anyhow!("depositId must be a string"))?
        .to_owned();

    let confirm = send_json(
        client,
        Method::POST,
        &format!(
            "{}/api/kyc/deposits/{}/confirm-payment",
            staff_base, instance_id
        ),
        Some(token),
        Some(json!({
            "note": "payment-confirmed",
            "providerTxnId": format!("txn-{}", chrono::Utc::now().timestamp_millis())
        })),
    )
    .await?;
    assert_eq!(confirm.status, 200, "{}", confirm.text);

    let approve = send_json(
        client,
        Method::POST,
        &format!("{}/api/kyc/deposits/{}/approve", staff_base, instance_id),
        Some(token),
        Some(json!({
            "firstName": "Retry",
            "lastName": "Flow",
            "depositAmount": 3600
        })),
    )
    .await?;
    assert_eq!(approve.status, 200, "{}", approve.text);

    Ok(instance_id)
}

async fn clear_first_deposit_instances(database_url: &str, user_id: &str) -> Result<()> {
    let (db_client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("postgres connection error while clearing instances: {error}");
        }
    });

    db_client
        .execute(
            "DELETE FROM sm_instance WHERE user_id = $1 AND kind = 'KYC_FIRST_DEPOSIT'",
            &[&user_id],
        )
        .await?;

    Ok(())
}

async fn wait_for_step_status(
    client: &reqwest::Client,
    staff_base: &str,
    token: &str,
    instance_id: &str,
    step_name: &str,
    expected_status: &str,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let detail = send_json(
            client,
            Method::GET,
            &format!("{}/api/kyc/instances/{}", staff_base, instance_id),
            Some(token),
            None,
        )
        .await?;
        if detail.status == 200
            && latest_attempt_status(&detail.body, step_name).as_deref() == Some(expected_status)
        {
            return Ok(());
        }
        sleep(Duration::from_millis(500)).await;
    }

    Err(anyhow!(
        "timed out waiting for step {} to reach status {} for instance {}",
        step_name,
        expected_status,
        instance_id
    ))
}

async fn wait_for_completed_deposit_instance(
    client: &reqwest::Client,
    staff_base: &str,
    token: &str,
    instance_id: &str,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let mut last_detail: Option<Value> = None;
    while Instant::now() < deadline {
        let detail = send_json(
            client,
            Method::GET,
            &format!("{}/api/kyc/instances/{}", staff_base, instance_id),
            Some(token),
            None,
        )
        .await?;
        if detail.status == 200 {
            last_detail = detail.body.clone();
            let overall = detail
                .body
                .as_ref()
                .and_then(|body| body.get("status"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let register = latest_attempt_status(&detail.body, "REGISTER_CUSTOMER");
            let approve = latest_attempt_status(&detail.body, "APPROVE_AND_DEPOSIT");
            if overall == "COMPLETED"
                && register.as_deref() == Some("SUCCEEDED")
                && approve.as_deref() == Some("SUCCEEDED")
            {
                return Ok(());
            }
        }
        sleep(Duration::from_millis(500)).await;
    }

    let last_detail_text = last_detail
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| "<none>".to_owned());
    Err(anyhow!(
        "timed out waiting for deposit instance {} to complete; last detail: {}",
        instance_id,
        last_detail_text
    ))
}

async fn count_cuss_endpoint_requests(
    client: &reqwest::Client,
    cuss_url: &str,
    endpoint_name: &str,
) -> Result<usize> {
    let recorded = send_json(
        client,
        Method::GET,
        &format!("{}/__admin/requests", cuss_url),
        None,
        None,
    )
    .await?;
    assert_eq!(recorded.status, 200, "{}", recorded.text);

    let count = recorded
        .body
        .as_ref()
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| {
                    item.get("endpoint")
                        .and_then(Value::as_str)
                        .map(|endpoint| endpoint == endpoint_name)
                        .unwrap_or(false)
                })
                .count()
        })
        .unwrap_or(0);

    Ok(count)
}
