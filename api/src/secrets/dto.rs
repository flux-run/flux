use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct CreateSecretRequest {
    pub key: String,
    pub value: String,
    pub project_id: Option<Uuid>,
}

#[derive(Deserialize)]
pub struct UpdateSecretRequest {
    pub value: String,
    pub project_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct SecretResponse {
    pub key: String,
    pub version: i32,
    pub created_at: String,
}
