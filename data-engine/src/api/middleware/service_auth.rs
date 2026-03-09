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

    let provided = req
        .headers()
        .get("x-service-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();

    if provided == expected {
        next.run(req).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({
                "error": "unauthorized: missing or invalid x-service-token"
            })),
        )
            .into_response()
    }
}
