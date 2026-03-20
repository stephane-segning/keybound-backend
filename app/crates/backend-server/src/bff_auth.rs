use crate::api::{BFF_AUTH_DEVICE_ID_HEADER, BFF_AUTH_USER_ID_HEADER};
use crate::auth_signature::{
    canonicalize_device_auth_payload, validate_public_key_match, validate_timestamp,
    validate_user_id_hint, verify_signature,
};
use crate::state::AppState;
use axum::body::{Body, to_bytes};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Request};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use backend_auth::JwtToken;
use backend_core::Error;
use std::sync::Arc;
use tracing::debug;

const HEADER_SIGNATURE: &str = "x-auth-signature";
const HEADER_TIMESTAMP: &str = "x-auth-signature-timestamp";
const HEADER_PUBLIC_KEY: &str = "x-auth-public-key";
const HEADER_DEVICE_ID: &str = "x-auth-device-id";
const HEADER_NONCE: &str = "x-auth-nonce";
const HEADER_USER_ID: &str = "x-auth-user-id";
const HEADER_AUTHORIZATION: &str = "authorization";

pub async fn require_bff_auth(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if !state.config.bff.enabled {
        return next.run(req).await;
    }

    let (mut parts, body) = req.into_parts();
    let body_bytes = match to_bytes(body, state.config.kc.max_body_bytes).await {
        Ok(value) => value,
        Err(_) => return Error::unauthorized("Invalid request body").into_response(),
    };

    let (user_id, device_id) = match authenticate(&state, &parts.headers).await {
        Ok(claims) => claims,
        Err(error) => return error.into_response(),
    };

    let user_header = match HeaderValue::from_str(&user_id) {
        Ok(value) => value,
        Err(_) => return Error::unauthorized("Invalid authenticated user id").into_response(),
    };
    let device_header = match HeaderValue::from_str(&device_id) {
        Ok(value) => value,
        Err(_) => return Error::unauthorized("Invalid authenticated device id").into_response(),
    };

    parts.headers.remove(BFF_AUTH_USER_ID_HEADER);
    parts.headers.insert(BFF_AUTH_USER_ID_HEADER, user_header);
    parts.headers.remove(BFF_AUTH_DEVICE_ID_HEADER);
    parts
        .headers
        .insert(BFF_AUTH_DEVICE_ID_HEADER, device_header);

    let req = Request::from_parts(parts, Body::from(body_bytes));
    next.run(req).await
}

async fn authenticate(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<(String, String), Error> {
    if let Some(token) = extract_bearer_token(headers) {
        return authenticate_bearer(state, &token).await;
    }

    authenticate_signature(state, headers).await
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let auth_header = headers
        .get(HEADER_AUTHORIZATION)
        .and_then(|value| value.to_str().ok())?;

    let prefix = "Bearer ";
    if !auth_header.to_ascii_lowercase().starts_with("bearer ") {
        return None;
    }

    Some(auth_header[prefix.len()..].to_owned())
}

async fn authenticate_bearer(
    state: &Arc<AppState>,
    token: &str,
) -> Result<(String, String), Error> {
    let jwt = JwtToken::verify(token, &state.oidc_state).await?;
    let user_id = jwt.user_id().to_owned();
    let device_id = "bff".to_owned();

    debug!(
        user_id = %user_id,
        "Bearer token authentication successful"
    );

    Ok((user_id, device_id))
}

async fn authenticate_signature(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<(String, String), Error> {
    let device_id = header_value(headers, HEADER_DEVICE_ID)
        .ok_or_else(|| Error::unauthorized("Missing x-auth-device-id"))?;
    let signature = header_value(headers, HEADER_SIGNATURE)
        .ok_or_else(|| Error::unauthorized("Missing x-auth-signature"))?;
    let timestamp_str = header_value(headers, HEADER_TIMESTAMP)
        .ok_or_else(|| Error::unauthorized("Missing x-auth-signature-timestamp"))?;
    let public_key = header_value(headers, HEADER_PUBLIC_KEY)
        .ok_or_else(|| Error::unauthorized("Missing x-auth-public-key"))?;
    let nonce = header_value(headers, HEADER_NONCE)
        .ok_or_else(|| Error::unauthorized("Missing x-auth-nonce"))?;
    let user_id_hint = header_value(headers, HEADER_USER_ID);

    tracing::debug!(
        device_id = %device_id,
        signature = %signature,
        timestamp = %timestamp_str,
        nonce = %nonce,
        public_key_len = public_key.len(),
        "Received signature auth headers"
    );

    let timestamp = timestamp_str
        .parse::<i64>()
        .map_err(|_| Error::unauthorized("Invalid x-auth-signature-timestamp"))?;

    validate_timestamp(timestamp, state.config.auth.max_clock_skew_seconds)?;

    state
        .replay_guard
        .check_and_record(
            &device_id,
            &nonce,
            timestamp,
            state.config.auth.max_clock_skew_seconds,
        )
        .await?;

    let lookup = backend_model::kc::DeviceLookupRequest {
        device_id: Some(device_id.clone()),
        jkt: None,
    };

    let device = state
        .device
        .lookup_device(&lookup)
        .await?
        .ok_or_else(|| Error::unauthorized("Unknown auth device"))?;

    if !device.status.eq_ignore_ascii_case("active") {
        return Err(Error::unauthorized("Device is not active"));
    }

    validate_user_id_hint(user_id_hint.as_deref(), &device.user_id)?;
    validate_public_key_match(&public_key, &device.public_jwk)?;

    let canonical_payload =
        canonicalize_device_auth_payload(&device_id, &nonce, &public_key, timestamp)?;

    tracing::debug!(
        canonical_payload = %canonical_payload,
        device_public_jwk = %device.public_jwk,
        "Verifying signature"
    );

    verify_signature(&public_key, &canonical_payload, &signature)?;

    tracing::info!(
        device_id = %device_id,
        user_id = %device.user_id,
        "Signature authentication successful"
    );

    Ok((device.user_id, device.device_id))
}

fn header_value(headers: &HeaderMap, key: &str) -> Option<String> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}
