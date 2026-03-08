use axum::{extract::{State, Path}, Json};
use sqlx::PgPool;
use uuid::Uuid;
use crate::services::job_service;

pub async fn handler(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    match job_service::get_job(&pool, id).await {
        Ok(job) => Json(serde_json::to_value(job).unwrap()),
        Err(_) => Json(serde_json::json!({"error": "Job not found"})),
    }
}