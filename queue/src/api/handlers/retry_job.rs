use std::sync::Arc;
use axum::{extract::{State, Path}, http::StatusCode, Json};
use uuid::Uuid;
use crate::state::AppState;
use crate::services::retry_service;

pub async fn handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<serde_json::Value>) {
    // attempts=0: manual retry runs with no backoff penalty
    match retry_service::retry_job(&state.pool, id, 0).await {
        Ok(_) => (StatusCode::ACCEPTED, Json(serde_json::json!({"status": "retried"}))),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to retry job"})),
        ),
    }
}