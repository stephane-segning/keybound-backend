use super::BackendApi;
use axum::extract::{OriginalUri, Path, State};
use axum::http::{HeaderMap, Method};
use axum::response::IntoResponse;
use axum::{
    Json, Router,
    routing::{delete, get, post},
};
use backend_core::Error;
use backend_model::kc::EnrollmentBindRequest;
use backend_repository::{
    FlowInstanceCreateInput, FlowSessionCreateInput, FlowStepCreateInput, FlowStepPatch,
    SigningKeyCreateInput,
};
use base64::Engine;
use chrono::Utc;
use gen_oas_server_kc::types::Object;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use openssl::rsa::Rsa;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};

const HEADER_SIGNATURE: &str = "x-auth-signature";
const HEADER_TIMESTAMP: &str = "x-auth-timestamp";
const HEADER_DEVICE_ID: &str = "x-auth-device-id";

pub fn router(api: BackendApi) -> Router {
    Router::new()
        .route("/enroll", post(enroll))
        .route("/enroll/{id}/bind", post(bind_enroll))
        .route("/devices", get(list_devices))
        .route("/devices/{id}", delete(revoke_device))
        .route("/token", post(exchange_token))
        .route("/jwks", get(jwks))
        .route("/approve/{step_id}", post(approve_step))
        .with_state(api)
}

#[derive(Debug, Deserialize)]
struct EnrollRequest {
    pub user_id: String,
    #[serde(default)]
    pub realm: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    pub device_id: String,
    pub jkt: String,
    pub public_jwk: Value,
    #[serde(default)]
    pub user_hint: Option<String>,
    #[serde(default)]
    pub attributes: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize)]
struct EnrollResponse {
    id: String,
    session_id: String,
    flow_id: String,
    step_id: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct BindEnrollRequest {
    pub user_id: String,
    pub realm: String,
    pub client_id: String,
    pub device_id: String,
    pub jkt: String,
    pub public_jwk: Value,
    #[serde(default)]
    pub user_hint: Option<String>,
    #[serde(default)]
    pub attributes: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize)]
struct BindEnrollResponse {
    id: String,
    device_record_id: String,
    status: String,
}

