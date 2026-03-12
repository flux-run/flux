/// Cache invalidation endpoint — `POST /internal/cache/invalidate`.
///
/// Called by the API service immediately after a new deployment goes live.
/// Ensures the runtime stops serving the old bundle within milliseconds
/// rather than waiting for the 60-second function-cache TTL to expire.
use std::sync::Arc;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;

#[derive(Deserialize)]
pub struct InvalidateCacheRequest {
    pub function_id:   Option<String>,
    pub deployment_id: Option<String>,
    pub project_id:    Option<Uuid>,
}

pub async fn invalidate_cache_handler(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<InvalidateCacheRequest>,
) -> impl IntoResponse {
    let provided = headers.get("X-Service-Token")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if provided != state.service_token {
        return (StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "unauthorized" }))).into_response();
    }

    let mut evicted: Vec<&str> = Vec::new();

    if let Some(ref fid) = req.function_id {
        state.bundle_cache.invalidate_function(fid);
        state.wasm_pool.evict(fid).await;
        state.schema_cache.invalidate(fid);
        evicted.push("function_bundle");
        evicted.push("function_schema");
    }
    if let Some(ref did) = req.deployment_id {
        state.bundle_cache.invalidate_deployment(did);
        evicted.push("deployment_bundle");
    }

    tracing::info!(
        function_id   = ?req.function_id,
        deployment_id = ?req.deployment_id,
        project_id    = ?req.project_id,
        "cache invalidated: {:?}", evicted,
    );

    (StatusCode::OK, Json(serde_json::json!({ "evicted": evicted }))).into_response()
}
