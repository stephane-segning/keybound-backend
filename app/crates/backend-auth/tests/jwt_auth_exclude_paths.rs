use async_trait::async_trait;
use axum::Router;
use axum::body::Body;
use axum::http::{HeaderValue, Request, StatusCode};
use axum::routing::get;
use axum::{body::to_bytes, response::Response};
use backend_auth::{JwksProvider, jwks_auth_layer, kc_signature_layer, require_kc_signature};
use backend_core::KcAuth;
use base64::Engine;
use jwks::Jwks;
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

#[derive(Clone)]
struct MockJwksProvider;

#[async_trait]
impl JwksProvider for MockJwksProvider {
    async fn get_jwks(&self, _url: &str) -> Result<Jwks, String> {
        // Return a dummy JWKS or error as needed for tests
        // For now, we can return an error to simulate failure or empty JWKS
        Err("mock error".to_string())
    }
}

#[tokio::test]
async fn kc_signature_bypasses_when_disabled() {
    let mut cfg = build_kc_auth();
    cfg.enabled = false;
    let request = Request::builder()
        .uri("/v1/users")
        .body(Body::empty())
        .unwrap();

    let result = require_kc_signature(&cfg, request).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn kc_signature_rejects_when_timestamp_header_is_missing() {
    let cfg = build_kc_auth();
    let request = Request::builder()
        .uri("/v1/users")
        .body(Body::empty())
        .unwrap();

    let result = require_kc_signature(&cfg, request).await;

    assert!(result.is_err());
    let payload = read_error_body(result.err().unwrap()).await;
    assert_eq!(payload["message"], "missing x-kc-timestamp");
}

#[tokio::test]
async fn kc_signature_rejects_when_signature_header_is_missing() {
    let cfg = build_kc_auth();
    let mut request = Request::builder()
        .uri("/v1/users")
        .body(Body::empty())
        .unwrap();
    let timestamp = now_unix_seconds().to_string();
    request
        .headers_mut()
        .insert("x-kc-timestamp", HeaderValue::from_str(&timestamp).unwrap());

    let result = require_kc_signature(&cfg, request).await;

    assert!(result.is_err());
    let payload = read_error_body(result.err().unwrap()).await;
    assert_eq!(payload["message"], "missing x-kc-signature");
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

    let result = require_kc_signature(&cfg, request).await;

    assert!(result.is_err());
    let payload = read_error_body(result.err().unwrap()).await;
    assert_eq!(payload["message"], "invalid x-kc-timestamp");
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

    let result = require_kc_signature(&cfg, request).await;

    assert!(result.is_err());
    let payload = read_error_body(result.err().unwrap()).await;
    assert_eq!(payload["message"], "invalid x-kc-timestamp");
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

    let result = require_kc_signature(&cfg, request).await;

    assert!(result.is_err());
    let payload = read_error_body(result.err().unwrap()).await;
    assert_eq!(payload["message"], "invalid x-kc-signature");
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

    let result = require_kc_signature(&cfg, request).await;

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

    let result = require_kc_signature(&cfg, request).await;

    assert!(result.is_ok());
    let request = result.unwrap();
    let bytes = to_bytes(request.into_body(), usize::MAX).await.unwrap();
    assert_eq!(String::from_utf8(bytes.to_vec()).unwrap(), body);
}

#[tokio::test]
async fn kc_signature_layer_rejects_requests_without_headers() {
    let router = Router::new()
        .route("/v1/users", get(|| async { "ok" }))
        .layer(kc_signature_layer(build_kc_auth()));

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
async fn jwks_auth_layer_enforces_when_path_matches_base_path() {
    let jwks_url = "http://localhost/jwks".to_string();
    let base_paths = vec!["/api/registration".to_string()];
    let router = Router::new()
        .route("/api/registration/users", get(|| async { "ok" }))
        .layer(
            jwks_auth_layer(jwks_url, base_paths)
                .with_provider(Box::new(MockJwksProvider) as Box<dyn JwksProvider>),
        );

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
    let jwks_url = "http://localhost/jwks".to_string();
    let base_paths = vec!["/api/registration".to_string()];
    let router = Router::new()
        .route("/public/info", get(|| async { "ok" }))
        .layer(
            jwks_auth_layer(jwks_url, base_paths)
                .with_provider(Box::new(MockJwksProvider) as Box<dyn JwksProvider>),
        );

    let request = Request::builder()
        .uri("/public/info")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn jwks_auth_layer_uses_default_paths_when_empty() {
    let jwks_url = "http://localhost/jwks".to_string();
    let base_paths = vec![];
    let router = Router::new()
        .route("/api/registration/users", get(|| async { "ok" }))
        .layer(
            jwks_auth_layer(jwks_url, base_paths)
                .with_provider(Box::new(MockJwksProvider) as Box<dyn JwksProvider>),
        );

    let request = Request::builder()
        .uri("/api/registration/users")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    // Should be unauthorized because default paths include /api/registration
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
