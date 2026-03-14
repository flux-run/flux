use std::sync::Arc;
use axum::{extract::State, http::StatusCode, Json};
use job_contract::job::CreateJobRequest;
use crate::state::AppState;
use crate::services::job_service;
use crate::services::job_service::CreateJobInput;

pub async fn handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateJobRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let pool = &state.pool;

    let run_at = match req.delay_seconds {
        Some(d) if d > 0 => {
            chrono::Utc::now().naive_utc()
                + chrono::Duration::try_seconds(d).unwrap_or_default()
        }
        _ => chrono::Utc::now().naive_utc(),
    };

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
        Ok(job_id) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "job_id": job_id })),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error":   "FUNCTION_ERROR",
                "message": "failed to create job",
                "code":    500,
            })),
        ),
    }
}