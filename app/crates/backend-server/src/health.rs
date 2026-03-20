//! Health check endpoint for monitoring service status.
//!
//! Provides a simple HTTP endpoint that returns 200 OK when the service is running.

use axum::Router;
use axum::routing::get;
use hyper::StatusCode;

/// Creates a router with the health check endpoint.
///
/// The health endpoint responds to GET /health requests with HTTP 200 OK.
/// This is used by load balancers and monitoring systems to verify service availability.
///
/// # Returns
/// Configured Router with health endpoint
pub fn health_router() -> Router {
    Router::new().route("/health", get(health_handler))
}

/// Health check handler that returns OK status.
///
/// This handler is intentionally simple and fast - it only confirms the service is running.
/// Deeper health checks (database, Redis, etc.) should be implemented separately if needed.
///
/// # Returns
/// HTTP 200 OK status code
async fn health_handler() -> StatusCode {
    StatusCode::OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn health_endpoint_returns_ok() {
        let app = health_router();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("request should be valid"),
            )
            .await
            .expect("router should handle request");

        assert_eq!(response.status(), StatusCode::OK);
    }
}
