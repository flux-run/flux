use axum::{extract::State, Json};
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;
use crate::services::job_service;
use crate::services::job_service::CreateJobInput;

#[derive(Deserialize)]
pub struct CreateJobRequest {
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub r#type: String,
    pub function_id: Option<Uuid>,
    pub payload: serde_json::Value,
    pub run_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn handler(
    State(pool): State<PgPool>,
    Json(req): Json<CreateJobRequest>,
) -> Json<serde_json::Value> {
    let run_at = req
        .run_at
        .map(|dt| dt.naive_utc())
        .unwrap_or_else(|| chrono::Utc::now().naive_utc());

    let input = CreateJobInput {
        tenant_id: req.tenant_id,
        project_id: req.project_id,
        job_type: req.r#type,
        function_id: req.function_id,
        payload: req.payload,
        run_at,
        max_attempts: 5,
    };

    match job_service::create_job(&pool, input).await {
        Ok(job_id) => Json(serde_json::json!({"job_id": job_id})),
        Err(_) => Json(serde_json::json!({"error": "Failed to create job"})),
    }
}