/// Middleware that protects all `/internal/*` routes with a shared service token.
///
/// Every handler mounted under `/internal` must be called by a trusted
/// internal service (runtime, gateway, queue) that knows
/// `INTERNAL_SERVICE_TOKEN`.  This middleware enforces that at the router
/// layer so individual handlers do not need to duplicate the check.
///
/// The `/health` and `/version` paths are intentionally excluded so
/// load-balancer probes keep working without a token.
use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use subtle::ConstantTimeEq;

pub async fn require_service_token(req: Request, next: Next) -> Response {
    let expected = crate::middleware::require_secret(
        "INTERNAL_SERVICE_TOKEN",
        "dev-service-token",
        "Internal service token (INTERNAL_SERVICE_TOKEN)",
    );

    let provided = req
        .headers()
        .get("x-service-token")
        .or_else(|| req.headers().get("X-Service-Token"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Constant-time comparison prevents timing-based token enumeration.
    let token_ok: bool = provided.as_bytes().ct_eq(expected.as_bytes()).into();
    if !token_ok {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "invalid_service_token",
                "message": "Internal endpoints require a valid X-Service-Token header"
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
        Json, Router,
    };
    use tower::util::ServiceExt;

    use super::require_service_token;

    fn app() -> Router {
        Router::new()
            .route("/ok", get(|| async { Json(serde_json::json!({ "ok": true })) }))
            .layer(from_fn(require_service_token))
    }

    #[tokio::test]
    async fn rejects_missing_service_token() {
        let response = app()
            .oneshot(Request::builder().uri("/ok").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "invalid_service_token");
    }

    #[tokio::test]
    async fn accepts_default_service_token() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/ok")
                    .header("x-service-token", "dev-service-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["ok"], true);
    }

    #[tokio::test]
    async fn rejects_empty_service_token() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/ok")
                    .header("x-service-token", "")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn rejects_whitespace_padded_service_token() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/ok")
                    .header("x-service-token", " dev-service-token ")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn rejects_incorrect_casing_service_token() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/ok")
                    .header("x-service-token", "Dev-Service-Token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn accepts_uppercase_header_name() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/ok")
                    .header("X-Service-Token", "dev-service-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn rejects_extremely_long_malicious_token() {
        let long_token = "a".repeat(10_000);
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/ok")
                    .header("x-service-token", long_token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
