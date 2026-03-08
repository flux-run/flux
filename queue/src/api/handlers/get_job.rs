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
        Ok(job) => {
            // Derive queue_time_ms: time from creation to execution start.
            let queue_time_ms = job.started_at.map(|s| {
                (s - job.created_at).num_milliseconds()
            });

            // Derive execution_time_ms: time from execution start to last update
            // (updated_at is stamped on completion/failure).
            let execution_time_ms = job.started_at.map(|s| {
                (job.updated_at - s).num_milliseconds()
            });

            let mut val = serde_json::to_value(&job).unwrap();
            if let Some(obj) = val.as_object_mut() {
                obj.insert("queue_time_ms".into(), queue_time_ms.into());
                obj.insert("execution_time_ms".into(), execution_time_ms.into());
            }
            Json(val)
        }
        Err(_) => Json(serde_json::json!({ "error": "Job not found" })),
    }
}