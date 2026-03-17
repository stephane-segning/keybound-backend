mod world;

pub use world::*;

use anyhow::Result;
use cucumber::{given, then, when, World};
use reqwest::Method;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Debug, Default)]
pub struct StoredSession {
    session_id: String,
    step_id: String,
    otp_ref: String,
    otp_code: String,
}

#[derive(Debug, Default, World)]
#[world(init = Self::new)]
pub struct FullE2eWorld {
    pub env: Option<Env>,
    pub client: Option<reqwest::Client>,
    pub token: Option<String>,
    pub subject: Option<String>,
    pub last_response: Option<JsonResponse>,
    pub error: Option<String>,
    pub stored_session: Option<StoredSession>,
}

impl FullE2eWorld {
    pub async fn new() -> Result<Self, anyhow::Error> {
        let env = Env::from_env()?;
        let client = http_client()?;
        Ok(Self {
            env: Some(env),
            client: Some(client),
            ..Default::default()
        })
    }

    pub fn env(&self) -> Result<&Env, anyhow::Error> {
        self.env.as_ref().ok_or_else(|| anyhow::anyhow!("env not initialized"))
    }

    pub fn client(&self) -> Result<&reqwest::Client, anyhow::Error> {
        self.client.as_ref().ok_or_else(|| anyhow::anyhow!("client not initialized"))
    }

    pub fn bff_base(&self) -> Result<String, anyhow::Error> {
        Ok(format!("{}/bff", self.env()?.user_storage_url))
    }

    pub fn staff_base(&self) -> Result<String, anyhow::Error> {
        Ok(format!("{}/staff", self.env()?.user_storage_url))
    }
}

fn require_level_values(body: &Option<Value>) -> Result<Vec<String>> {
    let level = require_json_field(body, "level")?
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("level must be array"))?;

    level
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| anyhow::anyhow!("level entries must be strings"))
        })
        .collect()
}

#[given("the e2e test environment is initialized")]
async fn init_environment(world: &mut FullE2eWorld) {
    match FullE2eWorld::new().await {
        Ok(w) => {
            world.env = w.env;
            world.client = w.client;
        }
        Err(e) => {
            world.error = Some(e.to_string());
        }
    }
}

#[given("I have a valid authentication token")]
async fn get_auth_token(world: &mut FullE2eWorld) {
    let env = match world.env.as_ref() {
        Some(e) => e,
        None => {
            world.error = Some("env not initialized".to_string());
            return;
        }
    };
    let client = match world.client.as_ref() {
        Some(c) => c,
        None => {
            world.error = Some("client not initialized".to_string());
            return;
        }
    };

    match get_client_token_and_subject(client, env).await {
        Ok((token, subject)) => {
            world.token = Some(token);
            world.subject = Some(subject);
        }
        Err(e) => {
            world.error = Some(e.to_string());
        }
    }
}

#[given("the database fixtures are set up")]
async fn setup_fixtures(world: &mut FullE2eWorld) {
    let env = match world.env.as_ref() {
        Some(e) => e,
        None => {
            world.error = Some("env not initialized".to_string());
            return;
        }
    };
    let subject = match world.subject.as_ref() {
        Some(s) => s,
        None => {
            world.error = Some("subject not initialized".to_string());
            return;
        }
    };

    match ensure_bff_fixtures(&env.database_url, subject).await {
        Ok(()) => {}
        Err(e) => {
            world.error = Some(e.to_string());
        }
    }
}

#[given("the SMS sink is reset")]
async fn given_reset_sms(world: &mut FullE2eWorld) {
    let env = match world.env.as_ref() {
        Some(e) => e,
        None => {
            world.error = Some("env not initialized".to_string());
            return;
        }
    };
    let client = match world.client.as_ref() {
        Some(c) => c,
        None => {
            world.error = Some("client not initialized".to_string());
            return;
        }
    };

    match reset_sms_sink(client, env).await {
        Ok(()) => {}
        Err(e) => {
            world.error = Some(e.to_string());
        }
    }
}

