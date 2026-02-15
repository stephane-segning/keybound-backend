use axum::body::Body;
use axum::http::{header::AUTHORIZATION, HeaderValue, Request};
use base64::Engine;
use backend_auth::{require_bff_auth, require_staff_bearer};
use backend_core::{BffAuth, StaffAuth};

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

fn valid_bearer_token() -> String {
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(r#"{"sub":"usr_test"}"#.as_bytes());
    format!("Bearer header.{payload}.signature")
}

#[tokio::test]
async fn bypasses_validation_for_path_outside_bff_base_path() {
    let cfg = build_bff_auth();
    let request = Request::builder().uri("/kc/users").body(Body::empty()).unwrap();

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
}

#[tokio::test]
async fn accepts_bearer_token_for_bff_base_path() {
    let cfg = build_bff_auth();
    let mut request = Request::builder()
        .uri("/api/registration/users")
        .body(Body::empty())
        .unwrap();
    request
        .headers_mut()
        .insert(AUTHORIZATION, HeaderValue::from_str(&valid_bearer_token()).unwrap());

    let result = require_bff_auth(&cfg, request).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn staff_auth_bypasses_when_path_is_outside_staff_base_path() {
    let cfg = build_staff_auth();
    let request = Request::builder().uri("/kc/realm").body(Body::empty()).unwrap();

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
}
