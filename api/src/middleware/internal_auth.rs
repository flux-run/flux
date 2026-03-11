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

pub async fn require_service_token(req: Request, next: Next) -> Response {
    let expected = std::env::var("INTERNAL_SERVICE_TOKEN")
        .unwrap_or_else(|_| "fluxbase_secret_token".to_string());

    let provided = req
        .headers()
        .get("x-service-token")
        .or_else(|| req.headers().get("X-Service-Token"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if provided != expected {
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
