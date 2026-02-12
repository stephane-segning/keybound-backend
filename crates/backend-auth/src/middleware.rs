use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use backend_id::prefixed;

#[derive(Debug, Clone)]
pub struct RequestContext {
    pub request_id: String,
    pub authorization: Option<String>,
}

pub async fn attach_request_context(mut req: Request, next: Next) -> Response {
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
        .or_else(|| prefixed("req").ok())
        .unwrap_or_else(|| "req_unknown".to_owned());

    let authorization = req
        .headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);

    req.extensions_mut().insert(RequestContext {
        request_id,
        authorization,
    });

    next.run(req).await
}
