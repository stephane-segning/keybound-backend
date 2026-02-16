use axum::Router;
use axum::body::Body;
use axum::http::{HeaderValue, Request, StatusCode, header::AUTHORIZATION};
use axum::routing::get;
use axum::{body::to_bytes, response::Response};
use backend_auth::{require_bff_auth, require_kc_signature, require_staff_bearer};
use backend_core::{BffAuth, KcAuth, StaffAuth};
use base64::Engine;
use ring::hmac;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

fn build_bff_auth() -> BffAuth {
    BffAuth {
        enabled: true,
        base_path: "/api/registration".to_owned(),
    }
}

fn build_staff_auth() -> StaffAuth {
    StaffAuth {
        enabled: true,
        base_path: "/api/kyc/staff".to_owned(),
    }
}

fn build_kc_auth() -> KcAuth {
    KcAuth {
        enabled: true,
        base_path: "/v1".to_owned(),
        signature_secret: "test-secret".to_owned(),
        max_clock_skew_seconds: 120,
        max_body_bytes: 1024,
    }
}

fn valid_bearer_token() -> String {
    let payload =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"sub":"usr_test"}"#.as_bytes());
    format!("Bearer header.{payload}.signature")
}

fn invalid_bearer_token() -> String {
    "Bearer this-is-not-a-jwt".to_owned()
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

#[tokio::test]
async fn bypasses_validation_for_path_outside_bff_base_path() {
    let cfg = build_bff_auth();
    let request = Request::builder()
        .uri("/kc/users")
        .body(Body::empty())
        .unwrap();

    let result = require_bff_auth(&cfg, request).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn enforces_validation_for_bff_base_path_without_token() {
    let cfg = build_bff_auth();
    let request = Request::builder()
        .uri("/api/registration/users")
        .body(Body::empty())
        .unwrap();

    let result = require_bff_auth(&cfg, request).await;

    assert!(result.is_err());

    let response = result.err().unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let payload = read_error_body(response).await;
    assert_eq!(payload["error"], "unauthorized");
    assert_eq!(payload["message"], "missing bearer token");
}

#[tokio::test]
async fn accepts_bearer_token_for_bff_base_path() {
    let cfg = build_bff_auth();
    let mut request = Request::builder()
        .uri("/api/registration/users")
        .body(Body::empty())
        .unwrap();
    request.headers_mut().insert(
        AUTHORIZATION,
        HeaderValue::from_str(&valid_bearer_token()).unwrap(),
    );

    let result = require_bff_auth(&cfg, request).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn bypasses_bff_auth_when_disabled() {
    let cfg = BffAuth {
        enabled: false,
        base_path: "/api/registration".to_owned(),
    };
    let request = Request::builder()
        .uri("/api/registration/users")
        .body(Body::empty())
        .unwrap();

    let result = require_bff_auth(&cfg, request).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn bypasses_bff_auth_when_base_path_is_blank() {
    let cfg = BffAuth {
        enabled: true,
        base_path: "   ".to_owned(),
    };
    let request = Request::builder()
        .uri("/api/registration/users")
        .body(Body::empty())
        .unwrap();

    let result = require_bff_auth(&cfg, request).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn rejects_bff_auth_with_non_bearer_authorization_scheme() {
    let cfg = build_bff_auth();
    let mut request = Request::builder()
        .uri("/api/registration/users")
        .body(Body::empty())
        .unwrap();
    request
        .headers_mut()
        .insert(AUTHORIZATION, HeaderValue::from_static("Basic abc123"));

    let result = require_bff_auth(&cfg, request).await;

    assert!(result.is_err());
    let response = result.err().unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let payload = read_error_body(response).await;
    assert_eq!(payload["message"], "missing bearer token");
}

#[tokio::test]
async fn rejects_bff_auth_with_invalid_bearer_token_payload() {
    let cfg = build_bff_auth();
    let mut request = Request::builder()
        .uri("/api/registration/users")
        .body(Body::empty())
        .unwrap();
    request.headers_mut().insert(
        AUTHORIZATION,
        HeaderValue::from_str(&invalid_bearer_token()).unwrap(),
    );

    let result = require_bff_auth(&cfg, request).await;

    assert!(result.is_err());
    let response = result.err().unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let payload = read_error_body(response).await;
    assert_eq!(payload["message"], "invalid bearer token");
}

#[tokio::test]
async fn accepts_case_insensitive_bearer_scheme_for_bff_auth() {
    let cfg = build_bff_auth();
    let payload =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"sub":"usr_test"}"#.as_bytes());
    let mut request = Request::builder()
        .uri("/api/registration/users")
        .body(Body::empty())
        .unwrap();
    request.headers_mut().insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("bEaReR header.{payload}.signature")).unwrap(),
    );

    let result = require_bff_auth(&cfg, request).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn staff_auth_bypasses_when_path_is_outside_staff_base_path() {
    let cfg = build_staff_auth();
    let request = Request::builder()
        .uri("/kc/realm")
        .body(Body::empty())
        .unwrap();

    let result = require_staff_bearer(&cfg, request).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn staff_auth_enforces_token_for_staff_base_path() {
    let cfg = build_staff_auth();
    let request = Request::builder()
        .uri("/api/kyc/staff/submissions")
        .body(Body::empty())
        .unwrap();

    let result = require_staff_bearer(&cfg, request).await;

    assert!(result.is_err());

    let response = result.err().unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let payload = read_error_body(response).await;
    assert_eq!(payload["error"], "unauthorized");
    assert_eq!(payload["message"], "missing bearer token");
}

#[tokio::test]
async fn staff_auth_accepts_valid_bearer_token_for_staff_base_path() {
    let cfg = build_staff_auth();
    let mut request = Request::builder()
        .uri("/api/kyc/staff/submissions")
        .body(Body::empty())
        .unwrap();
    request.headers_mut().insert(
        AUTHORIZATION,
        HeaderValue::from_str(&valid_bearer_token()).unwrap(),
    );

    let result = require_staff_bearer(&cfg, request).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn staff_auth_rejects_invalid_bearer_token_payload() {
    let cfg = build_staff_auth();
    let mut request = Request::builder()
        .uri("/api/kyc/staff/submissions")
        .body(Body::empty())
        .unwrap();
    request.headers_mut().insert(
        AUTHORIZATION,
        HeaderValue::from_str(&invalid_bearer_token()).unwrap(),
    );

    let result = require_staff_bearer(&cfg, request).await;

    assert!(result.is_err());
    let response = result.err().unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let payload = read_error_body(response).await;
    assert_eq!(payload["message"], "invalid bearer token");
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
async fn bff_bearer_layer_allows_requests_with_token() {
    let cfg = build_bff_auth();
    let router = Router::new()
        .route("/api/registration/users", get(|| async { "ok" }))
        .layer(bff_bearer_layer(cfg.clone()));

    let mut request = Request::builder()
        .uri("/api/registration/users")
        .body(Body::empty())
        .unwrap();
    request.headers_mut().insert(
        AUTHORIZATION,
        HeaderValue::from_str(&valid_bearer_token()).unwrap(),
    );

    let response = router.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn staff_bearer_layer_requires_token_for_protected_path() {
    let cfg = build_staff_auth();
    let router = Router::new()
        .route("/api/kyc/staff/submissions", get(|| async { "ok" }))
        .layer(staff_bearer_layer(cfg));

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/kyc/staff/submissions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
