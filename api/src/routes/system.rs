use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use api_contract::routes as R;

/// POST /internal/cache/invalidate
///
/// Signals all runtime workers to drop their in-memory function cache so the
/// next execution picks up the freshly deployed bundle.
/// Accepts an optional JSON body `{ "function_id": "..." }` to scope the
/// invalidation.  Forwards the body (or an empty object) to the runtime.
pub async fn cache_invalidate(
    State(state): State<crate::AppState>,
    body: Option<Json<serde_json::Value>>,
) -> impl IntoResponse {
    let payload = body.map(|b| b.0).unwrap_or_else(|| serde_json::json!({}));
    let service_token = std::env::var("INTERNAL_SERVICE_TOKEN")
        .unwrap_or_else(|_| "dev-service-token".to_string());
    let url = R::internal::CACHE_INVALIDATE.url(&state.runtime_url);
    match state.http_client.post(&url)
        .header("X-Service-Token", &service_token)
        .json(&payload)
        .send().await
    {
        Ok(resp) if resp.status().is_success() => {
            (StatusCode::OK, Json(serde_json::json!({ "invalidated": true })))
        }
        Ok(resp) => {
            tracing::warn!("gateway cache invalidate returned {}", resp.status());
            (StatusCode::BAD_GATEWAY, Json(serde_json::json!({ "invalidated": false, "error": "gateway_error" })))
        }
        Err(e) => {
            tracing::error!("cache invalidate request failed: {e}");
            (StatusCode::BAD_GATEWAY, Json(serde_json::json!({ "invalidated": false, "error": "gateway_unreachable" })))
        }
    }
}

/// Execution-plane guard.
///
/// The API service is the **control plane** only.  Function invocation and
/// all runtime traffic must flow through the Gateway.
pub async fn execution_not_allowed(
    req: axum::extract::Request,
) -> impl IntoResponse {
    tracing::warn!(
        "execution_plane_misroute: {} {} — must go to the Gateway",
        req.method(),
        req.uri().path(),
    );
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(serde_json::json!({
            "error":   "execution_not_allowed",
            "message": "Function execution must go through the Gateway (http://localhost:8081)",
            "code":    405,
        })),
    )
}
