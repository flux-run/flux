use axum::{extract::{State, Path}, Json};
use sqlx::PgPool;
use uuid::Uuid;
use crate::services::retry_service;

pub async fn handler(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    // Assume attempts=0 for manual retry
    match retry_service::retry_job(&pool, id, 0).await {
        Ok(_) => Json(serde_json::json!({"status": "retried"})),
        Err(_) => Json(serde_json::json!({"error": "Failed to retry job"})),
    }
}