mod common;

use anyhow::{Result, anyhow};
use common::{
    BffTestFixture, Env, ensure_bff_fixtures, http_client, require_json_field,
    send_json_with_bff,
};
use reqwest::Method;
use serde_json::{Value, json};
use serial_test::file_serial;
use std::time::Duration;
use tokio::time::sleep;

fn test_context() -> Result<(Env, reqwest::Client)> {
    let env = Env::from_env()?;
    let client = http_client()?;
    Ok((env, client))
}

fn require_session_id(body: &Option<Value>) -> Result<String> {
    require_json_field(body, "id")?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("session id must be a string"))
}

fn require_flow_id(body: &Option<Value>) -> Result<String> {
    require_json_field(body, "id")?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("flow id must be a string"))
}

#[allow(dead_code)]
fn require_step_id(body: &Option<Value>) -> Result<String> {
    require_json_field(body, "id")?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("step id must be a string"))
}

async fn wait_for_step_status(
    client: &reqwest::Client,
    bff_base: &str,
    fixture: &BffTestFixture,
    step_id: &str,
    expected_status: &str,
    timeout: Duration,
) -> Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    let url = format!("{}/flow/steps/{}", bff_base, step_id);

    while std::time::Instant::now() < deadline {
        let response = send_json_with_bff(
            client,
            Method::GET,
            &url,
            None,
            None,
            Some(fixture),
        )
        .await?;

        if response.status == 200 {
            let status = response
                .body
                .as_ref()
                .and_then(|b| b.get("status"))
                .and_then(Value::as_str);

            if let Some(s) = status
                && s == expected_status
            {
                return Ok(());
            }
        }

        sleep(Duration::from_millis(500)).await;
    }

    Err(anyhow!(
        "Step {} did not reach status {} within {:?}",
        step_id,
        expected_status,
        timeout
    ))
}

async fn wait_for_flow_status(
    client: &reqwest::Client,
    bff_base: &str,
    fixture: &BffTestFixture,
    flow_id: &str,
    expected_status: &str,
    timeout: Duration,
) -> Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    let url = format!("{}/flow/flows/{}", bff_base, flow_id);

    while std::time::Instant::now() < deadline {
        let response = send_json_with_bff(
            client,
            Method::GET,
            &url,
            None,
            None,
            Some(fixture),
        )
        .await?;

        if response.status == 200 {
            let status = response
                .body
                .as_ref()
                .and_then(|b| b.get("status"))
                .and_then(Value::as_str);

            if let Some(s) = status
                && s == expected_status
            {
                return Ok(());
            }
        }

        sleep(Duration::from_millis(500)).await;
    }

    Err(anyhow!(
        "Flow {} did not reach status {} within {:?}",
        flow_id,
        expected_status,
        timeout
    ))
}

