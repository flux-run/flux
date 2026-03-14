/// Service-to-service authentication middleware.
///
/// Every request to the data-engine must carry the `x-service-token` header
/// matching the `INTERNAL_SERVICE_TOKEN` environment variable. This ensures
/// that even with `--ingress all`, only trusted callers (API, Gateway) can
/// reach the data plane.
///
/// The API and Gateway both inject the token before forwarding requests.
use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};

/// Axum middleware function — rejects requests missing or carrying the wrong
/// `x-service-token` header.
pub async fn require_service_token(req: Request, next: Next) -> Response {
    // Health + version endpoints are exempt so load-balancer probes work.
    let path = req.uri().path().to_owned();
    if path == "/health" || path == "/version" {
        return next.run(req).await;
    }

    let expected = std::env::var("INTERNAL_SERVICE_TOKEN")
        .unwrap_or_else(|_| "fluxbase_secret_token".to_string());

    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_owned();

    let provided = req
        .headers()
        .get("x-service-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();

    if provided == expected {
        tracing::debug!(request_id = %request_id, path = %path, "request authenticated");
        next.run(req).await
    } else {
        tracing::warn!(request_id = %request_id, path = %path, "request rejected: invalid service token");
        (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({
                "error": "unauthorized: missing or invalid x-service-token"
            })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
        middleware::from_fn,
        routing::get,
        Json, Router,
    };
    use tower::util::ServiceExt;

    use super::require_service_token;

    fn app() -> Router {
        Router::new()
            .route("/health", get(|| async { Json(serde_json::json!({ "status": "ok" })) }))
            .route("/db/query", get(|| async { Json(serde_json::json!({ "ok": true })) }))
            .layer(from_fn(require_service_token))
    }

    #[tokio::test]
    async fn health_is_exempt_from_service_auth() {
        let response = app()
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn protected_path_rejects_missing_token() {
        let response = app()
            .oneshot(Request::builder().uri("/db/query").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(
            json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("unauthorized"),
            "unexpected body: {json}"
        );
    }

    #[tokio::test]
    async fn protected_path_accepts_default_token() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/db/query")
                    .header("x-service-token", "fluxbase_secret_token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
