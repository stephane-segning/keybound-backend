use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header::AUTHORIZATION};
use axum::response::{IntoResponse, Response};
use backend_core::{BffAuth, KcAuth, StaffAuth};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::hmac;
use ring::signature::{ECDSA_P256_SHA256_FIXED, UnparsedPublicKey};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

pub async fn require_kc_signature(
    cfg: &KcAuth,
    req: Request<Body>,
) -> std::result::Result<Request<Body>, Response> {
    if !cfg.enabled {
        return Ok(req);
    }

    let timestamp_header = match header_value(req.headers(), "x-kc-timestamp") {
        Some(value) => value,
        None => return Err(unauthorized("missing x-kc-timestamp")),
    };
    let signature_header = match header_value(req.headers(), "x-kc-signature") {
        Some(value) => value,
        None => return Err(unauthorized("missing x-kc-signature")),
    };
    if is_timestamp_invalid(&timestamp_header, cfg.max_clock_skew_seconds) {
        return Err(unauthorized("invalid x-kc-timestamp"));
    }

    let method = req.method().as_str().to_uppercase();
    let path = req.uri().path().to_owned();
    let (parts, body) = req.into_parts();
    let body_bytes = match to_bytes(body, cfg.max_body_bytes).await {
        Ok(value) => value,
        Err(_) => return Err(unauthorized("invalid request body")),
    };
    let body_str = String::from_utf8_lossy(&body_bytes);
    let payload = format!("{timestamp_header}\n{method}\n{path}\n{body_str}");

    let key = hmac::Key::new(hmac::HMAC_SHA256, cfg.signature_secret.as_bytes());
    let digest = hmac::sign(&key, payload.as_bytes());
    let expected = URL_SAFE_NO_PAD.encode(digest.as_ref());
    if expected != signature_header {
        return Err(unauthorized("invalid x-kc-signature"));
    }

    Ok(Request::from_parts(parts, Body::from(body_bytes)))
}

pub async fn require_bff_auth(
    cfg: &BffAuth,
    req: Request<Body>,
) -> std::result::Result<Request<Body>, Response> {
    if !cfg.enabled {
        return Ok(req);
    }

    let token = if cfg.require_bearer {
        match bearer_token(req.headers()) {
            Some(value) => value,
            None => return Err(unauthorized("missing bearer token")),
        }
    } else {
        bearer_token(req.headers()).unwrap_or_default()
    };

    let claims = if token.is_empty() {
        None
    } else {
        match decode_jwt_claims(&token) {
            Some(value) => Some(value),
            None => return Err(unauthorized("invalid bearer token")),
        }
    };

    if cfg.require_signature {
        let signature_header = match header_value(req.headers(), "x-signature") {
            Some(value) => value,
            None => return Err(unauthorized("missing x-signature")),
        };
        let timestamp_header = match header_value(req.headers(), "x-signature-timestamp") {
            Some(value) => value,
            None => return Err(unauthorized("missing x-signature-timestamp")),
        };
        let public_key_header = match header_value(req.headers(), "x-public-key") {
            Some(value) => value,
            None => return Err(unauthorized("missing x-public-key")),
        };
        if is_timestamp_invalid(&timestamp_header, cfg.max_clock_skew_seconds) {
            return Err(unauthorized("invalid x-signature-timestamp"));
        }

        let Some(claims) = claims.as_ref() else {
            return Err(unauthorized("missing bearer token claims"));
        };
        let Some(expected_jkt) = claims.jkt.as_ref() else {
            return Err(unauthorized("missing cnf.jkt"));
        };

        let public_jwk: Value = match serde_json::from_str(&public_key_header) {
            Ok(value) => value,
            Err(_) => return Err(unauthorized("invalid x-public-key")),
        };

        let computed_jkt = match compute_jkt_thumbprint(&public_jwk) {
            Some(value) => value,
            None => return Err(unauthorized("invalid x-public-key thumbprint")),
        };
        if &computed_jkt != expected_jkt {
            return Err(unauthorized("x-public-key does not match cnf.jkt"));
        }
        if !verify_bff_signature(&req, &timestamp_header, &signature_header, &public_jwk) {
            return Err(unauthorized("invalid x-signature"));
        }
    }

    Ok(req)
}

pub async fn require_staff_bearer(
    cfg: &StaffAuth,
    req: Request<Body>,
) -> std::result::Result<Request<Body>, Response> {
    if cfg.require_bearer && bearer_token(req.headers()).is_none() {
        return Err(unauthorized("missing bearer token"));
    }
    Ok(req)
}

#[derive(Debug, Clone)]
struct JwtClaims {
    jkt: Option<String>,
}

fn decode_jwt_claims(token: &str) -> Option<JwtClaims> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let payload = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let payload: Value = serde_json::from_slice(&payload).ok()?;
    let jkt = payload
        .get("cnf")
        .and_then(|value| value.get("jkt"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    Some(JwtClaims { jkt })
}

fn verify_bff_signature(req: &Request<Body>, timestamp: &str, signature: &str, public_jwk: &Value) -> bool {
    let x = public_jwk.get("x").and_then(Value::as_str);
    let y = public_jwk.get("y").and_then(Value::as_str);
    let (Some(x), Some(y)) = (x, y) else {
        return false;
    };

    let x = match URL_SAFE_NO_PAD.decode(x) {
        Ok(v) if v.len() == 32 => v,
        _ => return false,
    };
    let y = match URL_SAFE_NO_PAD.decode(y) {
        Ok(v) if v.len() == 32 => v,
        _ => return false,
    };
    let signature = match URL_SAFE_NO_PAD.decode(signature) {
        Ok(v) if v.len() == 64 => v,
        _ => return false,
    };

    let mut pk = [0u8; 65];
    pk[0] = 0x04;
    pk[1..33].copy_from_slice(&x);
    pk[33..65].copy_from_slice(&y);

    let method = req.method().as_str().to_uppercase();
    let path = req.uri().path();
    let query = req.uri().query().unwrap_or("");
    let payload = format!("{method}\n{path}\n{query}\n{timestamp}");

    let verifier = UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, pk);
    verifier.verify(payload.as_bytes(), &signature).is_ok()
}

fn compute_jkt_thumbprint(jwk: &Value) -> Option<String> {
    let kty = jwk.get("kty")?.as_str()?;
    let crv = jwk.get("crv")?.as_str()?;
    let x = jwk.get("x")?.as_str()?;
    let y = jwk.get("y")?.as_str()?;
    if kty != "EC" || crv != "P-256" {
        return None;
    }
    let canonical = format!(r#"{{"crv":"P-256","kty":"EC","x":"{x}","y":"{y}"}}"#);
    let hash = Sha256::digest(canonical.as_bytes());
    Some(URL_SAFE_NO_PAD.encode(hash))
}

fn bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let mut parts = value.splitn(2, ' ');
    let scheme = parts.next()?;
    let token = parts.next()?;
    if scheme.eq_ignore_ascii_case("bearer") && !token.is_empty() {
        Some(token.to_owned())
    } else {
        None
    }
}

fn header_value(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

fn is_timestamp_invalid(timestamp: &str, max_clock_skew_seconds: i64) -> bool {
    let Ok(ts) = timestamp.parse::<i64>() else {
        return true;
    };
    let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return true;
    };
    (now.as_secs() as i64 - ts).abs() > max_clock_skew_seconds
}

fn unauthorized(message: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        axum::Json(serde_json::json!({
            "error": "unauthorized",
            "message": message
        })),
    )
        .into_response()
}
