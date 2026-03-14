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
use subtle::ConstantTimeEq;

/// Axum middleware function — rejects requests missing or carrying the wrong
/// `x-service-token` header.
pub async fn require_service_token(req: Request, next: Next) -> Response {
    // Health + version endpoints are exempt so load-balancer probes work.
    let path = req.uri().path().to_owned();
    if path == "/health" || path == "/version" {
        return next.run(req).await;
    }

    let expected = {
        match std::env::var("INTERNAL_SERVICE_TOKEN") {
            Ok(v) if !v.is_empty() => v,
            _ => {
                if std::env::var("FLUX_ENV").as_deref() == Ok("production") {
                    panic!(
                        "[Flux] INTERNAL_SERVICE_TOKEN must be set in production. \
                         The data-engine service cannot start without it."
                    );
                }
                tracing::warn!(
                    "[Flux] INTERNAL_SERVICE_TOKEN not set — using insecure default \
                     'dev-service-token'. Set this env var in production."
                );
                "dev-service-token".to_string()
            }
        }
    };

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

    // Constant-time comparison prevents timing-based token enumeration.
    let token_ok: bool = provided.as_bytes().ct_eq(expected.as_bytes()).into();
    if token_ok {
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
                    .header("x-service-token", "dev-service-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn version_is_exempt_from_service_auth() {
        // Even if not explicitly mapped in `app()`, the middleware should process it.
        // We add it to `app()` to test middleware logic bypass.
        let router = Router::new()
            .route("/version", get(|| async { Json(serde_json::json!({ "ver": "1" })) }))
            .layer(from_fn(require_service_token));

        let response = router
            .oneshot(Request::builder().uri("/version").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn health_with_invalid_token_is_still_exempt() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .header("x-service-token", "totally_wrong_token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn health_case_sensitivity_is_enforced() {
        // App must have a wildcard or something to catch /HEALTH if it bypasses middleware.
        // If middleware bypasses, router might 404. If middleware catches it, it 401s.
        let router = Router::new()
            .route("/HEALTH", get(|| async { Json(serde_json::json!({ "ok": true })) }))
            .layer(from_fn(require_service_token));

        let response = router
            .oneshot(Request::builder().uri("/HEALTH").body(Body::empty()).unwrap())
            .await
            .unwrap();

        // Middleware expects exact "/health", so "/HEALTH" falls through to token check and 401s
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn health_with_query_params_is_exempt() {
        let router = Router::new()
            .route("/health", get(|| async { Json(serde_json::json!({ "ok": true })) }))
            .layer(from_fn(require_service_token));

        let response = router
            .oneshot(Request::builder().uri("/health?foo=bar").body(Body::empty()).unwrap())
            .await
            .unwrap();

        // URI path strips query params, so it should match "/health"
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn protected_path_rejects_empty_and_whitespace_tokens() {
        for token in ["", " dev-service-token ", "dev-service-token "] {
            let response = app()
                .oneshot(
                    Request::builder()
                        .uri("/db/query")
                        .header("x-service-token", token)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "Failed for token: '{}'", token);
        }
    }

    #[tokio::test]
    async fn parses_request_id_successfully() {
        // Ensure that providing x-request-id doesn't break things and tracing works.
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/db/query")
                    .header("x-service-token", "dev-service-token")
                    .header("x-request-id", "req-12345")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