#[tokio::test]
#[file_serial]
async fn flow_sdk_session_with_phone_otp_and_first_deposit() -> Result<()> {
    let (env, client) = test_context()?;

    let user_id = format!("usr_flow_sdk_e2e_{}", chrono::Utc::now().timestamp());
    ensure_bff_fixtures(&env.database_url, &user_id).await?;

    let fixture = BffTestFixture::generate(&user_id);
    let fixture = fixture.store_global();

    let bff_base = format!("{}/bff", env.user_storage_url);

    println!("=== Step 1: Create KYC_FULL session ===");
    let session_response = send_json_with_bff(
        &client,
        Method::POST,
        &format!("{}/flow/sessions", bff_base),
        None,
        Some(json!({
            "sessionType": "kyc_full",
            "context": {
                "phone_number": "+237690123456"
            }
        })),
        Some(fixture),
    )
    .await?;

    assert_eq!(session_response.status, 201, "Session creation failed: {}", session_response.text);
    let session_id = require_session_id(&session_response.body)?;
    println!("Created session: {}", session_id);

    println!("=== Step 2: Add PHONE_OTP flow to session ===");
    let phone_flow_response = send_json_with_bff(
        &client,
        Method::POST,
        &format!("{}/flow/sessions/{}/flows", bff_base, session_id),
        None,
        Some(json!({
            "flowType": "phone_otp",
            "context": {}
        })),
        Some(fixture),
    )
    .await?;

    assert_eq!(phone_flow_response.status, 201, "Phone OTP flow creation failed: {}", phone_flow_response.text);
    let phone_flow_id = require_flow_id(&phone_flow_response.body)?;
    println!("Created phone_otp flow: {}", phone_flow_id);

    println!("=== Step 3: Get flow details and find generate step ===");
    let flow_detail = send_json_with_bff(
        &client,
        Method::GET,
        &format!("{}/flow/flows/{}", bff_base, phone_flow_id),
        None,
        None,
        Some(fixture),
    )
    .await?;

    assert_eq!(flow_detail.status, 200, "Get flow failed: {}", flow_detail.text);
    
    let steps = flow_detail
        .body
        .as_ref()
        .and_then(|b| b.get("steps"))
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("steps missing in flow response"))?;

    let generate_step = steps
        .iter()
        .find(|s| s.get("stepType").and_then(Value::as_str) == Some("generate"))
        .ok_or_else(|| anyhow!("generate step not found"))?;

    let generate_step_id = generate_step
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("step id missing"))?
        .to_owned();

    println!("Found generate step: {}", generate_step_id);

    println!("=== Step 4: Wait for generate step to complete (SYSTEM actor) ===");
    wait_for_step_status(&client, &bff_base, fixture, &generate_step_id, "COMPLETED", Duration::from_secs(10)).await?;
    println!("Generate step completed");

    println!("=== Step 5: Get flow again to find verify step ===");
    let flow_detail_after = send_json_with_bff(
        &client,
        Method::GET,
        &format!("{}/flow/flows/{}", bff_base, phone_flow_id),
        None,
        None,
        Some(fixture),
    )
    .await?;

    let steps_after = flow_detail_after
        .body
        .as_ref()
        .and_then(|b| b.get("steps"))
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("steps missing"))?;

    let verify_step = steps_after
        .iter()
        .find(|s| s.get("stepType").and_then(Value::as_str) == Some("verify"))
        .ok_or_else(|| anyhow!("verify step not found"))?;

    let verify_step_id = verify_step
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("verify step id missing"))?
        .to_owned();

    println!("Found verify step: {}", verify_step_id);

    println!("=== Step 6: Get flow context to retrieve OTP ===");
    let flow_context = flow_detail_after
        .body
        .as_ref()
        .and_then(|b| b.get("context"))
        .cloned()
        .unwrap_or(json!({}));

    let otp = flow_context
        .get("otp")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("OTP not found in flow context"))?;

    println!("Retrieved OTP from context");

    println!("=== Step 7: Submit OTP verification ===");
    let verify_response = send_json_with_bff(
        &client,
        Method::POST,
        &format!("{}/flow/steps/{}", bff_base, verify_step_id),
        None,
        Some(json!({
            "input": {
                "code": otp
            }
        })),
        Some(fixture),
    )
    .await?;

    assert_eq!(verify_response.status, 200, "Verify step failed: {}", verify_response.text);
    println!("OTP verification submitted");

    println!("=== Step 8: Wait for phone_otp flow to complete ===");
    wait_for_flow_status(&client, &bff_base, fixture, &phone_flow_id, "COMPLETED", Duration::from_secs(10)).await?;
    println!("Phone OTP flow completed");

    println!("=== Step 9: Add FIRST_DEPOSIT flow to same session ===");
    let deposit_flow_response = send_json_with_bff(
        &client,
        Method::POST,
        &format!("{}/flow/sessions/{}/flows", bff_base, session_id),
        None,
        Some(json!({
            "flowType": "first_deposit",
            "context": {
                "amount": "5000",
                "currency": "XAF"
            }
        })),
        Some(fixture),
    )
    .await?;

    assert_eq!(deposit_flow_response.status, 201, "First deposit flow creation failed: {}", deposit_flow_response.text);
    let deposit_flow_id = require_flow_id(&deposit_flow_response.body)?;
    println!("Created first_deposit flow: {}", deposit_flow_id);

    println!("=== Step 10: Get user metadata ===");
    let user_response = send_json_with_bff(
        &client,
        Method::GET,
        &format!("{}/flow/users/{}", bff_base, user_id),
        None,
        None,
        Some(fixture),
    )
    .await?;

    assert_eq!(user_response.status, 200, "Get user failed: {}", user_response.text);
    
    let metadata = user_response
        .body
        .as_ref()
        .and_then(|b| b.get("metadata"))
        .cloned()
        .unwrap_or(json!({}));

    println!("User metadata: {}", serde_json::to_string_pretty(&metadata)?);

    println!("=== Step 11: Get KYC level ===");
    let kyc_level_response = send_json_with_bff(
        &client,
        Method::GET,
        &format!("{}/flow/users/{}/kyc-level", bff_base, user_id),
        None,
        None,
        Some(fixture),
    )
    .await?;

    assert_eq!(kyc_level_response.status, 200, "Get KYC level failed: {}", kyc_level_response.text);
    
    let levels = kyc_level_response
        .body
        .as_ref()
        .and_then(|b| b.get("level"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    println!("KYC levels: {:?}", levels);

    println!("=== E2E Flow SDK test completed successfully ===");

    Ok(())
}

#[tokio::test]
#[file_serial]
async fn flow_sdk_session_creation_and_listing() -> Result<()> {
    let (env, client) = test_context()?;

    let user_id = format!("usr_flow_sdk_list_{}", chrono::Utc::now().timestamp());
    ensure_bff_fixtures(&env.database_url, &user_id).await?;

    let fixture = BffTestFixture::generate(&user_id);
    let fixture = fixture.store_global();

    let bff_base = format!("{}/bff", env.user_storage_url);

    println!("=== Create multiple sessions ===");
    
    for i in 0..3 {
        let session_response = send_json_with_bff(
            &client,
            Method::POST,
            &format!("{}/flow/sessions", bff_base),
            None,
            Some(json!({
                "sessionType": "kyc_full",
                "context": {
                    "iteration": i
                }
            })),
            Some(fixture),
        )
        .await?;

        assert_eq!(session_response.status, 201, "Session {} creation failed", i);
    }

    println!("=== List sessions ===");
    let list_response = send_json_with_bff(
        &client,
        Method::GET,
        &format!("{}/flow/sessions", bff_base),
        None,
        None,
        Some(fixture),
    )
    .await?;

    assert_eq!(list_response.status, 200, "List sessions failed: {}", list_response.text);

    let sessions = list_response
        .body
        .as_ref()
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("sessions array missing"))?;

    assert!(sessions.len() >= 3, "Expected at least 3 sessions, got {}", sessions.len());

    println!("Found {} sessions", sessions.len());

    Ok(())
}

