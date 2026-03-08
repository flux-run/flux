use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateJobRequest {
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub function_id: Uuid,
    pub payload: Value,
    /// Optional caller-supplied deduplication key.
    /// If a job with this key already exists, the existing job_id is returned
    /// rather than creating a duplicate.
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateJobResponse {
    pub job_id: Uuid,
}