#[when(regex = r"^I send a (\w+) request to ([^\s]+) without authentication$")]
async fn send_request_no_auth(world: &mut FullE2eWorld, method: String, path: String) {
    let env = match world.env.as_ref() {
        Some(e) => e,
        None => {
            world.error = Some("env not initialized".to_string());
            return;
        }
    };
    let client = match world.client.as_ref() {
        Some(c) => c,
        None => {
            world.error = Some("client not initialized".to_string());
            return;
        }
    };

    let url = format!("{}{}", env.user_storage_url, path);
    let http_method = match method.to_uppercase().as_str() {
        "GET" => Method::GET,
        "POST" => Method::POST,
        "PUT" => Method::PUT,
        "DELETE" => Method::DELETE,
        _ => Method::GET,
    };

    let result = match http_method {
        Method::GET => client.get(&url).send().await,
        Method::POST => client.post(&url).json(&json!({})).send().await,
        _ => client.get(&url).send().await,
    };

    match result {
        Ok(response) => {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            let body = if text.is_empty() { None } else { serde_json::from_str::<Value>(&text).ok() };
            world.last_response = Some(JsonResponse { status, body, text });
        }
        Err(e) => {
            world.error = Some(e.to_string());
        }
    }
}

#[when(regex = r"^I send a (\w+) request to ([^\s]+) with an invalid Bearer token$")]
async fn send_request_invalid_bearer(world: &mut FullE2eWorld, method: String, path: String) {
    let env = match world.env.as_ref() {
        Some(e) => e,
        None => {
            world.error = Some("env not initialized".to_string());
            return;
        }
    };
    let client = match world.client.as_ref() {
        Some(c) => c,
        None => {
            world.error = Some("client not initialized".to_string());
            return;
        }
    };

    let url = format!("{}{}", env.user_storage_url, path);
    let http_method = match method.to_uppercase().as_str() {
        "GET" => Method::GET,
        "POST" => Method::POST,
        _ => Method::GET,
    };

    let result = match http_method {
        Method::GET => client.get(&url).header("Authorization", "Bearer definitely-invalid-token").send().await,
        Method::POST => client.post(&url).header("Authorization", "Bearer definitely-invalid-token").json(&json!({})).send().await,
        _ => client.get(&url).send().await,
    };

    match result {
        Ok(response) => {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            let body = if text.is_empty() { None } else { serde_json::from_str::<Value>(&text).ok() };
            world.last_response = Some(JsonResponse { status, body, text });
        }
        Err(e) => {
            world.error = Some(e.to_string());
        }
    }
}

#[when(regex = r"^I send a (\w+) request to ([^\s]+) with Basic auth$")]
async fn send_request_basic_auth(world: &mut FullE2eWorld, _method: String, path: String) {
    let env = match world.env.as_ref() {
        Some(e) => e,
        None => {
            world.error = Some("env not initialized".to_string());
            return;
        }
    };
    let client = match world.client.as_ref() {
        Some(c) => c,
        None => {
            world.error = Some("client not initialized".to_string());
            return;
        }
    };

    let url = format!("{}{}", env.user_storage_url, path);
    
    let result = client
        .post(&url)
        .header("Authorization", "Basic dGVzdDp0ZXN0")
        .json(&json!({}))
        .send()
        .await;

    match result {
        Ok(response) => {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            let body = if text.is_empty() { None } else { serde_json::from_str::<Value>(&text).ok() };
            world.last_response = Some(JsonResponse { status, body, text });
        }
        Err(e) => {
            world.error = Some(e.to_string());
        }
    }
}

#[when(regex = r"^I send a (\w+) request to ([^\s]+) with valid authentication$")]
async fn send_request_valid_auth(world: &mut FullE2eWorld, _method: String, path: String) {
    let bff_base = match world.bff_base() {
        Ok(b) => b,
        Err(e) => {
            world.error = Some(e.to_string());
            return;
        }
    };
    let client = match world.client.as_ref() {
        Some(c) => c,
        None => {
            world.error = Some("client not initialized".to_string());
            return;
        }
    };
    let token = match world.token.as_ref() {
        Some(t) => t,
        None => {
            world.error = Some("token not initialized".to_string());
            return;
        }
    };
    let subject = match world.subject.as_ref() {
        Some(s) => s,
        None => {
            world.error = Some("subject not initialized".to_string());
            return;
        }
    };

    let url = format!("{}{}", bff_base, path);
    
    let result = send_json(
        client,
        Method::POST,
        &url,
        Some(token),
        Some(json!({ "userId": subject, "flow": "PHONE_OTP" })),
    )
    .await;

    match result {
        Ok(response) => {
            world.last_response = Some(response);
        }
        Err(e) => {
            world.error = Some(e.to_string());
        }
    }
}

