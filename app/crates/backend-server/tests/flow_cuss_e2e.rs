use backend_flow_sdk::step::ContextUpdates;
use backend_flow_sdk::{
    Actor, Flow, FlowError, FlowRegistry, SessionDefinition, Step, StepContext, StepOutcome,
    StepTransition,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct CussRegisterStep {
    cuss_url: String,
}

#[async_trait::async_trait]
impl Step for CussRegisterStep {
    fn step_type(&self) -> &'static str {
        "CUSS_REGISTER_CUSTOMER"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "cuss_register"
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        let phone = ctx
            .session_context
            .get("phone_number")
            .and_then(|v| v.as_str())
            .unwrap_or("+237690000000");

        let full_name = ctx
            .session_context
            .get("full_name")
            .and_then(|v| v.as_str())
            .unwrap_or("Test User");

        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}/api/registration/register", self.cuss_url))
            .json(&json!({
                "fullName": full_name,
                "phone": phone,
                "externalId": ctx.session_id
            }))
            .timeout(Duration::from_secs(10))
            .send()
            .await;

        match response {
            Ok(res) if res.status().is_success() => {
                let body: Value = res.json().await.unwrap_or_else(|_| json!({}));
                let fineract_client_id = body
                    .get("fineractClientId")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);

                Ok(StepOutcome::Done {
                    output: Some(json!({
                        "fineractClientId": fineract_client_id,
                        "success": true
                    })),
                    updates: Some(Box::new(ContextUpdates {
                        user_metadata_patch: Some(json!({
                            "fineractClientId": fineract_client_id,
                            "cuss_registration_status": "COMPLETED",
                            "cuss_registration_at": chrono::Utc::now().to_rfc3339()
                        })),
                        session_context_patch: Some(json!({
                            "fineractClientId": fineract_client_id
                        })),
                        flow_context_patch: None,
                        notifications: None,
                    })),
                })
            }
            Ok(res) => {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                Ok(StepOutcome::Failed {
                    error: format!("CUSS register failed with status {}: {}", status, body),
                    retryable: status.is_server_error(),
                })
            }
            Err(e) => Ok(StepOutcome::Failed {
                error: format!("CUSS register network error: {}", e),
                retryable: true,
            }),
        }
    }
}

struct CussApproveStep {
    cuss_url: String,
}

#[async_trait::async_trait]
impl Step for CussApproveStep {
    fn step_type(&self) -> &'static str {
        "CUSS_APPROVE_AND_DEPOSIT"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "cuss_approve"
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        let savings_account_id = ctx
            .flow_context
            .get("step_output")
            .and_then(|v| v.get("CUSS_REGISTER_CUSTOMER"))
            .and_then(|v| v.get("savingsAccountId"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let deposit_amount = ctx
            .session_context
            .get("deposit_amount")
            .and_then(|v| v.as_f64())
            .unwrap_or(1000.0);

        let client = reqwest::Client::new();
        let response = client
            .post(format!(
                "{}/api/registration/approve-and-deposit",
                self.cuss_url
            ))
            .json(&json!({
                "savingsAccountId": savings_account_id,
                "depositAmount": deposit_amount
            }))
            .timeout(Duration::from_secs(10))
            .send()
            .await;

        match response {
            Ok(res) if res.status().is_success() => {
                let body: Value = res.json().await.unwrap_or_else(|_| json!({}));
                let transaction_id = body
                    .get("transactionId")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);

                Ok(StepOutcome::Done {
                    output: Some(json!({
                        "transactionId": transaction_id,
                        "savingsAccountId": savings_account_id,
                        "success": true
                    })),
                    updates: Some(Box::new(ContextUpdates {
                        user_metadata_patch: Some(json!({
                            "savingsAccountId": savings_account_id,
                            "deposit_transaction_id": transaction_id,
                            "cuss_approval_status": "COMPLETED",
                            "cuss_approval_at": chrono::Utc::now().to_rfc3339()
                        })),
                        session_context_patch: None,
                        flow_context_patch: None,
                        notifications: None,
                    })),
                })
            }
            Ok(res) => {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                Ok(StepOutcome::Failed {
                    error: format!("CUSS approve failed with status {}: {}", status, body),
                    retryable: status.is_server_error(),
                })
            }
            Err(e) => Ok(StepOutcome::Failed {
                error: format!("CUSS approve network error: {}", e),
                retryable: true,
            }),
        }
    }
}

struct AwaitApprovalStep;

