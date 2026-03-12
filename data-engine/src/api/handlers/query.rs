use axum::{extract::State, http::HeaderMap, Json};
use serde_json::json;
use std::sync::Arc;

use crate::{
    compiler::query_compiler::QueryRequest,
    engine::{error::EngineError, pipeline::QueryPipeline},
    state::AppState,
};

/// POST /db/query
///
/// Thin handler — auth, guard, policy, schema, compile, hooks, execute,
/// transform, and event emission are all handled inside `QueryPipeline::run`.
/// This function is responsible only for HTTP request/response plumbing.
pub async fn handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<QueryRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let (data, meta) = QueryPipeline::new(&state).run(&headers, req).await?;

    Ok(Json(json!({
        "data": data,
        "meta": {
            "strategy":    meta.strategy,
            "complexity":  meta.complexity,
            "elapsed_ms":  meta.elapsed_ms,
            "rows":        meta.rows,
            "sql":         meta.compiled_sql,
            "request_id":  meta.request_id,
        }
    })))
}