#[then(regex = r"^the response status is (\d+)$")]
async fn response_status_is(world: &mut FullE2eWorld, expected: u16) {
    let response = world.last_response.as_ref().expect("No response recorded");
    assert_eq!(response.status, expected, "Expected status {} but got {}: {}", expected, response.status, response.text);
}

#[then(regex = r"^the response status is not (\d+)$")]
async fn response_status_is_not(world: &mut FullE2eWorld, unexpected: u16) {
    let response = world.last_response.as_ref().expect("No response recorded");
    assert_ne!(response.status, unexpected, "Response should not have status {}: {}", unexpected, response.text);
}

#[when("I create a PHONE_OTP session")]
async fn create_phone_otp_session(world: &mut FullE2eWorld) {
    let bff_base = match world.bff_base() {
        Ok(b) => b,
        Err(e) => {
            world.error = Some(e.to_string());
            return;
        }
    };
    let client = match world.client.as_ref() {
        Some(c) => c,
        None => {
            world.error = Some("client not initialized".to_string());
            return;
        }
    };
    let token = match world.token.as_ref() {
        Some(t) => t,
        None => {
            world.error = Some("token not initialized".to_string());
            return;
        }
    };
    let subject = match world.subject.as_ref() {
        Some(s) => s,
        None => {
            world.error = Some("subject not initialized".to_string());
            return;
        }
    };

    let result = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/sessions", bff_base),
        Some(token),
        Some(json!({ "userId": subject, "flow": "PHONE_OTP" })),
    )
    .await;

    match result {
        Ok(response) => {
            if let Some(session_id) = response.body.as_ref()
                .and_then(|b| b.get("id"))
                .and_then(Value::as_str)
            {
                world.stored_session = Some(StoredSession {
                    session_id: session_id.to_string(),
                    ..Default::default()
                });
            }
            world.last_response = Some(response);
        }
        Err(e) => {
            world.error = Some(e.to_string());
        }
    }
}

#[when("I create a phone OTP step")]
async fn create_phone_otp_step(world: &mut FullE2eWorld) {
    let bff_base = match world.bff_base() {
        Ok(b) => b,
        Err(e) => {
            world.error = Some(e.to_string());
            return;
        }
    };
    let client = match world.client.as_ref() {
        Some(c) => c,
        None => {
            world.error = Some("client not initialized".to_string());
            return;
        }
    };
    let token = match world.token.as_ref() {
        Some(t) => t,
        None => {
            world.error = Some("token not initialized".to_string());
            return;
        }
    };

    let session_id = match world.stored_session.as_ref() {
        Some(s) => &s.session_id,
        None => {
            world.error = Some("no stored session".to_string());
            return;
        }
    };

    let subject = match world.subject.as_ref() {
        Some(s) => s,
        None => {
            world.error = Some("subject not initialized".to_string());
            return;
        }
    };

    let result = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/phone-otp/steps", bff_base),
        Some(token),
        Some(json!({
            "sessionId": session_id,
            "userId": subject,
            "policy": {}
        })),
    )
    .await;

    match result {
        Ok(response) => {
            if let Some(step_id) = response.body.as_ref()
                .and_then(|b| b.get("id"))
                .and_then(Value::as_str)
                && let Some(ref mut session) = world.stored_session
            {
                session.step_id = step_id.to_string();
            }
            world.last_response = Some(response);
        }
        Err(e) => {
            world.error = Some(e.to_string());
        }
    }
}

