use axum::Router;
use axum::body::Body;
use axum::http::{HeaderValue, Request, StatusCode};
use axum::routing::get;
use axum::{body::to_bytes, response::Response};
use backend_auth::{
    HttpClient, OidcState, SignatureState, jwks_auth_layer, kc_signature_layer,
    require_kc_signature,
};
use backend_core::KcAuth;
use base64::Engine;
use ring::hmac;
use serde_json::Value;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

fn build_kc_auth() -> KcAuth {
    KcAuth {
        enabled: true,
        base_path: "/v1".to_owned(),
        signature_secret: "test-secret".to_owned(),
        max_clock_skew_seconds: 120,
        max_body_bytes: 1024,
    }
}

fn build_signature_state(cfg: &KcAuth) -> SignatureState {
    SignatureState {
        signature_secret: cfg.signature_secret.clone(),
        max_clock_skew_seconds: cfg.max_clock_skew_seconds,
        max_body_bytes: cfg.max_body_bytes,
    }
}

async fn read_error_body(response: Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn kc_signature(secret: &str, timestamp: i64, method: &str, path: &str, body: &str) -> String {
    let payload = format!("{timestamp}\n{}\n{path}\n{body}", method.to_uppercase());
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let digest = hmac::sign(&key, payload.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest.as_ref())
}

fn build_oidc_state() -> Arc<OidcState> {
    let http_client = HttpClient::new_with_defaults().unwrap();
    Arc::new(OidcState::new(
        "http://localhost".to_string(),
        None,
        std::time::Duration::from_secs(3600),
        std::time::Duration::from_secs(3600),
        http_client,
    ))
}

#[tokio::test]
async fn kc_signature_bypasses_when_disabled() {
    let mut cfg = build_kc_auth();
    cfg.enabled = false;
    let request = Request::builder()
        .uri("/v1/users")
        .body(Body::empty())
        .unwrap();

    let state = build_signature_state(&cfg);
    let result = require_kc_signature(cfg.enabled, &state, request).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn kc_signature_rejects_when_timestamp_header_is_missing() {
    let cfg = build_kc_auth();
    let mut request = Request::builder()
        .uri("/v1/users")
        .body(Body::empty())
        .unwrap();
    // Signature is checked first in implementation
    request
        .headers_mut()
        .insert("x-kc-signature", HeaderValue::from_static("any"));

    let state = build_signature_state(&cfg);
    let result = require_kc_signature(cfg.enabled, &state, request).await;

    assert!(result.is_err());
    let payload = read_error_body(result.err().unwrap()).await;
    assert_eq!(payload["message"], "Missing x-kc-timestamp");
}

#[tokio::test]
async fn kc_signature_rejects_when_signature_header_is_missing() {
    let cfg = build_kc_auth();
    let request = Request::builder()
        .uri("/v1/users")
        .body(Body::empty())
        .unwrap();

    let state = build_signature_state(&cfg);
    let result = require_kc_signature(cfg.enabled, &state, request).await;

    assert!(result.is_err());
    let payload = read_error_body(result.err().unwrap()).await;
    assert_eq!(payload["message"], "Missing x-kc-signature");
}

#[tokio::test]
async fn kc_signature_rejects_when_timestamp_is_invalid() {
    let cfg = build_kc_auth();
    let mut request = Request::builder()
        .uri("/v1/users")
        .body(Body::empty())
        .unwrap();
    request
        .headers_mut()
        .insert("x-kc-timestamp", HeaderValue::from_static("invalid"));
    request
        .headers_mut()
        .insert("x-kc-signature", HeaderValue::from_static("any"));

    let state = build_signature_state(&cfg);
    let result = require_kc_signature(cfg.enabled, &state, request).await;

    assert!(result.is_err());
    let payload = read_error_body(result.err().unwrap()).await;
    assert_eq!(payload["message"], "Invalid x-kc-timestamp");
}

#[tokio::test]
async fn kc_signature_rejects_when_timestamp_is_outside_allowed_skew() {
    let mut cfg = build_kc_auth();
    cfg.max_clock_skew_seconds = 1;
    let mut request = Request::builder()
        .uri("/v1/users")
        .body(Body::empty())
        .unwrap();
    let stale = (now_unix_seconds() - 120).to_string();
    request
        .headers_mut()
        .insert("x-kc-timestamp", HeaderValue::from_str(&stale).unwrap());
    request
        .headers_mut()
        .insert("x-kc-signature", HeaderValue::from_static("any"));

    let state = build_signature_state(&cfg);
    let result = require_kc_signature(cfg.enabled, &state, request).await;

    assert!(result.is_err());
    let payload = read_error_body(result.err().unwrap()).await;
    assert_eq!(payload["message"], "Timestamp out of skew");
}

#[tokio::test]
async fn kc_signature_rejects_when_signature_is_invalid() {
    let cfg = build_kc_auth();
    let timestamp = now_unix_seconds().to_string();
    let mut request = Request::builder()
        .method("POST")
        .uri("/v1/users")
        .body(Body::from("{\"hello\":\"world\"}"))
        .unwrap();
    request
        .headers_mut()
        .insert("x-kc-timestamp", HeaderValue::from_str(&timestamp).unwrap());
    request.headers_mut().insert(
        "x-kc-signature",
        HeaderValue::from_static("invalid-signature"),
    );

    let state = build_signature_state(&cfg);
    let result = require_kc_signature(cfg.enabled, &state, request).await;

    assert!(result.is_err());
    let payload = read_error_body(result.err().unwrap()).await;
    assert_eq!(payload["message"], "Invalid signature");
}

#[tokio::test]
async fn kc_signature_rejects_when_body_exceeds_limit() {
    let mut cfg = build_kc_auth();
    cfg.max_body_bytes = 2;
    let timestamp = now_unix_seconds();
    let body = "abc";
    let signature = kc_signature(&cfg.signature_secret, timestamp, "POST", "/v1/users", body);
    let mut request = Request::builder()
        .method("POST")
        .uri("/v1/users")
        .body(Body::from(body))
        .unwrap();
    request.headers_mut().insert(
        "x-kc-timestamp",
        HeaderValue::from_str(&timestamp.to_string()).unwrap(),
    );
    request
        .headers_mut()
        .insert("x-kc-signature", HeaderValue::from_str(&signature).unwrap());

    let state = build_signature_state(&cfg);
    let result = require_kc_signature(cfg.enabled, &state, request).await;

    assert!(result.is_err());
    let payload = read_error_body(result.err().unwrap()).await;
    assert_eq!(payload["message"], "invalid request body");
}

#[tokio::test]
async fn kc_signature_accepts_valid_signature_and_preserves_body() {
    let cfg = build_kc_auth();
    let timestamp = now_unix_seconds();
    let body = "{\"hello\":\"world\"}";
    let signature = kc_signature(&cfg.signature_secret, timestamp, "POST", "/v1/users", body);
    let mut request = Request::builder()
        .method("POST")
        .uri("/v1/users")
        .body(Body::from(body))
        .unwrap();
    request.headers_mut().insert(
        "x-kc-timestamp",
        HeaderValue::from_str(&timestamp.to_string()).unwrap(),
    );
    request
        .headers_mut()
        .insert("x-kc-signature", HeaderValue::from_str(&signature).unwrap());

    let state = build_signature_state(&cfg);
    let result = require_kc_signature(cfg.enabled, &state, request).await;

    assert!(result.is_ok());
    let request = result.unwrap();
    let bytes = to_bytes(request.into_body(), usize::MAX).await.unwrap();
    assert_eq!(String::from_utf8(bytes.to_vec()).unwrap(), body);
}

#[tokio::test]
async fn kc_signature_layer_rejects_requests_without_headers() {
    let cfg = build_kc_auth();
    let state = Arc::new(build_signature_state(&cfg));
    let router = Router::new()
        .route("/v1/users", get(|| async { "ok" }))
        .layer(kc_signature_layer(cfg.enabled, state));

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/users")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn kc_signature_handles_url_encoded_paths() {
    let cfg = build_kc_auth();
    let timestamp = now_unix_seconds();
    let body = "";
    // The client sends encoded path.
    // Note: In a real HTTP request, the path on the wire is encoded.
    // When constructing Request::builder().uri("/v1/users/foo%20bar"), the URI stores it as is.
    let path = "/v1/users/foo%20bar";
    let signature = kc_signature(&cfg.signature_secret, timestamp, "GET", path, body);

    let mut request = Request::builder()
        .method("GET")
        .uri(path)
        .body(Body::empty())
        .unwrap();

    request.headers_mut().insert(
        "x-kc-timestamp",
        HeaderValue::from_str(&timestamp.to_string()).unwrap(),
    );
    request
        .headers_mut()
        .insert("x-kc-signature", HeaderValue::from_str(&signature).unwrap());

    let state = build_signature_state(&cfg);
    let result = require_kc_signature(cfg.enabled, &state, request).await;

    assert!(
        result.is_ok(),
        "Signature verification failed for encoded path"
    );
}

#[tokio::test]
async fn kc_signature_works_with_nested_router() {
    let cfg = build_kc_auth();
    let timestamp = now_unix_seconds();
    let body = "";
    let full_path = "/v1/nested/users";
    let signature = kc_signature(&cfg.signature_secret, timestamp, "GET", full_path, body);

    let state = Arc::new(build_signature_state(&cfg));
    let router = Router::new().nest(
        "/v1/nested",
        Router::new()
            .route("/users", get(|| async { "ok" }))
            .layer(kc_signature_layer(cfg.enabled, state)),
    );

    let mut request = Request::builder()
        .method("GET")
        .uri(full_path)
        .body(Body::empty())
        .unwrap();

    request.headers_mut().insert(
        "x-kc-timestamp",
        HeaderValue::from_str(&timestamp.to_string()).unwrap(),
    );
    request
        .headers_mut()
        .insert("x-kc-signature", HeaderValue::from_str(&signature).unwrap());

    let response = router.oneshot(request).await.unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Signature verification failed for nested router"
    );
}

#[tokio::test]
async fn kc_signature_rejects_when_method_mismatch() {
    let cfg = build_kc_auth();
    let timestamp = now_unix_seconds();
    let body = "";
    let path = "/v1/users";
    // Sign for POST
    let signature = kc_signature(&cfg.signature_secret, timestamp, "POST", path, body);

    // Request is GET
    let mut request = Request::builder()
        .method("GET")
        .uri(path)
        .body(Body::empty())
        .unwrap();

    request.headers_mut().insert(
        "x-kc-timestamp",
        HeaderValue::from_str(&timestamp.to_string()).unwrap(),
    );
    request
        .headers_mut()
        .insert("x-kc-signature", HeaderValue::from_str(&signature).unwrap());

    let state = build_signature_state(&cfg);
    let result = require_kc_signature(cfg.enabled, &state, request).await;

    assert!(result.is_err());
    let payload = read_error_body(result.err().unwrap()).await;
    assert_eq!(payload["message"], "Invalid signature");
}

#[tokio::test]
async fn kc_signature_rejects_when_path_mismatch() {
    let cfg = build_kc_auth();
    let timestamp = now_unix_seconds();
    let body = "";
    // Sign for /v1/other
    let signature = kc_signature(&cfg.signature_secret, timestamp, "GET", "/v1/other", body);

    // Request is /v1/users
    let mut request = Request::builder()
        .method("GET")
        .uri("/v1/users")
        .body(Body::empty())
        .unwrap();

    request.headers_mut().insert(
        "x-kc-timestamp",
        HeaderValue::from_str(&timestamp.to_string()).unwrap(),
    );
    request
        .headers_mut()
        .insert("x-kc-signature", HeaderValue::from_str(&signature).unwrap());

    let state = build_signature_state(&cfg);
    let result = require_kc_signature(cfg.enabled, &state, request).await;

    assert!(result.is_err());
    let payload = read_error_body(result.err().unwrap()).await;
    assert_eq!(payload["message"], "Invalid signature");
}

#[tokio::test]
async fn kc_signature_rejects_when_body_mismatch() {
    let cfg = build_kc_auth();
    let timestamp = now_unix_seconds();
    // Sign for "foo"
    let signature = kc_signature(&cfg.signature_secret, timestamp, "POST", "/v1/users", "foo");

    // Request has "bar"
    let mut request = Request::builder()
        .method("POST")
        .uri("/v1/users")
        .body(Body::from("bar"))
        .unwrap();

    request.headers_mut().insert(
        "x-kc-timestamp",
        HeaderValue::from_str(&timestamp.to_string()).unwrap(),
    );
    request
        .headers_mut()
        .insert("x-kc-signature", HeaderValue::from_str(&signature).unwrap());

    let state = build_signature_state(&cfg);
    let result = require_kc_signature(cfg.enabled, &state, request).await;

    assert!(result.is_err());
    let payload = read_error_body(result.err().unwrap()).await;
    assert_eq!(payload["message"], "Invalid signature");
}

#[tokio::test]
async fn jwks_auth_layer_enforces_when_path_matches_base_path() {
    let oidc_state = build_oidc_state();
    let base_paths = vec!["/api/registration".to_string()];
    let router = Router::new()
        .route("/api/registration/users", get(|| async { "ok" }))
        .layer(jwks_auth_layer(oidc_state, base_paths));

    let request = Request::builder()
        .uri("/api/registration/users")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    // Should be unauthorized because no token provided and path matches base-paths
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn jwks_auth_layer_bypasses_when_path_does_not_match_base_path() {
    let oidc_state = build_oidc_state();
    let base_paths = vec!["/api/registration".to_string()];
    let router = Router::new()
        .route("/public/info", get(|| async { "ok" }))
        .layer(jwks_auth_layer(oidc_state, base_paths));

    let request = Request::builder()
        .uri("/public/info")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn jwks_auth_layer_bypasses_when_empty_base_paths() {
    let oidc_state = build_oidc_state();
    let base_paths = vec![];
    let router = Router::new()
        .route("/api/registration/users", get(|| async { "ok" }))
        .layer(jwks_auth_layer(oidc_state, base_paths));

    let request = Request::builder()
        .uri("/api/registration/users")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    // Should be OK because empty base_paths means no protection at the layer level
    assert_eq!(response.status(), StatusCode::OK);
}
