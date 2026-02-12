use http::Request;
use std::collections::BTreeSet;
use swagger::auth::{Authorization, Scopes};
use swagger::{Has, XSpanIdString};

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
}

impl ServiceContext {
    pub fn from_request<B>(req: &Request<B>) -> Self {
        Self {
            x_span_id: XSpanIdString::get_or_generate(req),
            authorization: Some(dummy_authorization()),
        }
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
        subject: "anonymous".to_owned(),
        scopes: Scopes::Some(BTreeSet::new()),
        issuer: None,
    }
}
