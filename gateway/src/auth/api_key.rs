//! API key authentication.
//!
//! Validates `Authorization: Bearer flux_*` (or `X-API-Key`) against the
//! `api_keys` table.  Keys are stored hashed — the raw key is never persisted.
use sqlx::PgPool;
use sha2::{Sha256, Digest};

/// Returns `true` when `raw_key` matches an active (non-revoked) API key.
pub async fn validate(pool: &PgPool, raw_key: &str) -> anyhow::Result<bool> {
    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    let key_hash = format!("{:x}", hasher.finalize());

    let exists = sqlx::query(
        "SELECT id FROM api_keys WHERE key_hash = $1 AND is_revoked = false",
    )
    .bind(&key_hash)
    .fetch_optional(pool)
    .await?;

    Ok(exists.is_some())
}