#[derive(Debug, Serialize)]
struct DeviceRecordResponse {
    device_id: String,
    jkt: String,
    status: String,
    label: Option<String>,
    created_at: chrono::DateTime<Utc>,
    last_seen_at: Option<chrono::DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
struct DevicesResponse {
    items: Vec<DeviceRecordResponse>,
}

#[derive(Debug, Deserialize)]
struct TokenRequest {
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Serialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: i64,
    scope: String,
}

#[derive(Debug, Serialize)]
struct JwksResponse {
    keys: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct ApproveStepRequest {
    #[serde(default)]
    decision: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Serialize)]
struct ApproveStepResponse {
    step_id: String,
    status: String,
}

#[derive(Debug, Serialize)]
struct TokenClaims {
    sub: String,
    iss: String,
    aud: String,
    scope: String,
    device_id: String,
    iat: i64,
    exp: i64,
}

async fn enroll(
    State(api): State<BackendApi>,
    Json(body): Json<EnrollRequest>,
) -> Result<Json<EnrollResponse>, Error> {
    let session_id = backend_id::flow_session_id()?;
    let flow_id = backend_id::flow_instance_id()?;
    let step_id = backend_id::flow_step_id()?;

    if api.state.user.get_user(&body.user_id).await?.is_none() {
        return Err(Error::not_found("USER_NOT_FOUND", "User not found"));
    }

    let date = Utc::now().format("%Y-%m-%d").to_string();
    let session_human_id = format!("auth.{date}.{}", session_id);
    let flow_human_id = format!("{}.device_enroll", session_human_id);
    let step_human_id = format!("{}.bind", flow_human_id);

    let context = json!({
        "user_id": body.user_id,
        "realm": body.realm,
        "client_id": body.client_id,
        "device_id": body.device_id,
        "jkt": body.jkt,
        "public_jwk": canonicalize_jwk_value(&body.public_jwk)?,
        "user_hint": body.user_hint,
        "attributes": body.attributes,
        "flow_id": flow_id,
        "step_id": step_id,
    });

    api.state
        .flow
        .create_session(FlowSessionCreateInput {
            id: session_id.clone(),
            human_id: session_human_id,
            user_id: Some(body.user_id.clone()),
            session_type: "ACCOUNT_MANAGEMENT".to_owned(),
            status: "OPEN".to_owned(),
            context: context.clone(),
        })
        .await?;

    api.state
        .flow
        .create_flow(FlowInstanceCreateInput {
            id: flow_id.clone(),
            human_id: flow_human_id,
            session_id: session_id.clone(),
            flow_type: "DEVICE_ENROLL".to_owned(),
            status: "RUNNING".to_owned(),
            current_step: Some("BIND_DEVICE".to_owned()),
            step_ids: json!([step_id]),
            context,
        })
        .await?;

    api.state
        .flow
        .create_step(FlowStepCreateInput {
            id: step_id.clone(),
            human_id: step_human_id,
            flow_id: flow_id.clone(),
            step_type: "BIND_DEVICE".to_owned(),
            actor: "END_USER".to_owned(),
            status: "WAITING".to_owned(),
            attempt_no: 0,
            input: None,
            output: None,
            error: None,
            next_retry_at: None,
            finished_at: None,
        })
        .await?;

    Ok(Json(EnrollResponse {
        id: session_id.clone(),
        session_id,
        flow_id,
        step_id,
        status: "OPEN".to_owned(),
    }))
}

async fn bind_enroll(
    State(api): State<BackendApi>,
    Path(id): Path<String>,
    method: Method,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body_bytes: axum::body::Bytes,
) -> Result<Json<BindEnrollResponse>, Error> {
    let body: BindEnrollRequest = serde_json::from_slice(&body_bytes)
        .map_err(|error| Error::bad_request("INVALID_BODY", error.to_string()))?;

    let session = api
        .state
        .flow
        .get_session(&id)
        .await?
        .ok_or_else(|| Error::not_found("ENROLLMENT_NOT_FOUND", "Enrollment not found"))?;

    let expected_jwk = session
        .context
        .get("public_jwk")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::internal("ENROLLMENT_INVALID", "Enrollment JWK missing"))?;
    verify_signature_with_jwk(
        &api,
        &method,
        uri.path(),
        &headers,
        &body_bytes,
        expected_jwk,
    )?;

    let bind_request = EnrollmentBindRequest {
        realm: body.realm,
        client_id: body.client_id,
        user_id: body.user_id,
        user_hint: body.user_hint,
        device_id: body.device_id,
        jkt: body.jkt,
        public_jwk: value_to_kc_any_map(&body.public_jwk)?,
        attributes: body.attributes,
        created_at: Some(Utc::now()),
        proof: None,
    };

    let device_record_id = api.state.device.bind_device(&bind_request).await?;

    if let Some(step_id) = session.context.get("step_id").and_then(Value::as_str) {
        let _ = api
            .state
            .flow
            .patch_step(
                step_id,
                FlowStepPatch::new()
                    .status("COMPLETED")
                    .input(json!(bind_request.device_id))
                    .output(json!({ "device_record_id": device_record_id }))
                    .clear_error()
                    .finished_at(Utc::now()),
            )
            .await;
    }

    if let Some(flow_id) = session.context.get("flow_id").and_then(Value::as_str) {
        let _ = api
            .state
            .flow
            .update_flow(
                flow_id,
                Some("COMPLETED".to_owned()),
                Some(None),
                None,
                Some(json!({"bound_device_id": bind_request.device_id})),
            )
            .await;
    }

    api.state
        .flow
        .update_session_status(&id, "COMPLETED", Some(Utc::now()))
        .await?;

    Ok(Json(BindEnrollResponse {
        id,
        device_record_id,
        status: "BOUND".to_owned(),
    }))
}

async fn list_devices(
    State(api): State<BackendApi>,
    method: Method,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
) -> Result<Json<DevicesResponse>, Error> {
    let auth = authenticate_device(&api, &method, uri.path(), &headers, &[]).await?;
    let devices = api
        .state
        .device
        .list_user_devices(&auth.user_id, true)
        .await?;

    Ok(Json(DevicesResponse {
        items: devices
            .into_iter()
            .map(|row| DeviceRecordResponse {
                device_id: row.device_id,
                jkt: row.jkt,
                status: row.status,
                label: row.label,
                created_at: row.created_at,
                last_seen_at: row.last_seen_at,
            })
            .collect(),
    }))
}

