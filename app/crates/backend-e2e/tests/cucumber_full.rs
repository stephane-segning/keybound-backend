mod world;

pub use world::*;

use anyhow::{Result, anyhow};
use cucumber::{World, given, then, when};
use reqwest::Method;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const FIXTURE_FULL_NAME: &str = "E2E Subject";
const FIXTURE_PHONE_NUMBER: &str = "+237690123456";

#[derive(Debug, Default)]
pub struct FlowState {
    pub session_id: Option<String>,
    pub phone_flow_id: Option<String>,
    pub phone_verify_step_id: Option<String>,
    pub deposit_flow_id: Option<String>,
    pub admin_step_id: Option<String>,
    pub deposit_amount: Option<i64>,
    pub phone_number: Option<String>,
    pub otp_code: Option<String>,
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
    pub flow: FlowState,
}

impl FullE2eWorld {
    pub async fn new() -> Result<Self, anyhow::Error> {
        Ok(Self {
            env: Some(Env::from_env()?),
            client: Some(http_client()?),
            ..Default::default()
        })
    }

    fn env(&self) -> Result<&Env> {
        self.env
            .as_ref()
            .ok_or_else(|| anyhow!("env not initialized"))
    }

    fn client(&self) -> Result<&reqwest::Client> {
        self.client
            .as_ref()
            .ok_or_else(|| anyhow!("client not initialized"))
    }

    fn subject(&self) -> Result<&str> {
        self.subject
            .as_deref()
            .ok_or_else(|| anyhow!("subject not initialized"))
    }

    fn token(&self) -> Result<&str> {
        self.token
            .as_deref()
            .ok_or_else(|| anyhow!("token not initialized"))
    }

    fn bff_base(&self) -> Result<String> {
        Ok(format!("{}/bff", self.env()?.user_storage_url))
    }

    fn staff_base(&self) -> Result<String> {
        Ok(format!("{}/staff", self.env()?.user_storage_url))
    }
}

fn require_response_field<'a>(body: &'a Option<Value>, field: &str) -> Result<&'a Value> {
    body.as_ref()
        .and_then(|json| json.get(field))
        .ok_or_else(|| anyhow!("response body missing field `{field}`"))
}

fn require_id(body: &Option<Value>, field: &str) -> Result<String> {
    require_response_field(body, field)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("{field} must be a string"))
}

async fn reset_cuss(client: &reqwest::Client, env: &Env) -> Result<()> {
    let response = send_json(
        client,
        Method::POST,
        &format!("{}/__admin/reset", env.cuss_url),
        None,
        Some(json!({})),
    )
    .await?;

    if response.status != 200 {
        return Err(anyhow!(
            "cuss reset failed ({}): {}",
            response.status,
            response.text
        ));
    }

    Ok(())
}

async fn list_flow_steps(world: &FullE2eWorld, flow_id: &str) -> Result<Vec<Value>> {
    let response = send_json(
        world.client()?,
        Method::GET,
        &format!("{}/flow/flows/{}", world.bff_base()?, flow_id),
        Some(world.token()?),
        None,
    )
    .await?;

    if response.status != 200 {
        return Err(anyhow!(
            "get flow failed ({}): {}",
            response.status,
            response.text
        ));
    }

    response
        .body
        .as_ref()
        .and_then(|body| body.get("steps"))
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| anyhow!("steps missing from flow response"))
}

