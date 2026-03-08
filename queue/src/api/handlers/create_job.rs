use std::sync::Arc;
use axum::{extract::State, Json};
use job_contract::job::{CreateJobRequest, CreateJobResponse};
use crate::state::AppState;
use crate::services::job_service;
use crate::services::job_service::CreateJobInput;

pub async fn handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateJobRequest>,
) -> Json<serde_json::Value> {
    let pool = &state.pool;
    let run_at = chrono::Utc::now().naive_utc();

    let input = CreateJobInput {
        tenant_id: req.tenant_id,
        project_id: req.project_id,
        function_id: req.function_id,
        payload: req.payload,
        run_at,
        max_attempts: 5,
        idempotency_key: req.idempotency_key,
    };

    match job_service::create_job(pool, input).await {
        Ok(job_id) => Json(serde_json::to_value(CreateJobResponse { job_id }).unwrap()),
        Err(_) => Json(serde_json::json!({"error": "Failed to create job"})),
    }
}