/// POST /internal/cache/invalidate
///
/// Called by the data-engine or API after any write mutation to evict stale
/// query-cache entries for that project (and optionally a specific table).
///
/// Request body:
///   { "project_id": "...", "table": "users" }   // table is optional
///
/// Protected by X-Service-Token to prevent unauthenticated eviction.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct InvalidatePayload {
    /// Project to invalidate (required).
    pub project_id: String,
    /// Optional table name — when present, only entries tagged with this table
    /// are evicted.  When absent, the entire project's cache is flushed.
    pub table: Option<String>,
}

fn validate_service_token(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get("x-service-token")
        .and_then(|v| v.to_str().ok())
        .map(|t| t == expected)
        .unwrap_or(false)
}

pub async fn invalidate_handler(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(payload): Json<InvalidatePayload>,
) -> impl IntoResponse {
    if !validate_service_token(&headers, &state.internal_service_token) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "invalid_service_token" })),
        )
            .into_response();
    }

    let before = state.query_cache.len();
    state
        .query_cache
        .invalidate(&payload.project_id, payload.table.as_deref());
    let after = state.query_cache.len();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "ok": true,
            "evicted": before.saturating_sub(after),
            "remaining": after,
        })),
    )
        .into_response()
}

/// GET /internal/cache/stats — live cache metrics (no auth required on internal port).
pub async fn stats_handler(State(state): State<SharedState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "entries": state.query_cache.len(),
    }))
}
