use sqlx::PgPool;
use uuid::Uuid;
use super::model::ApiKey;
use super::crypto;

pub async fn create_api_key(
    pool: &PgPool,
    tenant_id: Uuid,
    project_id: Uuid,
    name: &str,
) -> Result<(ApiKey, String), String> {
    let (plaintext_key, key_hash) = crypto::generate_new_key();

    let record: Result<ApiKey, sqlx::Error> = sqlx::query_as::<_, ApiKey>(
        r#"
        INSERT INTO api_keys (tenant_id, project_id, name, key_hash)
        VALUES ($1, $2, $3, $4)
        RETURNING id, tenant_id, project_id, name, key_hash, created_at, last_used_at, revoked
        "#
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(name)
    .bind(key_hash)
    .fetch_one(pool)
    .await;

    let record = record.map_err(|e| e.to_string())?;

    Ok((record, plaintext_key))
}

pub async fn list_api_keys(
    pool: &PgPool,
    tenant_id: Uuid,
    project_id: Uuid,
) -> Result<Vec<ApiKey>, String> {
    let records: Result<Vec<ApiKey>, sqlx::Error> = sqlx::query_as::<_, ApiKey>(
        r#"
        SELECT id, tenant_id, project_id, name, key_hash, created_at, last_used_at, revoked
        FROM api_keys
        WHERE tenant_id = $1 AND project_id = $2 AND revoked = false
        ORDER BY created_at DESC
        "#
    )
    .bind(tenant_id)
    .bind(project_id)
    .fetch_all(pool)
    .await;

    let records = records.map_err(|e| e.to_string())?;

    Ok(records)
}

pub async fn revoke_api_key(
    pool: &PgPool,
    id: Uuid,
    tenant_id: Uuid,
) -> Result<(), String> {
    let result: Result<sqlx::postgres::PgQueryResult, sqlx::Error> = sqlx::query(
        r#"
        UPDATE api_keys 
        SET revoked = true 
        WHERE id = $1 AND tenant_id = $2
        "#
    )
    .bind(id)
    .bind(tenant_id)
    .execute(pool)
    .await;

    let result = result.map_err(|e| e.to_string())?;

    if result.rows_affected() == 0 {
        return Err("API Key not found or belongs to another tenant".into());
    }

    Ok(())
}

pub async fn mark_key_used(
    pool: &PgPool,
    hash: &str,
) -> Result<ApiKey, String> {
    let record: Result<Option<ApiKey>, sqlx::Error> = sqlx::query_as::<_, ApiKey>(
        r#"
        UPDATE api_keys
        SET last_used_at = now()
        WHERE key_hash = $1 AND revoked = false
        RETURNING id, tenant_id, project_id, name, key_hash, created_at, last_used_at, revoked
        "#
    )
    .bind(hash)
    .fetch_optional(pool)
    .await;

    let record = record.map_err(|e| e.to_string())?;
    
    record.ok_or_else(|| "Invalid or revoked API Key".into())
}
