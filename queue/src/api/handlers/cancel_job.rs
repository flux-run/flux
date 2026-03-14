use std::sync::Arc;
use axum::{extract::{State, Path}, http::StatusCode, Json};
use uuid::Uuid;
use crate::state::AppState;
use crate::queue::update_status::update_status;

pub async fn handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<serde_json::Value>) {
    match update_status(&state.pool, id, "cancelled").await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"status": "cancelled"}))),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error":   "FUNCTION_ERROR",
                "message": "failed to cancel job",
                "code":    500,
            })),
        ),
    }
}