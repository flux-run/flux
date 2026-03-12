use std::sync::Arc;
use axum::{extract::{State, Query}, http::StatusCode, Json};
use serde::Deserialize;
use crate::state::AppState;
use crate::services::job_service;

#[derive(Deserialize)]
pub struct ListQuery {
    /// Filter by status: pending | running | completed | failed | cancelled
    pub status: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 { 50 }

pub async fn handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListQuery>,
) -> (StatusCode, Json<serde_json::Value>) {
    let limit  = q.limit.clamp(1, 200);
    let offset = q.offset.max(0);

    match job_service::list_jobs(&state.pool, q.status.as_deref(), limit, offset).await {
        Ok(jobs) => (
            StatusCode::OK,
            Json(serde_json::json!({ "jobs": jobs, "limit": limit, "offset": offset })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}
