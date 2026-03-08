use std::sync::Arc;
use axum::{extract::{State, Path}, Json};
use uuid::Uuid;
use crate::state::AppState;
use crate::services::job_service;

pub async fn handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    match job_service::get_job(&state.pool, id).await {
        Ok(job) => Json(serde_json::to_value(job).unwrap()),
        Err(_) => Json(serde_json::json!({"error": "Job not found"})),
    }
}