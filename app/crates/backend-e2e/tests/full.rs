mod common;

use anyhow::{Result, anyhow};
use common::{
    Env, create_foreign_deposit_fixture, ensure_bff_fixtures, get_client_token_and_subject,
    http_client, require_json_field, reset_sms_sink, send_json, wait_for_otp,
};
use reqwest::Method;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio::time::sleep;

#[tokio::test]
async fn full_suite() -> Result<()> {
    let env = Env::from_env()?;
    let client = http_client()?;

    scenario_auth_enforcement(&client, &env).await?;
    scenario_bff_deposit_and_otp_flow(&client, &env).await?;
    scenario_bff_session_resume_and_otp_limits(&client, &env).await?;
    scenario_bff_deposit_denies_non_owner(&client, &env).await?;
    scenario_staff_instance_detail_and_retry(&client, &env).await?;
    scenario_staff_summary_and_instances(&client, &env).await?;
    scenario_staff_deposit_flow_triggers_worker_and_cuss(&client, &env).await?;

    Ok(())
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