#[async_trait::async_trait]
impl Step for AwaitApprovalStep {
    fn step_type(&self) -> &'static str {
        "AWAIT_ADMIN_APPROVAL"
    }

    fn actor(&self) -> Actor {
        Actor::Admin
    }

    fn human_id(&self) -> &'static str {
        "await_approval"
    }

    async fn execute(&self, _ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        Ok(StepOutcome::Waiting {
            actor: Actor::Admin,
        })
    }
}

struct CheckUserStep;

#[async_trait::async_trait]
impl Step for CheckUserStep {
    fn step_type(&self) -> &'static str {
        "CHECK_USER_EXISTS"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "check_user"
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        let phone = ctx
            .session_context
            .get("phone_number")
            .and_then(|v| v.as_str());

        let user_id = ctx.session_context.get("userId").and_then(|v| v.as_str());

        Ok(StepOutcome::Done {
            output: Some(json!({
                "userExists": user_id.is_some(),
                "phone": phone
            })),
            updates: Some(Box::new(ContextUpdates {
                flow_context_patch: Some(json!({
                    "user_exists": user_id.is_some()
                })),
                session_context_patch: None,
                user_metadata_patch: None,
                notifications: None,
            })),
        })
    }
}

struct ValidateDepositStep;

#[async_trait::async_trait]
impl Step for ValidateDepositStep {
    fn step_type(&self) -> &'static str {
        "VALIDATE_DEPOSIT"
    }

    fn actor(&self) -> Actor {
        Actor::System
    }

    fn human_id(&self) -> &'static str {
        "validate_deposit"
    }

    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        let amount = ctx
            .session_context
            .get("deposit_amount")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| FlowError::InvalidDefinition("Missing deposit_amount".to_string()))?;

        if !(1.0..=100000.0).contains(&amount) {
            return Ok(StepOutcome::Failed {
                error: "Invalid deposit amount".to_string(),
                retryable: false,
            });
        }

        Ok(StepOutcome::Done {
            output: Some(json!({ "valid": true, "amount": amount })),
            updates: None,
        })
    }
}

struct CussDepositFlow {
    transitions: HashMap<String, StepTransition>,
    steps: Vec<Arc<dyn Step>>,
}

impl CussDepositFlow {
    fn new(cuss_url: String) -> Self {
        let steps: Vec<Arc<dyn Step>> = vec![
            Arc::new(CheckUserStep),
            Arc::new(ValidateDepositStep),
            Arc::new(AwaitApprovalStep),
            Arc::new(CussRegisterStep {
                cuss_url: cuss_url.clone(),
            }),
            Arc::new(CussApproveStep { cuss_url }),
        ];

        let mut transitions = HashMap::new();
        transitions.insert(
            "CHECK_USER_EXISTS".to_string(),
            StepTransition {
                on_success: "VALIDATE_DEPOSIT".to_string(),
                on_failure: Some("FAILED".to_string()),
            },
        );
        transitions.insert(
            "VALIDATE_DEPOSIT".to_string(),
            StepTransition {
                on_success: "AWAIT_ADMIN_APPROVAL".to_string(),
                on_failure: Some("FAILED".to_string()),
            },
        );
        transitions.insert(
            "AWAIT_ADMIN_APPROVAL".to_string(),
            StepTransition {
                on_success: "CUSS_REGISTER_CUSTOMER".to_string(),
                on_failure: Some("FAILED".to_string()),
            },
        );
        transitions.insert(
            "CUSS_REGISTER_CUSTOMER".to_string(),
            StepTransition {
                on_success: "CUSS_APPROVE_AND_DEPOSIT".to_string(),
                on_failure: Some("FAILED".to_string()),
            },
        );
        transitions.insert(
            "CUSS_APPROVE_AND_DEPOSIT".to_string(),
            StepTransition {
                on_success: "COMPLETED".to_string(),
                on_failure: Some("FAILED".to_string()),
            },
        );

        Self { steps, transitions }
    }
}

impl Flow for CussDepositFlow {
    fn flow_type(&self) -> &str {
        "CUSS_DEPOSIT"
    }

    fn human_id(&self) -> &str {
        "cuss_deposit"
    }

    fn feature(&self) -> Option<&str> {
        None
    }

    fn steps(&self) -> &[Arc<dyn Step>] {
        &self.steps
    }

    fn initial_step(&self) -> &str {
        "CHECK_USER_EXISTS"
    }

    fn transitions(&self) -> &HashMap<String, StepTransition> {
        &self.transitions
    }
}

struct MockUserRepo {
    users: HashMap<String, Value>,
}

impl MockUserRepo {
    fn new() -> Self {
        Self {
            users: HashMap::new(),
        }
    }

    fn add_user(&mut self, user_id: &str, metadata: Value) {
        self.users.insert(
            user_id.to_string(),
            json!({
                "user_id": user_id,
                "metadata": metadata
            }),
        );
    }

