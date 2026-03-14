use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};

/// POST /internal/cache/invalidate
///
/// Signals all runtime workers to drop their in-memory function cache so the
/// next execution picks up the freshly deployed bundle.
pub async fn cache_invalidate(
    State(state): State<crate::AppState>,
) -> impl IntoResponse {
    let url = format!("{}/internal/cache/invalidate", state.runtime_url);
    match state.http_client.post(&url).send().await {
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
