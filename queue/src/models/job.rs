use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(FromRow, Serialize, Deserialize, Clone)]
pub struct Job {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub function_id: Uuid,
    pub payload: serde_json::Value,
    pub status: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub max_runtime_seconds: i32,
    pub run_at: NaiveDateTime,
    pub locked_at: Option<NaiveDateTime>,
    pub started_at: Option<NaiveDateTime>,
    /// UUID sent as `x-request-id` to the runtime. Links this job to its
    /// execution record so `flux trace <request_id>` shows the full trace.
    pub request_id: Option<Uuid>,
    pub idempotency_key: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}