    fn get_metadata(&self, user_id: &str) -> Option<&Value> {
        self.users.get(user_id).and_then(|u| u.get("metadata"))
    }

    fn update_metadata(&mut self, user_id: &str, patch: Value) {
        if let Some(user) = self.users.get_mut(user_id)
            && let Some(meta) = user.get_mut("metadata")
            && let (Some(meta_obj), Some(patch_obj)) = (meta.as_object_mut(), patch.as_object())
        {
            for (k, v) in patch_obj {
                meta_obj.insert(k.clone(), v.clone());
            }
        }
    }
}

#[tokio::test]
async fn test_flow_cuss_deposit_saves_metadata() {
    let mock_server = MockServer::start().await;
    let cuss_url = mock_server.uri();

    let test_user_id = "usr_test_001";

    Mock::given(method("POST"))
        .and(path("/api/registration/register"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "success": true,
            "status": "success",
            "fineractClientId": 12345,
            "savingsAccountId": 67890
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/registration/approve-and-deposit"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "status": "success",
            "savingsAccountId": 67890,
            "transactionId": 99999
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mut registry = FlowRegistry::new();
    let flow = Arc::new(CussDepositFlow::new(cuss_url));
    for step in flow.steps() {
        registry.register_step(step.clone());
    }
    registry.register_flow(flow);

    registry.register_session(SessionDefinition {
        session_type: "KYC_FULL".to_string(),
        human_id_prefix: "kyc".to_string(),
        feature: None,
        allowed_flows: vec!["CUSS_DEPOSIT".to_string()],
    });

    let mut user_repo = MockUserRepo::new();
    user_repo.add_user(test_user_id, json!({}));

    let session_context = json!({
        "userId": test_user_id,
        "phone_number": "+237690000000",
        "full_name": "Test User",
        "deposit_amount": 5000.0
    });

    let flow_def = registry.get_flow("CUSS_DEPOSIT").expect("flow");
    let initial_step = flow_def.initial_step();
    let step_def = flow_def
        .steps()
        .iter()
        .find(|s| s.step_type() == initial_step)
        .expect("initial step");

    let ctx = StepContext {
        session_id: "sess_001".to_string(),
        session_user_id: Some(test_user_id.to_string()),
        flow_id: "flow_001".to_string(),
        step_id: "step_001".to_string(),
        input: json!({}),
        session_context: session_context.clone(),
        flow_context: json!({}),
        services: Default::default(),
    };

    let outcome = step_def.execute(&ctx).await.expect("check user step");
    match outcome {
        StepOutcome::Done { output, updates } => {
            assert!(output.is_some());
            if let Some(updates) = updates
                && let Some(metadata_patch) = updates.user_metadata_patch
            {
                user_repo.update_metadata(test_user_id, metadata_patch);
            }
        }
        _ => panic!("Expected Done outcome"),
    }

    let validate_step = flow_def
        .steps()
        .iter()
        .find(|s| s.step_type() == "VALIDATE_DEPOSIT")
        .expect("validate step");

    let ctx2 = StepContext {
        session_id: "sess_001".to_string(),
        session_user_id: Some(test_user_id.to_string()),
        flow_id: "flow_001".to_string(),
        step_id: "step_002".to_string(),
        input: json!({}),
        session_context: session_context.clone(),
        flow_context: json!({ "user_exists": true }),
        services: Default::default(),
    };

    let outcome2 = validate_step.execute(&ctx2).await.expect("validate step");
    match outcome2 {
        StepOutcome::Done { output, .. } => {
            let output = output.expect("output");
            assert!(
                output
                    .get("valid")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            );
        }
        _ => panic!("Expected Done outcome for validate"),
    }

    let register_step = flow_def
        .steps()
        .iter()
        .find(|s| s.step_type() == "CUSS_REGISTER_CUSTOMER")
        .expect("register step");

    let ctx3 = StepContext {
        session_id: "sess_001".to_string(),
        session_user_id: Some(test_user_id.to_string()),
        flow_id: "flow_001".to_string(),
        step_id: "step_003".to_string(),
        input: json!({}),
        session_context: session_context.clone(),
        flow_context: json!({ "step_output": {} }),
        services: Default::default(),
    };

    let outcome3 = register_step.execute(&ctx3).await.expect("register step");
    match outcome3 {
        StepOutcome::Done { output, updates } => {
            let output = output.expect("output");
            let client_id = output
                .get("fineractClientId")
                .and_then(|v| v.as_i64())
                .expect("fineractClientId");
            assert_eq!(client_id, 12345);

            if let Some(updates) = updates
                && let Some(metadata_patch) = updates.user_metadata_patch
            {
                user_repo.update_metadata(test_user_id, metadata_patch);
            }
        }
        StepOutcome::Failed { error, .. } => {
            panic!("Register step failed: {}", error);
        }
        _ => panic!("Expected Done outcome for register"),
    }

    let approve_step = flow_def
        .steps()
        .iter()
        .find(|s| s.step_type() == "CUSS_APPROVE_AND_DEPOSIT")
        .expect("approve step");

    let ctx4 = StepContext {
        session_id: "sess_001".to_string(),
        session_user_id: Some(test_user_id.to_string()),
        flow_id: "flow_001".to_string(),
        step_id: "step_004".to_string(),
        input: json!({}),
        session_context: session_context.clone(),
        flow_context: json!({
            "step_output": {
                "CUSS_REGISTER_CUSTOMER": {
                    "savingsAccountId": 67890
                }
            }
        }),
        services: Default::default(),
    };

    let outcome4 = approve_step.execute(&ctx4).await.expect("approve step");
    match outcome4 {
        StepOutcome::Done { output, updates } => {
            let output = output.expect("output");
            let tx_id = output
                .get("transactionId")
                .and_then(|v| v.as_i64())
                .expect("transactionId");
            assert_eq!(tx_id, 99999);
            let savings_id = output
                .get("savingsAccountId")
                .and_then(|v| v.as_i64())
                .expect("savingsAccountId");
            assert_eq!(savings_id, 67890);

            if let Some(updates) = updates
                && let Some(metadata_patch) = updates.user_metadata_patch
            {
                user_repo.update_metadata(test_user_id, metadata_patch);
            }
        }
        StepOutcome::Failed { error, .. } => {
            panic!("Approve step failed: {}", error);
        }
        _ => panic!("Expected Done outcome for approve"),
    }

    let metadata = user_repo.get_metadata(test_user_id).expect("metadata");
    assert_eq!(
        metadata.get("fineractClientId").and_then(|v| v.as_i64()),
        Some(12345)
    );
    assert_eq!(
        metadata.get("savingsAccountId").and_then(|v| v.as_i64()),
        Some(67890)
    );
    assert_eq!(
        metadata
            .get("deposit_transaction_id")
            .and_then(|v| v.as_i64()),
        Some(99999)
    );
    assert_eq!(
        metadata
            .get("cuss_registration_status")
            .and_then(|v| v.as_str()),
        Some("COMPLETED")
    );
    assert_eq!(
        metadata
            .get("cuss_approval_status")
            .and_then(|v| v.as_str()),
        Some("COMPLETED")
    );
}

#[tokio::test]
async fn test_flow_cuss_register_retryable_on_5xx() {
    let mock_server = MockServer::start().await;
    let cuss_url = mock_server.uri();

    Mock::given(method("POST"))
        .and(path("/api/registration/register"))
        .respond_with(ResponseTemplate::new(503).set_body_json(json!({
            "error": "Service unavailable"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let register_step = CussRegisterStep { cuss_url };

    let ctx = StepContext {
        session_id: "sess_001".to_string(),
        session_user_id: Some("usr_001".to_string()),
        flow_id: "flow_001".to_string(),
        step_id: "step_001".to_string(),
        input: json!({}),
        session_context: json!({
            "phone_number": "+237690000000",
            "full_name": "Test User"
        }),
        flow_context: json!({}),
        services: Default::default(),
    };

    let outcome = register_step.execute(&ctx).await.expect("step execution");
    match outcome {
        StepOutcome::Failed { error, retryable } => {
            assert!(error.contains("503"));
            assert!(retryable);
        }
        _ => panic!("Expected Failed outcome with retryable=true"),
    }
}

#[tokio::test]
async fn test_flow_cuss_register_non_retryable_on_4xx() {
    let mock_server = MockServer::start().await;
    let cuss_url = mock_server.uri();

    Mock::given(method("POST"))
        .and(path("/api/registration/register"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "Invalid request"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let register_step = CussRegisterStep { cuss_url };

    let ctx = StepContext {
        session_id: "sess_001".to_string(),
        session_user_id: Some("usr_001".to_string()),
        flow_id: "flow_001".to_string(),
        step_id: "step_001".to_string(),
        input: json!({}),
        session_context: json!({
            "phone_number": "+237690000000",
            "full_name": "Test User"
        }),
        flow_context: json!({}),
        services: Default::default(),
    };

    let outcome = register_step.execute(&ctx).await.expect("step execution");
    match outcome {
        StepOutcome::Failed { error, retryable } => {
            assert!(error.contains("400"));
            assert!(!retryable);
        }
        _ => panic!("Expected Failed outcome with retryable=false"),
    }
}