#[when(regex = r"^I issue an OTP to phone number ([^\s]+)$")]
async fn issue_otp_to_phone(world: &mut FullE2eWorld, msisdn: String) {
    let bff_base = match world.bff_base() {
        Ok(b) => b,
        Err(e) => {
            world.error = Some(e.to_string());
            return;
        }
    };
    let client = match world.client.as_ref() {
        Some(c) => c,
        None => {
            world.error = Some("client not initialized".to_string());
            return;
        }
    };
    let token = match world.token.as_ref() {
        Some(t) => t,
        None => {
            world.error = Some("token not initialized".to_string());
            return;
        }
    };

    let session = match world.stored_session.as_ref() {
        Some(s) => s,
        None => {
            world.error = Some("no stored session".to_string());
            return;
        }
    };

    let result = send_json(
        client,
        Method::POST,
        &format!("{}/internal/kyc/phone-otp/challenges", bff_base),
        Some(token),
        Some(json!({
            "sessionId": session.session_id,
            "stepId": session.step_id,
            "msisdn": msisdn,
            "channel": "SMS",
            "ttlSeconds": 120
        })),
    )
    .await;

    match result {
        Ok(response) => {
            if let Some(otp_ref) = response.body.as_ref()
                .and_then(|b| b.get("otpRef"))
                .and_then(Value::as_str)
                && let Some(ref mut stored) = world.stored_session
            {
                stored.otp_ref = otp_ref.to_string();
            }
            world.last_response = Some(response);
        }
        Err(e) => {
            world.error = Some(e.to_string());
        }
    }
}

#[then(regex = r"^I receive an OTP within (\d+) seconds$")]
async fn receive_otp(world: &mut FullE2eWorld, timeout_secs: u64) {
    let env = world.env.as_ref().expect("env not initialized");
    let client = world.client.as_ref().expect("client not initialized");
    
    let phone = "+237690000033";
    
    match wait_for_otp(client, env, phone, Duration::from_secs(timeout_secs)).await {
        Ok(otp) => {
            if let Some(ref mut stored) = world.stored_session {
                stored.otp_code = otp;
            }
        }
        Err(e) => {
            world.error = Some(e.to_string());
        }
    }
}

#[then("the step status is NOT_STARTED")]
async fn step_status_not_started(world: &mut FullE2eWorld) {
    let response = world.last_response.as_ref().expect("No response recorded");
    let status = response.body.as_ref()
        .and_then(|b| b.get("status"))
        .and_then(Value::as_str);
    assert_eq!(status, Some("NOT_STARTED"), "Step status should be NOT_STARTED");
}

#[then("the step status is IN_PROGRESS")]
async fn step_status_in_progress(world: &mut FullE2eWorld) {
    let response = world.last_response.as_ref().expect("No response recorded");
    let status = response.body.as_ref()
        .and_then(|b| b.get("status"))
        .and_then(Value::as_str);
    assert_eq!(status, Some("IN_PROGRESS"), "Step status should be IN_PROGRESS");
}

#[then("the step status is VERIFIED")]
async fn step_status_verified(world: &mut FullE2eWorld) {
    let response = world.last_response.as_ref().expect("No response recorded");
    let status = response.body.as_ref()
        .and_then(|b| b.get("status"))
        .and_then(Value::as_str);
    assert_eq!(status, Some("VERIFIED"), "Step status should be VERIFIED");
}

#[then(regex = "^the KYC level is \"([^\"]+)\"$")]
async fn kyc_level_is(world: &mut FullE2eWorld, expected: String) {
    let response = world.last_response.as_ref().expect("No response recorded");
    let levels = require_level_values(&response.body).expect("level should be array");
    assert_eq!(levels, vec![expected], "KYC level mismatch");
}

#[then("phoneOtpVerified is false")]
async fn phone_otp_verified_false(world: &mut FullE2eWorld) {
    let response = world.last_response.as_ref().expect("No response recorded");
    let verified = response.body.as_ref()
        .and_then(|b| b.get("phoneOtpVerified"))
        .and_then(Value::as_bool);
    assert_eq!(verified, Some(false), "phoneOtpVerified should be false");
}

#[then("phoneOtpVerified is true")]
async fn phone_otp_verified_true(world: &mut FullE2eWorld) {
    let response = world.last_response.as_ref().expect("No response recorded");
    let verified = response.body.as_ref()
        .and_then(|b| b.get("phoneOtpVerified"))
        .and_then(Value::as_bool);
    assert_eq!(verified, Some(true), "phoneOtpVerified should be true");
}

