use crate::api::{BFF_AUTH_DEVICE_ID_HEADER, BFF_AUTH_USER_ID_HEADER};
use crate::auth_signature::{
    ReplayGuard, canonicalize_device_auth_payload, canonicalize_public_key, validate_public_key_match,
    validate_timestamp, validate_user_id_hint, verify_signature,
};
use crate::state::AppState;
use axum::body::{Body, to_bytes};
use axum::extract::{OriginalUri, State};
use axum::http::{HeaderMap, HeaderValue, Method, Request};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use backend_core::Error;
use std::sync::Arc;

const HEADER_SIGNATURE: &str = "x-auth-signature";
const HEADER_TIMESTAMP: &str = "x-auth-signature-timestamp";
const HEADER_PUBLIC_KEY: &str = "x-auth-public-key";
const HEADER_DEVICE_ID: &str = "x-auth-device-id";
const HEADER_NONCE: &str = "x-auth-nonce";
const HEADER_USER_ID: &str = "x-auth-user-id";

pub async fn require_bff_signature(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if !state.config.bff.enabled {
        return next.run(req).await;
    }

    let method = req.method().clone();
    let path = req
        .extensions()
        .get::<OriginalUri>()
        .map(|uri| uri.0.path().to_owned())
        .unwrap_or_else(|| req.uri().path().to_owned());

    let (mut parts, body) = req.into_parts();
    let body_bytes = match to_bytes(body, state.config.kc.max_body_bytes).await {
        Ok(value) => value,
        Err(_) => return Error::unauthorized("Invalid request body").into_response(),
    };

    let (user_id, device_id) =
        match authenticate_signature(&state, &method, &path, &parts.headers, body_bytes.as_ref())
            .await
        {
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

async fn authenticate_signature(
    state: &Arc<AppState>,
    method: &Method,
    path: &str,
    headers: &HeaderMap,
    body: &[u8],
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

    let timestamp_i64 = timestamp_str
        .parse::<i64>()
        .map_err(|_| Error::unauthorized("Invalid x-auth-signature-timestamp"))?;

    let canonical_payload = canonicalize_device_auth_payload(
        &device_id,
        &nonce,
        &public_key,
        timestamp_i64,
    )?;

    verify_signature(&public_key, &canonical_payload, &signature)?;

    Ok((device.user_id, device.device_id))
}

fn header_value(headers: &HeaderMap, key: &str) -> Option<String> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}
