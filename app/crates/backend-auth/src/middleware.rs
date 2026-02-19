use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::extract::OriginalUri;
use axum::http::{Request, StatusCode, header::AUTHORIZATION};
use axum::response::{IntoResponse, Response};
use backend_core::KcAuth;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::{Validation, decode, decode_header};
use jwks::Jwks;
use ring::hmac;
use serde::Deserialize;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::OnceCell;
use tower::{Layer, Service};
use tracing::error;

pub async fn require_kc_signature(
    cfg: &KcAuth,
    req: Request<Body>,
) -> Result<Request<Body>, Response> {
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
    let path = req
        .extensions()
        .get::<OriginalUri>()
        .map(|uri| uri.0.path().to_owned())
        .unwrap_or_else(|| req.uri().path().to_owned());
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

pub fn kc_signature_layer(cfg: KcAuth) -> KcSignatureLayer {
    KcSignatureLayer::new(cfg)
}

#[async_trait]
pub trait JwksProvider: Send + Sync + 'static {
    async fn get_jwks(&self, url: &str) -> Result<Jwks, String>;
}

#[derive(Clone, Default)]
pub struct DefaultJwksProvider;

#[async_trait]
impl JwksProvider for DefaultJwksProvider {
    async fn get_jwks(&self, url: &str) -> Result<Jwks, String> {
        Jwks::from_jwks_url(url).await.map_err(|e| e.to_string())
    }
}

pub fn jwks_auth_layer(jwks_url: String, base_paths: Vec<String>) -> JwksAuthLayer {
    JwksAuthLayer::new(jwks_url, base_paths)
}

#[derive(Clone)]
pub struct JwksAuthLayer {
    jwks_url: String,
    base_paths: Vec<String>,
    jwks: Arc<OnceCell<Jwks>>,
    provider: Arc<Box<dyn JwksProvider>>,
}

impl JwksAuthLayer {
    pub fn new(jwks_url: String, base_paths: Vec<String>) -> Self {
        Self {
            jwks_url,
            base_paths,
            jwks: Arc::new(OnceCell::new()),
            provider: Arc::new(Box::new(DefaultJwksProvider)),
        }
    }

    pub fn with_provider(mut self, provider: Box<dyn JwksProvider>) -> Self {
        self.provider = Arc::new(provider);
        self
    }
}

impl<S> Layer<S> for JwksAuthLayer {
    type Service = JwksAuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        JwksAuthService {
            inner,
            jwks_url: self.jwks_url.clone(),
            base_paths: self.base_paths.clone(),
            jwks: Arc::clone(&self.jwks),
            provider: Arc::clone(&self.provider),
        }
    }
}

#[derive(Clone)]
pub struct JwksAuthService<S> {
    inner: S,
    jwks_url: String,
    base_paths: Vec<String>,
    jwks: Arc<OnceCell<Jwks>>,
    provider: Arc<Box<dyn JwksProvider>>,
}

impl<S> Service<Request<Body>> for JwksAuthService<S>
where
    S: Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();
        let jwks_url = self.jwks_url.clone();
        let base_paths = self.base_paths.clone();
        let jwks_cell = Arc::clone(&self.jwks);
        let provider = Arc::clone(&self.provider);

        Box::pin(async move {
            let path = req
                .extensions()
                .get::<OriginalUri>()
                .map(|uri| uri.0.path())
                .unwrap_or_else(|| req.uri().path());

            let is_protected = base_paths.iter().any(|p| path.starts_with(p));

            if !is_protected {
                return inner.call(req).await;
            }

            let token = match bearer_token(req.headers()) {
                Some(value) => value,
                None => return Ok(unauthorized("missing bearer token")),
            };

            let jwks = match jwks_cell
                .get_or_try_init(|| async {
                    tracing::info!("Lazy-loading JWKS from {}", jwks_url);
                    provider.get_jwks(&jwks_url).await
                })
                .await
            {
                Ok(jwks) => jwks,
                Err(e) => {
                    error!("failed to load JWKS: {}", e);
                    return Ok(unauthorized("failed to load JWKS"));
                }
            };

            if validate_token(&token, jwks) {
                inner.call(req).await
            } else {
                Ok(unauthorized("invalid bearer token"))
            }
        })
    }
}

fn validate_token(token: &str, jwks: &Jwks) -> bool {
    let header = match decode_header(token) {
        Ok(h) => h,
        Err(e) => {
            error!("decode header error: {:?}", e);
            return false;
        }
    };

    let kid = match header.kid {
        Some(k) => k,
        None => {
            error!("decode header error: kid not found");
            return false;
        }
    };

    let jwk = match jwks.keys.get(&kid) {
        Some(j) => j,
        None => {
            error!("decode header error: jwk");
            return false;
        }
    };

    let mut validation = Validation::new(header.alg);
    validation.validate_aud = false;

    let result = decode::<JwtClaims>(token, &jwk.decoding_key, &validation);

    if let Err(e) = result {
        error!("decode header error: {e}");
        false
    } else {
        true
    }
}

#[derive(Clone)]
pub struct KcSignatureLayer {
    cfg: Arc<KcAuth>,
}

impl KcSignatureLayer {
    fn new(cfg: KcAuth) -> Self {
        Self { cfg: Arc::new(cfg) }
    }
}

impl<S> Layer<S> for KcSignatureLayer {
    type Service = KcSignatureService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        KcSignatureService {
            inner,
            cfg: Arc::clone(&self.cfg),
        }
    }
}

#[derive(Clone)]
pub struct KcSignatureService<S> {
    inner: S,
    cfg: Arc<KcAuth>,
}

impl<S> Service<Request<Body>> for KcSignatureService<S>
where
    S: Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let cfg = Arc::clone(&self.cfg);
        let mut inner = self.inner.clone();
        Box::pin(async move {
            match require_kc_signature(&cfg, req).await {
                Ok(req) => inner.call(req).await,
                Err(resp) => Ok(resp),
            }
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
struct JwtClaims {
    #[allow(dead_code)]
    sub: Option<String>,
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
