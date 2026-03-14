use sqlx::PgPool;
use uuid::Uuid;
use std::collections::HashMap;

use super::{
    dto::{CreateSecretRequest, SecretResponse, UpdateSecretRequest},
    encryption::{decrypt_secret, encrypt_secret, EncryptionError},
    events::emit_secret_event,
};

#[derive(Debug)]
pub enum ServiceError {
    Database(String),
    Encryption(String),
    NotFound(String),
    Conflict(String),
}

impl From<sqlx::Error> for ServiceError {
    fn from(err: sqlx::Error) -> Self {
        ServiceError::Database(err.to_string())
    }
}

impl From<EncryptionError> for ServiceError {
    fn from(err: EncryptionError) -> Self {
        ServiceError::Encryption(err.0)
    }
}

pub async fn create_secret(
    pool: &PgPool,
    payload: CreateSecretRequest,
) -> Result<(Uuid, i32), ServiceError> {
    let encrypted = encrypt_secret(&payload.value)?;
    let secret_id = Uuid::new_v4();
    let version: i32 = 1;

    let res = sqlx::query(
        "INSERT INTO secrets (id, key, encrypted_value, version) VALUES ($1, $2, $3, $4)",
    )
    .bind(secret_id)
    .bind(&payload.key)
    .bind(&encrypted)
    .bind(version)
    .execute(pool)
    .await;

    if let Err(sqlx::Error::Database(ref db_err)) = res {
        if db_err.is_unique_violation() {
            return Err(ServiceError::Conflict("Secret already exists".into()));
        }
    }
    res.map_err(|e| ServiceError::Database(e.to_string()))?;

    emit_secret_event("secret.created", &payload.key, version);

    Ok((secret_id, version))
}

pub async fn update_secret(
    pool: &PgPool,
    key: &str,
    payload: UpdateSecretRequest,
) -> Result<i32, ServiceError> {
    let encrypted = encrypt_secret(&payload.value)?;

    #[derive(sqlx::FromRow)]
    struct VersionRow { version: i32 }

    let row = sqlx::query_as::<_, VersionRow>(
        "UPDATE secrets SET encrypted_value = $1, version = version + 1, updated_at = NOW() \
         WHERE key = $2 RETURNING version",
    )
    .bind(&encrypted)
    .bind(key)
    .fetch_optional(pool)
    .await
    .map_err(|e| ServiceError::Database(e.to_string()))?;

    if let Some(r) = row {
        emit_secret_event("secret.updated", key, r.version);
        Ok(r.version)
    } else {
        Err(ServiceError::NotFound("Secret not found".into()))
    }
}

pub async fn delete_secret(
    pool: &PgPool,
    key: &str,
) -> Result<(), ServiceError> {
    let res = sqlx::query("DELETE FROM secrets WHERE key = $1")
        .bind(key)
        .execute(pool)
        .await
        .map_err(|e| ServiceError::Database(e.to_string()))?;

    if res.rows_affected() == 0 {
        return Err(ServiceError::NotFound("Secret not found".into()));
    }

    emit_secret_event("secret.deleted", key, 0);

    Ok(())
}

pub async fn list_secrets(
    pool: &PgPool,
) -> Result<Vec<SecretResponse>, ServiceError> {
    #[derive(sqlx::FromRow)]
    struct SecretMetadataRow {
        key: String,
        version: i32,
        created_at: Option<chrono::NaiveDateTime>,
    }

    let records = sqlx::query_as::<_, SecretMetadataRow>(
        "SELECT key, version, created_at FROM secrets ORDER BY key ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| ServiceError::Database(e.to_string()))?;

    let response = records.into_iter().map(|r| SecretResponse {
        key: r.key,
        version: r.version,
        created_at: r.created_at.map(|d| d.to_string()).unwrap_or_default(),
    }).collect();

    Ok(response)
}

pub async fn get_runtime_secrets(
    pool: &PgPool,
) -> Result<HashMap<String, String>, ServiceError> {
    #[derive(sqlx::FromRow)]
    struct EncryptedSecretRow {
        key: String,
        encrypted_value: String,
    }

    let records = sqlx::query_as::<_, EncryptedSecretRow>(
        "SELECT key, encrypted_value FROM secrets",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| ServiceError::Database(e.to_string()))?;

    let mut secrets_map = HashMap::new();
    for row in records {
        let plaintext = decrypt_secret(&row.encrypted_value)?;
        secrets_map.insert(row.key, plaintext);
    }

    Ok(secrets_map)
}
