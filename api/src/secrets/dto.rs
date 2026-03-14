use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct CreateSecretRequest {
    pub key: String,
    pub value: String,
}

#[derive(Deserialize)]
pub struct UpdateSecretRequest {
    pub value: String,
}

#[derive(Serialize)]
pub struct SecretResponse {
    pub key: String,
    pub version: i32,
    pub created_at: String,
}