fn find_step_id(steps: &[Value], step_type: &str) -> Result<String> {
    steps
        .iter()
        .find(|step| step.get("stepType").and_then(Value::as_str) == Some(step_type))
        .and_then(|step| step.get("id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("step `{step_type}` not found"))
}

async fn wait_for_flow_status(
    world: &FullE2eWorld,
    flow_id: &str,
    expected_status: &str,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let url = format!("{}/flow/flows/{}", world.bff_base()?, flow_id);
    let mut last_status: Option<String> = None;

    while Instant::now() < deadline {
        let response = send_json(
            world.client()?,
            Method::GET,
            &url,
            Some(world.token()?),
            None,
        )
        .await?;

        if response.status == 200 {
            let status = response.body.as_ref().and_then(|body| {
                body.get("status")
                    .or_else(|| body.get("flow").and_then(|flow| flow.get("status")))
            });

            if let Some(status) = status.and_then(Value::as_str) {
                last_status = Some(status.to_owned());
                if status == expected_status {
                    return Ok(());
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    Err(anyhow!(
        "flow {flow_id} did not reach status {expected_status} within {:?} (last_status={})",
        timeout,
        last_status.unwrap_or_else(|| "unknown".to_owned())
    ))
}

async fn wait_for_session_close_reason(
    world: &FullE2eWorld,
    session_id: &str,
    expected_status: &str,
    expected_reason: &str,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let url = format!("{}/flow/sessions/{}", world.bff_base()?, session_id);

    while Instant::now() < deadline {
        let response = send_json(
            world.client()?,
            Method::GET,
            &url,
            Some(world.token()?),
            None,
        )
        .await?;

        if response.status == 200
            && let Some(session) = response.body.as_ref().and_then(|body| body.get("session"))
        {
            let status = session.get("status").and_then(Value::as_str);
            let reason = session
                .pointer("/context/close_reason")
                .and_then(Value::as_str);
            if status == Some(expected_status) && reason == Some(expected_reason) {
                return Ok(());
            }
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    Err(anyhow!(
        "session {session_id} did not reach {expected_status}/{expected_reason}"
    ))
}

async fn find_waiting_admin_step(world: &FullE2eWorld, user_id: &str) -> Result<String> {
    let response = send_json(
        world.client()?,
        Method::GET,
        &format!(
            "{}/flow/steps?status=WAITING&userId={}&flowType=first_deposit",
            world.staff_base()?,
            user_id
        ),
        Some(world.token()?),
        None,
    )
    .await?;

    if response.status != 200 {
        return Err(anyhow!(
            "list admin steps failed ({}): {}",
            response.status,
            response.text
        ));
    }

    let steps = response
        .body
        .as_ref()
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    find_step_id(&steps, "await_admin_decision")
}

async fn cuss_requests(world: &FullE2eWorld) -> Result<Vec<Value>> {
    let response = send_json(
        world.client()?,
        Method::GET,
        &format!("{}/__admin/requests", world.env()?.cuss_url),
        None,
        None,
    )
    .await?;

    if response.status != 200 {
        return Err(anyhow!(
            "cuss requests failed ({}): {}",
            response.status,
            response.text
        ));
    }

    Ok(response
        .body
        .as_ref()
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

async fn get_staff_flow_detail(world: &FullE2eWorld, flow_id: &str) -> Result<Value> {
    let response = send_json(
        world.client()?,
        Method::GET,
        &format!("{}/flow/flows/{}", world.staff_base()?, flow_id),
        Some(world.token()?),
        None,
    )
    .await?;

    if response.status != 200 {
        return Err(anyhow!(
            "get staff flow failed ({}): {}",
            response.status,
            response.text
        ));
    }

    response
        .body
        .ok_or_else(|| anyhow!("staff flow response body missing"))
}

async fn get_staff_step_detail(world: &FullE2eWorld, step_id: &str) -> Result<Value> {
    let response = send_json(
        world.client()?,
        Method::GET,
        &format!("{}/flow/steps/{}", world.staff_base()?, step_id),
        Some(world.token()?),
        None,
    )
    .await?;

    if response.status != 200 {
        return Err(anyhow!(
            "get staff step failed ({}): {}",
            response.status,
            response.text
        ));
    }

    response
        .body
        .ok_or_else(|| anyhow!("staff step response body missing"))
}

fn find_step<'a>(steps: &'a [Value], step_type: &str) -> Result<&'a Value> {
    steps
        .iter()
        .find(|step| step.get("stepType").and_then(Value::as_str) == Some(step_type))
        .ok_or_else(|| anyhow!("step `{step_type}` not found"))
}

#[given("the e2e test environment is initialized")]
async fn init_environment(world: &mut FullE2eWorld) {
    match FullE2eWorld::new().await {
        Ok(next) => {
            world.env = next.env;
            world.client = next.client;
        }
        Err(error) => world.error = Some(error.to_string()),
    }
}

#[given("I have a valid authentication token")]
async fn get_auth_token(world: &mut FullE2eWorld) {
    match get_client_token_and_subject(world.client().unwrap(), world.env().unwrap()).await {
        Ok((token, subject)) => {
            world.token = Some(token);
            world.subject = Some(subject);
        }
        Err(error) => world.error = Some(error.to_string()),
    }
}

#[given("the database fixtures are set up")]
async fn setup_fixtures(world: &mut FullE2eWorld) {
    let subject = match world.subject() {
        Ok(value) => value.to_owned(),
        Err(error) => {
            world.error = Some(error.to_string());
            return;
        }
    };

    if let Err(error) = ensure_bff_fixtures(&world.env().unwrap().database_url, &subject).await {
        world.error = Some(error.to_string());
        return;
    }
}

#[given("the SMS sink is reset")]
async fn given_reset_sms(world: &mut FullE2eWorld) {
    if let Err(error) = reset_sms_sink(world.client().unwrap(), world.env().unwrap()).await {
        world.error = Some(error.to_string());
    }
}

#[given("the CUSS sink is reset")]
async fn given_reset_cuss(world: &mut FullE2eWorld) {
    if let Err(error) = reset_cuss(world.client().unwrap(), world.env().unwrap()).await {
        world.error = Some(error.to_string());
    }
}

#[when(regex = r"^I send a (\w+) request to ([^\s]+) without authentication$")]
async fn send_request_no_auth(world: &mut FullE2eWorld, method: String, path: String) {
    let url = format!("{}{}", world.env().unwrap().user_storage_url, path);
    let result = match method.to_uppercase().as_str() {
        "POST" => {
            world
                .client()
                .unwrap()
                .post(url)
                .json(&json!({}))
                .send()
                .await
        }
        _ => world.client().unwrap().get(url).send().await,
    };

    match result {
        Ok(response) => {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            world.last_response = Some(JsonResponse {
                status,
                body: serde_json::from_str(&text).ok(),
                text,
            });
        }
        Err(error) => world.error = Some(error.to_string()),
    }
}

#[when(regex = r"^I send a (\w+) request to ([^\s]+) with an invalid Bearer token$")]
async fn send_request_invalid_bearer(world: &mut FullE2eWorld, method: String, path: String) {
    let url = format!("{}{}", world.env().unwrap().user_storage_url, path);
    let result = match method.to_uppercase().as_str() {
        "POST" => {
            world
                .client()
                .unwrap()
                .post(url)
                .header("Authorization", "Bearer definitely-invalid-token")
                .json(&json!({}))
                .send()
                .await
        }
        _ => {
            world
                .client()
                .unwrap()
                .get(url)
                .header("Authorization", "Bearer definitely-invalid-token")
                .send()
                .await
        }
    };

    match result {
        Ok(response) => {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            world.last_response = Some(JsonResponse {
                status,
                body: serde_json::from_str(&text).ok(),
                text,
            });
        }
        Err(error) => world.error = Some(error.to_string()),
    }
}

#[when(regex = r"^I send a (\w+) request to ([^\s]+) with Basic auth$")]
async fn send_request_basic_auth(world: &mut FullE2eWorld, method: String, path: String) {
    let url = format!("{}{}", world.env().unwrap().user_storage_url, path);
    let result = match method.to_uppercase().as_str() {
        "POST" => {
            world
                .client()
                .unwrap()
                .post(url)
                .header("Authorization", "Basic dGVzdDp0ZXN0")
                .json(&json!({}))
                .send()
                .await
        }
        _ => {
            world
                .client()
                .unwrap()
                .get(url)
                .header("Authorization", "Basic dGVzdDp0ZXN0")
                .send()
                .await
        }
    };

    match result {
        Ok(response) => {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            world.last_response = Some(JsonResponse {
                status,
                body: serde_json::from_str(&text).ok(),
                text,
            });
        }
        Err(error) => world.error = Some(error.to_string()),
    }
}

#[when(regex = r"^I send a (\w+) request to ([^\s]+) with valid authentication$")]
async fn send_request_valid_auth(world: &mut FullE2eWorld, method: String, path: String) {
    let url = format!("{}{}", world.env().unwrap().user_storage_url, path);
    let result = send_json(
        world.client().unwrap(),
        match method.to_uppercase().as_str() {
            "POST" => Method::POST,
            _ => Method::GET,
        },
        &url,
        Some(world.token().unwrap()),
        Some(json!({ "sessionType": "kyc_full" })),
    )
    .await;

    match result {
        Ok(response) => world.last_response = Some(response),
        Err(error) => world.error = Some(error.to_string()),
    }
}

#[given("I complete phone OTP verification")]
async fn complete_phone_otp(world: &mut FullE2eWorld) {
    let subject = world.subject().unwrap().to_owned();
    let bff_base = world.bff_base().unwrap();
    let phone_number = "+237690000033".to_owned();

    let session = send_json(
        world.client().unwrap(),
        Method::POST,
        &format!("{}/flow/sessions", bff_base),
        Some(world.token().unwrap()),
        Some(json!({ "sessionType": "kyc_full" })),
    )
    .await;

    let session = match session {
        Ok(value) => value,
        Err(error) => {
            world.error = Some(error.to_string());
            return;
        }
    };

    if session.status != 201 {
        world.error = Some(format!(
            "create phone_otp session failed ({}): {}",
            session.status, session.text
        ));
        return;
    }

    let session_id = match require_id(&session.body, "id") {
        Ok(value) => value,
        Err(error) => {
            world.error = Some(format!(
                "create phone_otp session missing id: {} | body={}",
                error, session.text
            ));
            return;
        }
    };

    let flow = send_json(
        world.client().unwrap(),
        Method::POST,
        &format!("{}/flow/sessions/{}/flows", bff_base, session_id),
        Some(world.token().unwrap()),
        Some(json!({ "flowType": "phone_otp" })),
    )
    .await;

    let flow = match flow {
        Ok(value) => value,
        Err(error) => {
            world.error = Some(error.to_string());
            return;
        }
    };

    if flow.status != 201 {
        world.error = Some(format!(
            "create phone_otp flow failed ({}): {}",
            flow.status, flow.text
        ));
        return;
    }

    let flow_id = match require_id(&flow.body, "id") {
        Ok(value) => value,
        Err(error) => {
            world.error = Some(format!(
                "create phone_otp flow missing id: {} | body={}",
                error, flow.text
            ));
            return;
        }
    };

    let steps = match list_flow_steps(world, &flow_id).await {
        Ok(value) => value,
        Err(error) => {
            world.error = Some(error.to_string());
            return;
        }
    };
    let init_step_id = match find_step_id(&steps, "init_phone") {
        Ok(value) => value,
        Err(error) => {
            world.error = Some(error.to_string());
            return;
        }
    };

    if let Err(error) = send_json(
        world.client().unwrap(),
        Method::POST,
        &format!("{}/flow/steps/{}", bff_base, init_step_id),
        Some(world.token().unwrap()),
        Some(json!({ "input": { "phone_number": phone_number } })),
    )
    .await
    {
        world.error = Some(error.to_string());
        return;
    }

    let otp = match wait_for_otp(
        world.client().unwrap(),
        world.env().unwrap(),
        "+237690000033",
        Duration::from_secs(15),
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            world.error = Some(error.to_string());
            return;
        }
    };

    let verify_steps = match list_flow_steps(world, &flow_id).await {
        Ok(value) => value,
        Err(error) => {
            world.error = Some(error.to_string());
            return;
        }
    };
    let verify_step_id = match find_step_id(&verify_steps, "verify_otp") {
        Ok(value) => value,
        Err(error) => {
            world.error = Some(error.to_string());
            return;
        }
    };

    match send_json(
        world.client().unwrap(),
        Method::POST,
        &format!("{}/flow/steps/{}", bff_base, verify_step_id),
        Some(world.token().unwrap()),
        Some(json!({ "input": { "code": otp } })),
    )
    .await
    {
        Ok(response) => world.last_response = Some(response),
        Err(error) => {
            world.error = Some(error.to_string());
            return;
        }
    }

    if let Err(error) =
        wait_for_flow_status(world, &flow_id, "COMPLETED", Duration::from_secs(10)).await
    {
        world.error = Some(error.to_string());
        return;
    }

    world.flow.session_id = Some(session_id);
    world.flow.phone_flow_id = Some(flow_id);
    world.flow.phone_verify_step_id = Some(verify_step_id);
    world.flow.phone_number = Some(phone_number);
    world.flow.otp_code = Some(otp);
    let _ = subject;
}

#[given("I start a first deposit flow for 5000 XAF")]
async fn start_first_deposit_flow(world: &mut FullE2eWorld) {
    start_first_deposit(world, 5000).await;
}

#[given("I start a first deposit flow for 7000 XAF")]
async fn start_first_deposit_flow_reject(world: &mut FullE2eWorld) {
    start_first_deposit(world, 7000).await;
}

async fn start_first_deposit(world: &mut FullE2eWorld, amount: i64) {
    let bff_base = world.bff_base().unwrap();
    let session_id = if let Some(existing) = world.flow.session_id.clone() {
        existing
    } else {
        match send_json(
            world.client().unwrap(),
            Method::POST,
            &format!("{}/flow/sessions", bff_base),
            Some(world.token().unwrap()),
            Some(json!({ "sessionType": "kyc_full" })),
        )
        .await
        {
            Ok(response) => {
                if response.status != 201 {
                    world.error = Some(format!(
                        "create session failed ({}): {}",
                        response.status, response.text
                    ));
                    return;
                }
                match require_id(&response.body, "id") {
                    Ok(id) => id,
                    Err(error) => {
                        world.error = Some(format!(
                            "create session missing id: {} | body={}",
                            error, response.text
                        ));
                        return;
                    }
                }
            }
            Err(error) => {
                world.error = Some(error.to_string());
                return;
            }
        }
    };

    let flow = match send_json(
        world.client().unwrap(),
        Method::POST,
        &format!("{}/flow/sessions/{}/flows", bff_base, session_id),
        Some(world.token().unwrap()),
        Some(json!({ "flowType": "first_deposit" })),
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            world.error = Some(error.to_string());
            return;
        }
    };
    if flow.status != 201 {
        world.error = Some(format!(
            "create first_deposit flow failed ({}): {}",
            flow.status, flow.text
        ));
        return;
    }

    let flow_id = match require_id(&flow.body, "id") {
        Ok(value) => value,
        Err(error) => {
            world.error = Some(format!(
                "create first_deposit flow missing id: {} | body={}",
                error, flow.text
            ));
            return;
        }
    };

    let steps = match list_flow_steps(world, &flow_id).await {
        Ok(value) => value,
        Err(error) => {
            world.error = Some(error.to_string());
            return;
        }
    };
    let init_step_id = match find_step_id(&steps, "init_first_deposit") {
        Ok(value) => value,
        Err(error) => {
            world.error = Some(error.to_string());
            return;
        }
    };

    match send_json(
        world.client().unwrap(),
        Method::POST,
        &format!("{}/flow/steps/{}", bff_base, init_step_id),
        Some(world.token().unwrap()),
        Some(json!({ "input": { "amount": amount, "currency": "XAF" } })),
    )
    .await
    {
        Ok(response) => world.last_response = Some(response),
        Err(error) => {
            world.error = Some(error.to_string());
            return;
        }
    }

    match find_waiting_admin_step(world, world.subject().unwrap()).await {
        Ok(step_id) => world.flow.admin_step_id = Some(step_id),
        Err(error) => world.error = Some(error.to_string()),
    }

    world.flow.session_id = Some(session_id);
    world.flow.deposit_flow_id = Some(flow_id);
    world.flow.deposit_amount = Some(amount);
}

#[when("I approve the pending first deposit admin step")]
async fn approve_first_deposit(world: &mut FullE2eWorld) {
    let admin_step_id = match world.flow.admin_step_id.clone() {
        Some(value) => value,
        None => {
            world.error = Some("admin step id missing".to_owned());
            return;
        }
    };

    match send_json(
        world.client().unwrap(),
        Method::POST,
        &format!(
            "{}/flow/steps/{}",
            world.staff_base().unwrap(),
            admin_step_id
        ),
        Some(world.token().unwrap()),
        Some(json!({ "input": { "decision": "APPROVED" } })),
    )
    .await
    {
        Ok(response) => world.last_response = Some(response),
        Err(error) => {
            world.error = Some(error.to_string());
            return;
        }
    }

    if let Some(flow_id) = world.flow.deposit_flow_id.clone()
        && let Err(error) =
            wait_for_flow_status(world, &flow_id, "COMPLETED", Duration::from_secs(15)).await
    {
        world.error = Some(error.to_string());
    }
}

#[when("I reject the pending first deposit admin step")]
async fn reject_first_deposit(world: &mut FullE2eWorld) {
    let admin_step_id = match world.flow.admin_step_id.clone() {
        Some(value) => value,
        None => {
            world.error = Some("admin step id missing".to_owned());
            return;
        }
    };

    match send_json(
        world.client().unwrap(),
        Method::POST,
        &format!(
            "{}/flow/steps/{}",
            world.staff_base().unwrap(),
            admin_step_id
        ),
        Some(world.token().unwrap()),
        Some(json!({ "input": { "decision": "REJECTED" } })),
    )
    .await
    {
        Ok(response) => world.last_response = Some(response),
        Err(error) => {
            world.error = Some(error.to_string());
            return;
        }
    }

    if let Some(flow_id) = world.flow.deposit_flow_id.clone()
        && let Err(error) =
            wait_for_flow_status(world, &flow_id, "CLOSED", Duration::from_secs(10)).await
    {
        world.error = Some(error.to_string());
    }
}

#[when("I get the current user")]
async fn get_current_user(world: &mut FullE2eWorld) {
    match send_json(
        world.client().unwrap(),
        Method::GET,
        &format!(
            "{}/flow/users/{}",
            world.bff_base().unwrap(),
            world.subject().unwrap()
        ),
        Some(world.token().unwrap()),
        None,
    )
    .await
    {
        Ok(response) => world.last_response = Some(response),
        Err(error) => world.error = Some(error.to_string()),
    }
}

#[when("I get the KYC level")]
async fn get_kyc_level(world: &mut FullE2eWorld) {
    match send_json(
        world.client().unwrap(),
        Method::GET,
        &format!(
            "{}/flow/users/{}/kyc-level",
            world.bff_base().unwrap(),
            world.subject().unwrap()
        ),
        Some(world.token().unwrap()),
        None,
    )
    .await
    {
        Ok(response) => world.last_response = Some(response),
        Err(error) => world.error = Some(error.to_string()),
    }
}

#[then(regex = r"^the response status is (\d+)$")]
async fn response_status_is(world: &mut FullE2eWorld, expected: u16) {
    let response = world.last_response.as_ref().expect("No response recorded");
    assert_eq!(response.status, expected, "{}", response.text);
}

#[then(regex = r"^the response status is not (\d+)$")]
async fn response_status_is_not(world: &mut FullE2eWorld, unexpected: u16) {
    let response = world.last_response.as_ref().expect("No response recorded");
    assert_ne!(response.status, unexpected, "{}", response.text);
}

#[then("the response contains the correct user ID")]
async fn response_contains_correct_user_id(world: &mut FullE2eWorld) {
    let response = world.last_response.as_ref().expect("No response recorded");
    let subject = world.subject().expect("subject should be set");
    let user_id = response
        .body
        .as_ref()
        .and_then(|body| body.get("userId"))
        .and_then(Value::as_str);
    assert_eq!(user_id, Some(subject));
}

fn require_levels(body: &Option<Value>) -> Vec<String> {
    body.as_ref()
        .and_then(|value| value.get("level"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entry| entry.as_str().map(str::to_owned))
        .collect()
}

#[then(regex = "^the KYC level is \"([^\"]+)\"$")]
async fn kyc_level_is(world: &mut FullE2eWorld, expected: String) {
    assert_eq!(
        require_levels(&world.last_response.as_ref().unwrap().body),
        vec![expected]
    );
}

#[then(regex = "^the KYC level contains \"([^\"]+)\"$")]
async fn kyc_level_contains(world: &mut FullE2eWorld, expected: String) {
    let levels = require_levels(&world.last_response.as_ref().unwrap().body);
    assert!(levels.contains(&expected), "levels={levels:?}");
}

#[then("phoneOtpVerified is false")]
async fn phone_otp_verified_false(world: &mut FullE2eWorld) {
    let value = world
        .last_response
        .as_ref()
        .and_then(|response| response.body.as_ref())
        .and_then(|body| body.get("phoneOtpVerified"))
        .and_then(Value::as_bool);
    assert_eq!(value, Some(false));
}

#[then("phoneOtpVerified is true")]
async fn phone_otp_verified_true(world: &mut FullE2eWorld) {
    let value = world
        .last_response
        .as_ref()
        .and_then(|response| response.body.as_ref())
        .and_then(|body| body.get("phoneOtpVerified"))
        .and_then(Value::as_bool);
    assert_eq!(value, Some(true));
}

#[then("firstDepositVerified is false")]
async fn first_deposit_verified_false(world: &mut FullE2eWorld) {
    let value = world
        .last_response
        .as_ref()
        .and_then(|response| response.body.as_ref())
        .and_then(|body| body.get("firstDepositVerified"))
        .and_then(Value::as_bool);
    assert!(
        value.is_none() || value == Some(false),
        "expected firstDepositVerified to be false or absent, got {value:?}"
    );
}

#[then("firstDepositVerified is true")]
async fn first_deposit_verified_true(world: &mut FullE2eWorld) {
    let value = world
        .last_response
        .as_ref()
        .and_then(|response| response.body.as_ref())
        .and_then(|body| body.get("firstDepositVerified"))
        .and_then(Value::as_bool);
    assert_eq!(value, Some(true));
}

#[then("the first deposit metadata is persisted")]
async fn first_deposit_metadata_persisted(world: &mut FullE2eWorld) {
    let response = send_json(
        world.client().unwrap(),
        Method::GET,
        &format!(
            "{}/flow/users/{}",
            world.bff_base().unwrap(),
            world.subject().unwrap()
        ),
        Some(world.token().unwrap()),
        None,
    )
    .await
    .expect("user request should succeed");

    let metadata = response
        .body
        .as_ref()
        .and_then(|body| body.get("metadata"))
        .cloned()
        .unwrap_or_else(|| json!({}));

    assert_eq!(metadata.pointer("/fineractId"), Some(&json!(1)));
    assert_eq!(metadata.pointer("/savingsAccountId"), Some(&json!(2)));
    assert_eq!(
        metadata.pointer("/firstDeposit/status"),
        Some(&json!("APPROVED"))
    );
    assert_eq!(
        metadata.pointer("/firstDeposit/transactionId"),
        Some(&json!(20))
    );
}

#[then("the first deposit metadata is not persisted")]
async fn first_deposit_metadata_not_persisted(world: &mut FullE2eWorld) {
    let response = send_json(
        world.client().unwrap(),
        Method::GET,
        &format!(
            "{}/flow/users/{}",
            world.bff_base().unwrap(),
            world.subject().unwrap()
        ),
        Some(world.token().unwrap()),
        None,
    )
    .await
    .expect("user request should succeed");

    let metadata = response
        .body
        .as_ref()
        .and_then(|body| body.get("metadata"))
        .cloned()
        .unwrap_or_else(|| json!({}));

    assert_eq!(metadata.pointer("/fineractId"), None);
    assert_eq!(metadata.pointer("/savingsAccountId"), None);
    assert_eq!(metadata.pointer("/firstDeposit/status"), None);
    assert_eq!(metadata.pointer("/firstDeposit/transactionId"), None);
}

#[then("the first deposit flow is waiting for admin review")]
async fn first_deposit_waiting_for_admin(world: &mut FullE2eWorld) {
    let flow_id = world
        .flow
        .deposit_flow_id
        .clone()
        .expect("deposit flow id should be set");
    let admin_step_id = world
        .flow
        .admin_step_id
        .clone()
        .expect("admin step id should be set");

    let staff_step = get_staff_step_detail(world, &admin_step_id)
        .await
        .expect("staff step should load");
    assert_eq!(
        staff_step.get("stepType").and_then(Value::as_str),
        Some("await_admin_decision")
    );
    assert_eq!(
        staff_step.get("actor").and_then(Value::as_str),
        Some("ADMIN")
    );
    assert_eq!(
        staff_step.get("status").and_then(Value::as_str),
        Some("WAITING")
    );

    let flow_detail = get_staff_flow_detail(world, &flow_id)
        .await
        .expect("staff flow should load");
    let steps = flow_detail
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    assert_eq!(
        find_step(&steps, "init_first_deposit")
            .ok()
            .and_then(|step| step.get("status"))
            .and_then(Value::as_str),
        Some("COMPLETED")
    );
    assert_eq!(
        find_step(&steps, "get_user")
            .ok()
            .and_then(|step| step.get("status"))
            .and_then(Value::as_str),
        Some("COMPLETED")
    );
    assert_eq!(
        find_step(&steps, "await_admin_decision")
            .ok()
            .and_then(|step| step.get("status"))
            .and_then(Value::as_str),
        Some("WAITING")
    );
}

#[then("the staff flow detail shows the completed deposit path")]
async fn completed_deposit_path_visible(world: &mut FullE2eWorld) {
    let flow_id = world
        .flow
        .deposit_flow_id
        .clone()
        .expect("deposit flow id should be set");
    let flow_detail = get_staff_flow_detail(world, &flow_id)
        .await
        .expect("staff flow should load");
    let steps = flow_detail
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for step_type in [
        "await_admin_decision",
        "decide_admin",
        "cuss_register_customer",
        "cuss_approve_and_deposit",
        "update_deposit_metadata",
    ] {
        assert_eq!(
            find_step(&steps, step_type)
                .ok()
                .and_then(|step| step.get("status"))
                .and_then(Value::as_str),
            Some("COMPLETED"),
            "step `{step_type}` should be completed"
        );
    }
    assert_ne!(
        find_step(&steps, "close_session")
            .ok()
            .and_then(|step| step.get("status"))
            .and_then(Value::as_str),
        Some("COMPLETED"),
        "close_session should not be completed on approve"
    );
}

#[then("the staff flow detail shows the rejected deposit path")]
async fn rejected_deposit_path_visible(world: &mut FullE2eWorld) {
    let flow_id = world
        .flow
        .deposit_flow_id
        .clone()
        .expect("deposit flow id should be set");
    let flow_detail = get_staff_flow_detail(world, &flow_id)
        .await
        .expect("staff flow should load");
    let steps = flow_detail
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for step_type in ["await_admin_decision", "decide_admin", "close_session"] {
        assert_eq!(
            find_step(&steps, step_type)
                .ok()
                .and_then(|step| step.get("status"))
                .and_then(Value::as_str),
            Some("COMPLETED"),
            "step `{step_type}` should be completed"
        );
    }

    for step_type in [
        "cuss_register_customer",
        "cuss_approve_and_deposit",
        "update_deposit_metadata",
    ] {
        assert_ne!(
            find_step(&steps, step_type)
                .ok()
                .and_then(|step| step.get("status"))
                .and_then(Value::as_str),
            Some("COMPLETED"),
            "step `{step_type}` should not be completed on reject"
        );
    }
}

#[then("the CUSS payloads match the first deposit flow")]
async fn cuss_payloads_match_first_deposit(world: &mut FullE2eWorld) {
    let requests = cuss_requests(world)
        .await
        .expect("cuss requests should load");
    let subject = world.subject().expect("subject should be set").to_owned();
    let deposit_amount = world
        .flow
        .deposit_amount
        .expect("deposit amount should be set");

    let register_request = requests
        .iter()
        .find(|item| item.get("endpoint").and_then(Value::as_str) == Some("register"))
        .expect("register request missing");
    let approve_request = requests
        .iter()
        .find(|item| item.get("endpoint").and_then(Value::as_str) == Some("approve"))
        .expect("approve request missing");

    assert_eq!(
        register_request.pointer("/payload/externalId"),
        Some(&json!(subject))
    );
    assert_eq!(
        register_request.pointer("/payload/fullName"),
        Some(&json!(FIXTURE_FULL_NAME))
    );
    assert_eq!(
        register_request.pointer("/payload/phone"),
        Some(&json!(FIXTURE_PHONE_NUMBER))
    );
    assert_eq!(
        approve_request.pointer("/payload/depositAmount"),
        Some(&json!(deposit_amount))
    );
    assert_eq!(
        approve_request.pointer("/payload/savingsAccountId"),
        Some(&json!(2))
    );
}

#[then("the reject path closes the session with reason REJECTED_BY_ADMIN")]
async fn reject_path_closes_session(world: &mut FullE2eWorld) {
    let session_id = world
        .flow
        .session_id
        .clone()
        .expect("session id should be set");
    wait_for_session_close_reason(
        world,
        &session_id,
        "CLOSED",
        "REJECTED_BY_ADMIN",
        Duration::from_secs(10),
    )
    .await
    .expect("session should be closed with reject reason");
}

#[then("no CUSS request was recorded")]
async fn no_cuss_request_recorded(world: &mut FullE2eWorld) {
    let requests = cuss_requests(world)
        .await
        .expect("cuss requests should load");
    assert!(
        requests.is_empty(),
        "unexpected CUSS requests: {requests:?}"
    );
}

#[then("CUSS register and approve requests were recorded")]
async fn cuss_requests_recorded(world: &mut FullE2eWorld) {
    let requests = cuss_requests(world)
        .await
        .expect("cuss requests should load");
    assert_eq!(requests.len(), 2, "unexpected CUSS requests: {requests:?}");
    assert!(
        requests
            .iter()
            .any(|item| { item.get("endpoint").and_then(Value::as_str) == Some("register") })
    );
    assert!(
        requests
            .iter()
            .any(|item| { item.get("endpoint").and_then(Value::as_str) == Some("approve") })
    );
}

#[then("no error occurred")]
async fn no_error(world: &mut FullE2eWorld) {
    assert!(world.error.is_none(), "Unexpected error: {:?}", world.error);
}

#[tokio::main]
async fn main() {
    FullE2eWorld::run("tests/features/full").await;
}
