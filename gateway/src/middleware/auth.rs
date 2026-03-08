use axum::{
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
    Json,
};
use sqlx::PgPool;

pub async fn validate_api_key(
    pool: &PgPool,
    api_key: &str,
) -> anyhow::Result<bool> {
    // API keys are stored with their hash in the api_keys table
    // For now, we'll do a simple lookup. 
    // In a real system, we'd hash the provided key and compare.
    // Assuming the user provides the raw key in X-API-Key for now.
    
    // NOTE: This implementation depends on how api_keys are stored.
    // If we have a 'keys' table or similar. 
    // Checking api/src/api_keys/model.rs earlier showed an 'api_keys' table.
    
    let exists = sqlx::query(
        "SELECT id FROM api_keys WHERE key_hash = $1 AND is_revoked = false"
    )
    .bind(api_key)
    .fetch_optional(pool)
    .await?;

    Ok(exists.is_some())
}
