use std::sync::Arc;
use axum::{extract::{State, Path}, Json};
use uuid::Uuid;
use crate::state::AppState;
use crate::queue::update_status::update_status;

pub async fn handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    match update_status(&state.pool, id, "cancelled").await {
        Ok(_) => Json(serde_json::json!({"status": "cancelled"})),
        Err(_) => Json(serde_json::json!({"error": "Failed to cancel job"})),
    }
}