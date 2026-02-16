mod middleware;

use base64::Engine;
use http::Request;
use std::collections::BTreeSet;
use swagger::auth::{Authorization, Scopes};
use swagger::{Has, XSpanIdString};

pub use middleware::{JwksProvider, jwks_auth_layer, kc_signature_layer, require_kc_signature};

#[derive(Debug, Clone)]
pub struct KcContext {
    x_span_id: XSpanIdString,
}

impl KcContext {
    pub fn from_request<B>(req: &Request<B>) -> Self {
        Self {
            x_span_id: XSpanIdString::get_or_generate(req),
        }
    }
}

impl Has<XSpanIdString> for KcContext {
    fn get(&self) -> &XSpanIdString {
        &self.x_span_id
    }

    fn get_mut(&mut self) -> &mut XSpanIdString {
        &mut self.x_span_id
    }

    fn set(&mut self, value: XSpanIdString) {
        self.x_span_id = value;
    }
}

#[derive(Debug, Clone)]
pub struct ServiceContext {
    x_span_id: XSpanIdString,
    authorization: Option<Authorization>,
    user_id: Option<String>,
}

impl ServiceContext {
    pub fn from_request<B>(req: &Request<B>) -> Self {
        Self {
            x_span_id: XSpanIdString::get_or_generate(req),
            authorization: Some(dummy_authorization()),
            user_id: bearer_subject(req),
        }
    }

    pub fn user_id(&self) -> Option<&str> {
        self.user_id.as_deref()
    }
}

impl Has<XSpanIdString> for ServiceContext {
    fn get(&self) -> &XSpanIdString {
        &self.x_span_id
    }

    fn get_mut(&mut self) -> &mut XSpanIdString {
        &mut self.x_span_id
    }

    fn set(&mut self, value: XSpanIdString) {
        self.x_span_id = value;
    }
}

impl Has<Option<Authorization>> for ServiceContext {
    fn get(&self) -> &Option<Authorization> {
        &self.authorization
    }

    fn get_mut(&mut self) -> &mut Option<Authorization> {
        &mut self.authorization
    }

    fn set(&mut self, value: Option<Authorization>) {
        self.authorization = value;
    }
}

fn dummy_authorization() -> Authorization {
    Authorization {
        subject: "authenticated".to_owned(),
        scopes: Scopes::Some(BTreeSet::new()),
        issuer: None,
    }
}

fn bearer_subject<B>(req: &Request<B>) -> Option<String> {
    let value = req
        .headers()
        .get(http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let token = value.strip_prefix("Bearer ")?;
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let payload: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    payload.get("sub")?.as_str().map(ToOwned::to_owned)
}