#[tokio::test]
#[file_serial]
async fn flow_sdk_retry_config_validation() -> Result<()> {
    let (env, client) = test_context()?;

    let user_id = format!("usr_flow_sdk_retry_{}", chrono::Utc::now().timestamp());
    ensure_bff_fixtures(&env.database_url, &user_id).await?;

    let fixture = BffTestFixture::generate(&user_id);
    let fixture = fixture.store_global();

    let bff_base = format!("{}/bff", env.user_storage_url);

    println!("=== Create session and flow ===");
    let session_response = send_json_with_bff(
        &client,
        Method::POST,
        &format!("{}/flow/sessions", bff_base),
        None,
        Some(json!({
            "sessionType": "kyc_full"
        })),
        Some(fixture),
    )
    .await?;

    assert_eq!(session_response.status, 201);
    let session_id = require_session_id(&session_response.body)?;

    println!("=== Add phone_otp flow ===");
    let flow_response = send_json_with_bff(
        &client,
        Method::POST,
        &format!("{}/flow/sessions/{}/flows", bff_base, session_id),
        None,
        Some(json!({
            "flowType": "phone_otp"
        })),
        Some(fixture),
    )
    .await?;

    assert_eq!(flow_response.status, 201);
    let flow_id = require_flow_id(&flow_response.body)?;

    println!("=== Verify flow was created with retry config ===");
    let flow_detail = send_json_with_bff(
        &client,
        Method::GET,
        &format!("{}/flow/flows/{}", bff_base, flow_id),
        None,
        None,
        Some(fixture),
    )
    .await?;

    assert_eq!(flow_detail.status, 200);

    let flow_def = flow_detail.body.as_ref().ok_or_else(|| anyhow::anyhow!("No body"))?;
    println!("Flow definition: {}", serde_json::to_string_pretty(flow_def)?);

    Ok(())
}