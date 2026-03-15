use crate::api::{BFF_AUTH_DEVICE_ID_HEADER, BFF_AUTH_USER_ID_HEADER};
use crate::state::AppState;
use axum::body::{Body, to_bytes};
use axum::extract::{OriginalUri, State};
use axum::http::{HeaderMap, HeaderValue, Method, Request};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use backend_core::Error;
use base64::Engine as _;
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

const HEADER_SIGNATURE: &str = "x-auth-signature";
const HEADER_TIMESTAMP: &str = "x-auth-signature-timestamp";
const HEADER_PUBLIC_KEY: &str = "x-auth-public-key";
const HEADER_DEVICE_ID: &str = "x-auth-device-id";
const HEADER_NONCE: &str = "x-auth-nonce";
const HEADER_USER_ID: &str = "x-auth-user-id";

static NONCE_CACHE: OnceLock<Mutex<HashMap<String, i64>>> = OnceLock::new();

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

    let timestamp = timestamp_str
        .parse::<i64>()
        .map_err(|_| Error::unauthorized("Invalid x-auth-signature-timestamp"))?;

    let skew = (Utc::now().timestamp() - timestamp).abs();
    if skew > state.config.auth.max_clock_skew_seconds {
        return Err(Error::unauthorized("Timestamp out of skew"));
    }

    verify_nonce(
        &device_id,
        &nonce,
        timestamp,
        state.config.auth.max_clock_skew_seconds,
    )?;

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

    if let Some(user_id) = user_id_hint.as_deref()
        && user_id != device.user_id
    {
        return Err(Error::unauthorized(
            "x-auth-user-id does not match device owner",
        ));
    }

    let provided_public_key = canonicalize_public_key(&public_key)?;
    let stored_public_key = canonicalize_public_key(&device.public_jwk)?;
    if provided_public_key != stored_public_key {
        return Err(Error::unauthorized(
            "x-auth-public-key does not match bound device key",
        ));
    }

    let body_str = std::str::from_utf8(body)
        .map_err(|_| Error::bad_request("INVALID_BODY", "Body must be utf-8"))?;

    let canonical_payload = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        timestamp,
        nonce,
        method.as_str().to_uppercase(),
        path,
        body_str,
        provided_public_key,
        device_id,
        user_id_hint.as_deref().unwrap_or_default(),
    );
    let digest = Sha256::digest(canonical_payload.as_bytes());
    let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);

    if signature != expected {
        return Err(Error::unauthorized("Invalid signature"));
    }

    Ok((device.user_id, device.device_id))
}

fn verify_nonce(
    device_id: &str,
    nonce: &str,
    timestamp: i64,
    skew_seconds: i64,
) -> Result<(), Error> {
    let now = Utc::now().timestamp();
    let cutoff = now - skew_seconds.max(1);
    let nonce_key = format!("{device_id}:{nonce}");

    let cache = NONCE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = cache
        .lock()
        .map_err(|_| Error::internal("NONCE_CACHE_LOCK_FAILED", "failed to lock nonce cache"))?;

    entries.retain(|_, seen_at| *seen_at >= cutoff);
    if entries.contains_key(&nonce_key) {
        return Err(Error::unauthorized("Nonce already used"));
    }

    entries.insert(nonce_key, timestamp);
    Ok(())
}

fn canonicalize_public_key(raw: &str) -> Result<String, Error> {
    let parsed: serde_json::Value = serde_json::from_str(raw)
        .map_err(|_| Error::unauthorized("x-auth-public-key must be valid JSON"))?;
    let object = parsed
        .as_object()
        .ok_or_else(|| Error::unauthorized("x-auth-public-key must be a JSON object"))?;

    let sorted = object
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<String, serde_json::Value>>();

    serde_json::to_string(&sorted)
        .map_err(|error| Error::internal("PUBLIC_KEY_SERIALIZATION_FAILED", error.to_string()))
}

fn header_value(headers: &HeaderMap, key: &str) -> Option<String> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}
