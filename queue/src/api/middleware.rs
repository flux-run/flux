//! Auth middleware for the Queue HTTP API.
//!
//! All `/jobs` routes require a valid `X-Service-Token` header matching
//! `INTERNAL_SERVICE_TOKEN` (or `SERVICE_TOKEN` as a fallback).
//!
//! `/health` and `/version` are explicitly excluded so orchestration probes
//! continue to work without credentials.

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use tracing::warn;

/// Require a valid service token on non-probe routes.
///
/// Paths starting with `/health` or `/version` are allowed through
/// unconditionally.  All other paths must supply the correct
/// `X-Service-Token` (or `x-service-token`) header.
pub async fn require_service_token(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_lowercase();

    // Let health / version probes through without auth.
    if path.starts_with("/health") || path.starts_with("/version") {
        return next.run(req).await;
    }

    let expected = std::env::var("INTERNAL_SERVICE_TOKEN")
        .or_else(|_| std::env::var("SERVICE_TOKEN"))
        .unwrap_or_else(|_| {
            if std::env::var("FLUX_ENV").as_deref() == Ok("production") {
                panic!(
                    "[Flux] INTERNAL_SERVICE_TOKEN must be set in production. \
                     The queue service cannot start without it."
                );
            }
            warn!(
                "[Flux] INTERNAL_SERVICE_TOKEN not set — using insecure default \
                 'stub_token'. Set this env var in production."
            );
            "stub_token".to_string()
        });

    let provided = req
        .headers()
        .get("x-service-token")
        .or_else(|| req.headers().get("X-Service-Token"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim();

    if provided != expected {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error":   "invalid_service_token",
                "message": "Queue endpoints require a valid X-Service-Token header"
            })),
        )
            .into_response();
    }

    next.run(req).await
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
        middleware::from_fn,
        routing::get,
        Router,
    };
    use tower::util::ServiceExt;

    use super::require_service_token;
    use api_contract::routes as R;

    fn app() -> Router {
        Router::new()
            .route(R::jobs::LIST.path,      get(|| async { "jobs" }))
            .route(R::health::HEALTH.path,  get(|| async { "ok" }))
            .route(R::health::VERSION.path, get(|| async { "v" }))
            .layer(from_fn(require_service_token))
    }

    #[tokio::test]
    async fn health_passes_without_token() {
        let resp = app()
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn version_passes_without_token() {
        let resp = app()
            .oneshot(Request::builder().uri("/version").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn jobs_without_token_is_401() {
        let resp = app()
            .oneshot(Request::builder().uri("/jobs").body(Body::empty()).unwrap())
            .await
            .unwrap();
        // No INTERNAL_SERVICE_TOKEN env — defaults to "stub_token"; no token provided → 401.
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn jobs_with_correct_token_passes() {
        // The middleware defaults to "stub_token" when env var is absent.
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/jobs")
                    .header("x-service-token", "stub_token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn jobs_with_wrong_token_is_401() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/jobs")
                    .header("x-service-token", "wrong-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let body = to_bytes(resp.into_body(), 512).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "invalid_service_token");
    }

    #[tokio::test]
    async fn jobs_with_uppercase_header_passes() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/jobs")
                    .header("X-Service-Token", "stub_token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
