use axum::{
    extract::State,
    http::HeaderMap,
    Json,
};
use serde_json::json;
use std::sync::Arc;

use crate::{
    engine::{auth_context::AuthContext, error::EngineError},
    state::AppState,
};

/// GET /db/debug
///
/// Returns engine configuration limits and live cache statistics.
/// Useful for debugging in the Query Explorer and CLI tooling.
/// Requires valid tenant/project authentication.
pub async fn handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, EngineError> {
    // Require auth so internal limits are not exposed to the public.
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    Ok(Json(json!({
        "limits": {
            "default_rows":    state.default_query_limit,
            "max_rows":        state.max_query_limit,
            "max_complexity":  state.query_guard.max_complexity,
            "max_nest_depth":  state.query_guard.max_nest_depth,
            "timeout_ms":      state.query_guard.timeout.as_millis() as u64,
        },
        "cache": {
            "schema_entries": state.schema_cache.entry_count(),
            "plan_entries":   state.plan_cache.entry_count(),
        },
        "version": env!("CARGO_PKG_VERSION"),
    })))
}