#[then("firstDepositVerified is false")]
async fn first_deposit_verified_false(world: &mut FullE2eWorld) {
    let response = world.last_response.as_ref().expect("No response recorded");
    let verified = response.body.as_ref()
        .and_then(|b| b.get("firstDepositVerified"))
        .and_then(Value::as_bool);
    assert_eq!(verified, Some(false), "firstDepositVerified should be false");
}

#[then("firstDepositVerified is true")]
async fn first_deposit_verified_true(world: &mut FullE2eWorld) {
    let response = world.last_response.as_ref().expect("No response recorded");
    let verified = response.body.as_ref()
        .and_then(|b| b.get("firstDepositVerified"))
        .and_then(Value::as_bool);
    assert_eq!(verified, Some(true), "firstDepositVerified should be true");
}

#[then(regex = "^the KYC level contains \"([^\"]+)\"$")]
async fn kyc_level_contains(world: &mut FullE2eWorld, expected: String) {
    let response = world.last_response.as_ref().expect("No response recorded");
    let levels = require_level_values(&response.body).expect("level should be array");
    assert!(levels.contains(&expected), "KYC level should contain {}", expected);
}

#[when("I get the KYC level")]
async fn get_kyc_level(world: &mut FullE2eWorld) {
    let bff_base = world.bff_base().expect("bff_base");
    let client = world.client.as_ref().expect("client");
    let token = world.token.as_ref().expect("token");
    let subject = world.subject.as_ref().expect("subject");

    let result = send_json(
        client,
        Method::GET,
        &format!("{}/internal/users/{}/kyc-level", bff_base, subject),
        Some(token),
        None,
    )
    .await;

    match result {
        Ok(response) => {
            world.last_response = Some(response);
        }
        Err(e) => {
            world.error = Some(e.to_string());
        }
    }
}

#[when("I get the KYC summary")]
async fn get_kyc_summary(world: &mut FullE2eWorld) {
    let bff_base = world.bff_base().expect("bff_base");
    let client = world.client.as_ref().expect("client");
    let token = world.token.as_ref().expect("token");
    let subject = world.subject.as_ref().expect("subject");

    let result = send_json(
        client,
        Method::GET,
        &format!("{}/internal/users/{}/kyc-summary", bff_base, subject),
        Some(token),
        None,
    )
    .await;

    match result {
        Ok(response) => {
            world.last_response = Some(response);
        }
        Err(e) => {
            world.error = Some(e.to_string());
        }
    }
}

#[when("I get the current user")]
async fn get_current_user(world: &mut FullE2eWorld) {
    let bff_base = world.bff_base().expect("bff_base");
    let client = world.client.as_ref().expect("client");
    let token = world.token.as_ref().expect("token");
    let subject = world.subject.as_ref().expect("subject");

    let result = send_json(
        client,
        Method::GET,
        &format!("{}/internal/users/{}", bff_base, subject),
        Some(token),
        None,
    )
    .await;

    match result {
        Ok(response) => {
            world.last_response = Some(response);
        }
        Err(e) => {
            world.error = Some(e.to_string());
        }
    }
}

#[then("the response contains the correct user ID")]
async fn response_contains_correct_user_id(world: &mut FullE2eWorld) {
    let response = world.last_response.as_ref().expect("No response recorded");
    let subject = world.subject.as_ref().expect("subject should be set");
    let user_id = response.body.as_ref()
        .and_then(|b| b.get("userId"))
        .and_then(Value::as_str);
    assert_eq!(user_id, Some(subject.as_str()), "userId should match subject");
}

#[then(regex = "^phoneOtpStatus is \"([^\"]+)\"$")]
async fn phone_otp_status_is(world: &mut FullE2eWorld, expected: String) {
    let response = world.last_response.as_ref().expect("No response recorded");
    let status = response.body.as_ref()
        .and_then(|b| b.get("phoneOtpStatus"))
        .and_then(Value::as_str);
    assert_eq!(status, Some(expected.as_str()), "phoneOtpStatus mismatch");
}

#[then("no error occurred")]
async fn no_error(world: &mut FullE2eWorld) {
    assert!(world.error.is_none(), "Unexpected error: {:?}", world.error);
}

#[tokio::main]
async fn main() {
    FullE2eWorld::run("tests/features").await;
}