mod common;

use anyhow::{Result, anyhow};
use common::{
    Env, ensure_bff_fixtures, get_client_token_and_subject, http_client, require_json_field,
    reset_sms_sink, send_json, wait_for_otp,
};
use reqwest::Method;
use serde_json::{Value, json};
use std::time::Duration;

#[tokio::test]
async fn full_auth_enforcement_on_bff_and_staff() -> Result<()> {
    let env = Env::from_env()?;
    let client = http_client()?;
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

    let (token, subject) = get_client_token_and_subject(&client, &env).await?;
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

#[tokio::test]
async fn full_bff_deposit_and_otp_flow() -> Result<()> {
    let env = Env::from_env()?;
    let client = http_client()?;

    let (token, subject) = get_client_token_and_subject(&client, &env).await?;
    ensure_bff_fixtures(&env.database_url, &subject).await?;
    reset_sms_sink(&client, &env).await?;

    let bff_base = format!("{}/bff", env.user_storage_url);

    let deposit_response = send_json(
        &client,
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
        &client,
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
        &client,
        Method::POST,
        &format!("{}/internal/kyc/sessions", bff_base),
        Some(&token),
        Some(json!({"userId": require_json_field(&lookup.body, "userId")? })),
    )
    .await?;
    assert_eq!(session.status, 201, "{}", session.text);
    let session_id = require_json_field(&session.body, "id")?
        .as_str()
        .ok_or_else(|| anyhow!("session id must be a string"))?;

    let step = send_json(
        &client,
        Method::POST,
        &format!("{}/internal/kyc/steps", bff_base),
        Some(&token),
        Some(json!({
            "sessionId": session_id,
            "userId": require_json_field(&lookup.body, "userId")?,
            "type": "PHONE",
            "policy": {}
        })),
    )
    .await?;
    assert_eq!(step.status, 201, "{}", step.text);
    let step_id = require_json_field(&step.body, "id")?
        .as_str()
        .ok_or_else(|| anyhow!("step id must be a string"))?;

    let msisdn = "+237690000033";
    let issue = send_json(
        &client,
        Method::POST,
        &format!("{}/internal/kyc/phone/otp/issue", bff_base),
        Some(&token),
        Some(json!({
            "stepId": step_id,
            "msisdn": msisdn,
            "channel": "SMS",
            "ttlSeconds": 120
        })),
    )
    .await?;
    assert_eq!(issue.status, 200, "{}", issue.text);
    let otp_ref = require_json_field(&issue.body, "otpRef")?
        .as_str()
        .ok_or_else(|| anyhow!("otpRef must be a string"))?;

    let otp = wait_for_otp(&client, &env, msisdn, Duration::from_secs(30)).await?;

    let verify = send_json(
        &client,
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

    Ok(())
}

#[tokio::test]
async fn full_staff_summary_and_instances() -> Result<()> {
    let env = Env::from_env()?;
    let client = http_client()?;

    let (token, subject) = get_client_token_and_subject(&client, &env).await?;
    ensure_bff_fixtures(&env.database_url, &subject).await?;

    let staff_base = format!("{}/staff", env.user_storage_url);

    let summary = send_json(
        &client,
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
    let by_status = summary
        .body
        .as_ref()
        .and_then(|body| body.get("byStatus"))
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("summary.byStatus should be an object"))?;
    let _ = by_kind;
    let _ = by_status;

    let instances = send_json(
        &client,
        Method::GET,
        &format!("{}/api/kyc/instances", staff_base),
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(instances.status, 200, "{}", instances.text);
    assert!(
        instances
            .body
            .as_ref()
            .and_then(|body| body.get("items"))
            .and_then(Value::as_array)
            .is_some()
    );

    let missing = send_json(
        &client,
        Method::GET,
        &format!("{}/api/kyc/instances/missing-instance", staff_base),
        Some(&token),
        None,
    )
    .await?;
    assert_eq!(missing.status, 404, "{}", missing.text);

    Ok(())
}

#[tokio::test]
async fn full_cuss_stub_records_register_calls() -> Result<()> {
    let env = Env::from_env()?;
    let client = http_client()?;

    let reset = send_json(
        &client,
        Method::POST,
        &format!("{}/__admin/reset", env.cuss_url),
        None,
        Some(json!({})),
    )
    .await?;
    assert_eq!(reset.status, 200, "{}", reset.text);

    let register = send_json(
        &client,
        Method::POST,
        &format!("{}/api/registration/register", env.cuss_url),
        None,
        Some(json!({
            "firstName": "E2E",
            "lastName": "Runner",
            "phone": "+237690000044",
            "externalId": format!("cuss-{}", chrono::Utc::now().timestamp_millis())
        })),
    )
    .await?;
    assert_eq!(register.status, 201, "{}", register.text);

    let recorded = send_json(
        &client,
        Method::GET,
        &format!("{}/__admin/requests", env.cuss_url),
        None,
        None,
    )
    .await?;
    assert_eq!(recorded.status, 200, "{}", recorded.text);

    let has_register = recorded
        .body
        .as_ref()
        .and_then(Value::as_array)
        .map(|items| {
            items.iter().any(|item| {
                item.get("endpoint")
                    .and_then(Value::as_str)
                    .map(|value| value == "register")
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    assert!(has_register, "cuss requests should include register entry");

    Ok(())
}