async fn revoke_device(
    State(api): State<BackendApi>,
    Path(device_id): Path<String>,
    method: Method,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
) -> Result<impl IntoResponse, Error> {
    let auth = authenticate_device(&api, &method, uri.path(), &headers, &[]).await?;

    let device = api
        .state
        .device
        .get_user_device(&auth.user_id, &device_id)
        .await?
        .ok_or_else(|| Error::not_found("DEVICE_NOT_FOUND", "Device not found"))?;

    api.state
        .device
        .update_device_status(&device.device_record_id, "REVOKED")
        .await?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn exchange_token(
    State(api): State<BackendApi>,
    method: Method,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body_bytes: axum::body::Bytes,
) -> Result<Json<TokenResponse>, Error> {
    let body: TokenRequest = serde_json::from_slice(&body_bytes)
        .map_err(|error| Error::bad_request("INVALID_BODY", error.to_string()))?;

    let auth = authenticate_device(&api, &method, uri.path(), &headers, &body_bytes).await?;
    let scope = body.scope.unwrap_or_else(|| "openid profile".to_owned());

    let key = ensure_active_signing_key(&api).await?;

    let now = Utc::now().timestamp();
    let expires_in = api.state.config.auth.token_ttl_seconds.max(60);
    let claims = TokenClaims {
        sub: auth.user_id,
        iss: api
            .state
            .config
            .auth
            .token_issuer
            .clone()
            .unwrap_or_else(|| api.state.config.oauth2.issuer.clone()),
        aud: api
            .state
            .config
            .auth
            .token_audience
            .clone()
            .unwrap_or_else(|| "user-storage-auth".to_owned()),
        scope: scope.clone(),
        device_id: auth.device_id,
        iat: now,
        exp: now + expires_in,
    };

    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(key.kid.clone());

    let token = encode(
        &header,
        &claims,
        &EncodingKey::from_rsa_pem(key.private_key_pem.as_bytes())
            .map_err(|error| Error::internal("AUTH_SIGNING_KEY_INVALID", error.to_string()))?,
    )
    .map_err(|error| Error::internal("AUTH_TOKEN_ISSUE_FAILED", error.to_string()))?;

    Ok(Json(TokenResponse {
        access_token: token,
        token_type: "Bearer".to_owned(),
        expires_in,
        scope,
    }))
}

async fn jwks(State(api): State<BackendApi>) -> Result<Json<JwksResponse>, Error> {
    let keys = api.state.flow.list_active_signing_keys().await?;
    Ok(Json(JwksResponse {
        keys: keys.into_iter().map(|row| row.public_key_jwk).collect(),
    }))
}

async fn approve_step(
    State(api): State<BackendApi>,
    Path(step_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ApproveStepRequest>,
) -> Result<Json<ApproveStepResponse>, Error> {
    let auth_header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !auth_header.to_ascii_lowercase().starts_with("bearer ") {
        return Err(Error::unauthorized("Missing bearer token"));
    }

    let decision = body
        .decision
        .unwrap_or_else(|| "APPROVED".to_owned())
        .to_ascii_uppercase();
    let target_status = if decision == "APPROVED" {
        "COMPLETED"
    } else {
        "FAILED"
    };

    let patch = if decision == "APPROVED" {
        FlowStepPatch::new()
            .status(target_status)
            .output(json!({"decision": decision, "message": body.message}))
            .clear_error()
            .finished_at(Utc::now())
    } else {
        FlowStepPatch::new()
            .status(target_status)
            .error(json!({"decision": decision, "message": body.message}))
            .finished_at(Utc::now())
    };

    api.state.flow.patch_step(&step_id, patch).await?;

    Ok(Json(ApproveStepResponse {
        step_id,
        status: target_status.to_owned(),
    }))
}

#[derive(Debug)]
struct AuthenticatedDevice {
    device_id: String,
    user_id: String,
}

async fn authenticate_device(
    api: &BackendApi,
    method: &Method,
    path: &str,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<AuthenticatedDevice, Error> {
    let device_id = header_value(headers, HEADER_DEVICE_ID)
        .ok_or_else(|| Error::unauthorized("Missing x-auth-device-id"))?;

    let lookup = backend_model::kc::DeviceLookupRequest {
        device_id: Some(device_id.clone()),
        jkt: None,
    };

    let device = api
        .state
        .device
        .lookup_device(&lookup)
        .await?
        .ok_or_else(|| Error::unauthorized("Unknown auth device"))?;

    if !device.status.eq_ignore_ascii_case("active") {
        return Err(Error::unauthorized("Device is not active"));
    }

    verify_signature_with_jwk(api, method, path, headers, body, &device.public_jwk)?;

    Ok(AuthenticatedDevice {
        device_id: device.device_id,
        user_id: device.user_id,
    })
}

fn verify_signature_with_jwk(
    api: &BackendApi,
    method: &Method,
    path: &str,
    headers: &HeaderMap,
    body: &[u8],
    public_jwk: &str,
) -> Result<(), Error> {
    let signature = header_value(headers, HEADER_SIGNATURE)
        .ok_or_else(|| Error::unauthorized("Missing x-auth-signature"))?;
    let timestamp_str = header_value(headers, HEADER_TIMESTAMP)
        .ok_or_else(|| Error::unauthorized("Missing x-auth-timestamp"))?;

    let timestamp = timestamp_str
        .parse::<i64>()
        .map_err(|_| Error::unauthorized("Invalid x-auth-timestamp"))?;
    let skew = (Utc::now().timestamp() - timestamp).abs();
    if skew > api.state.config.auth.max_clock_skew_seconds {
        return Err(Error::unauthorized("Timestamp out of skew"));
    }

    let body_str = std::str::from_utf8(body)
        .map_err(|_| Error::bad_request("INVALID_BODY", "Body must be utf-8"))?;

    let canonical_payload = format!(
        "{}\n{}\n{}\n{}\n{}",
        timestamp,
        method.as_str().to_uppercase(),
        path,
        body_str,
        public_jwk,
    );
    let digest = Sha256::digest(canonical_payload.as_bytes());
    let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    if signature != expected {
        return Err(Error::unauthorized("Invalid signature"));
    }

    Ok(())
}

fn header_value(headers: &HeaderMap, key: &str) -> Option<String> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

async fn ensure_active_signing_key(
    api: &BackendApi,
) -> Result<backend_model::db::SigningKeyRow, Error> {
    if let Some(active) = api.state.flow.get_active_signing_key().await? {
        return Ok(active);
    }

    let (kid, private_key_pem, public_jwk) = generate_rsa_signing_key()?;
    let _ = api.state.flow.deactivate_signing_keys().await?;
    api.state
        .flow
        .create_signing_key(SigningKeyCreateInput {
            kid,
            private_key_pem,
            public_key_jwk: public_jwk,
            algorithm: "RS256".to_owned(),
            expires_at: None,
            is_active: true,
        })
        .await
}

fn generate_rsa_signing_key() -> Result<(String, String, Value), Error> {
    let rsa = Rsa::generate(2048)
        .map_err(|error| Error::internal("AUTH_KEYGEN_FAILED", error.to_string()))?;
    let private_key_pem = String::from_utf8(
        rsa.private_key_to_pem()
            .map_err(|error| Error::internal("AUTH_KEYGEN_FAILED", error.to_string()))?,
    )
    .map_err(|error| Error::internal("AUTH_KEYGEN_FAILED", error.to_string()))?;

    let kid = backend_id::signing_key_id()?;
    let n = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rsa.n().to_vec());
    let e = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rsa.e().to_vec());
    let public_jwk = json!({
        "kid": kid,
        "kty": "RSA",
        "alg": "RS256",
        "use": "sig",
        "n": n,
        "e": e,
    });

    Ok((kid, private_key_pem, public_jwk))
}

fn value_to_kc_any_map(value: &Value) -> Result<HashMap<String, Object>, Error> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::bad_request("INVALID_JWK", "public_jwk must be an object"))?;

    Ok(object
        .iter()
        .map(|(key, value)| (key.clone(), Object(value.clone())))
        .collect())
}

fn canonicalize_jwk_value(value: &Value) -> Result<String, Error> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::bad_request("INVALID_JWK", "public_jwk must be an object"))?;
    let sorted = object
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<String, Value>>();
    serde_json::to_string(&sorted)
        .map_err(|error| Error::bad_request("INVALID_JWK", error.to_string()))
}
