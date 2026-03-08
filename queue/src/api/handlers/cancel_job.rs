use axum::{extract::{State, Path}, Json};
use sqlx::PgPool;
use uuid::Uuid;
use crate::queue::update_status::update_status;

pub async fn handler(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    match update_status(&pool, id, "cancelled").await {
        Ok(_) => Json(serde_json::json!({"status": "cancelled"})),
        Err(_) => Json(serde_json::json!({"error": "Failed to cancel job"})),
    }
}