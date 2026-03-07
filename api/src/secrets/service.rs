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
    tenant_id: Uuid,
    payload: CreateSecretRequest,
) -> Result<(Uuid, i32), ServiceError> {
    let encrypted = encrypt_secret(&payload.value)?;
    let secret_id = Uuid::new_v4();
    let version = 1;

    let res = sqlx::query!(
        r#"
        INSERT INTO secrets (id, tenant_id, project_id, key, encrypted_value, version)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        secret_id,
        tenant_id,
        payload.project_id,
        payload.key,
        encrypted,
        version
    )
    .execute(pool)
    .await;

    if let Err(sqlx::Error::Database(ref db_err)) = res {
        if db_err.is_unique_violation() {
            return Err(ServiceError::Conflict("Secret already exists".into()));
        }
    }
    res.map_err(|e| ServiceError::Database(e.to_string()))?;

    emit_secret_event("secret.created", tenant_id, payload.project_id, &payload.key, version);

    Ok((secret_id, version))
}

pub async fn update_secret(
    pool: &PgPool,
    tenant_id: Uuid,
    key: &str,
    payload: UpdateSecretRequest,
) -> Result<i32, ServiceError> {
    let encrypted = encrypt_secret(&payload.value)?;

    // Try projecting match with or without project id
    let res = match payload.project_id {
        Some(pid) => {
            sqlx::query!(
                "UPDATE secrets SET encrypted_value = $1, version = version + 1, updated_at = NOW() WHERE tenant_id = $2 AND project_id = $3 AND key = $4 RETURNING version",
                encrypted,
                tenant_id,
                pid,
                key
            )
            .fetch_optional(pool)
            .await
            .map_err(|e| ServiceError::Database(e.to_string()))?
            .map(|r| r.version)
        },
        None => {
            sqlx::query!(
                "UPDATE secrets SET encrypted_value = $1, version = version + 1, updated_at = NOW() WHERE tenant_id = $2 AND project_id IS NULL AND key = $3 RETURNING version",
                encrypted,
                tenant_id,
                key
            )
            .fetch_optional(pool)
            .await
            .map_err(|e| ServiceError::Database(e.to_string()))?
            .map(|r| r.version)
        }
    };

    if let Some(version) = res {
        emit_secret_event("secret.updated", tenant_id, payload.project_id, key, version);
        Ok(version)
    } else {
        Err(ServiceError::NotFound("Secret not found".into()))
    }
}

pub async fn delete_secret(
    pool: &PgPool,
    tenant_id: Uuid,
    project_id: Option<Uuid>,
    key: &str,
) -> Result<(), ServiceError> {
    let res = match project_id {
        Some(pid) => {
            sqlx::query!(
                "DELETE FROM secrets WHERE tenant_id = $1 AND project_id = $2 AND key = $3",
                tenant_id,
                pid,
                key
            )
            .execute(pool)
            .await
            .map_err(|e| ServiceError::Database(e.to_string()))?
        },
        None => {
            sqlx::query!(
                "DELETE FROM secrets WHERE tenant_id = $1 AND project_id IS NULL AND key = $2",
                tenant_id,
                key
            )
            .execute(pool)
            .await
            .map_err(|e| ServiceError::Database(e.to_string()))?
        }
    };

    if res.rows_affected() == 0 {
        return Err(ServiceError::NotFound("Secret not found".into()));
    }

    emit_secret_event("secret.deleted", tenant_id, project_id, key, 0);

    Ok(())
}

pub async fn list_secrets(
    pool: &PgPool,
    tenant_id: Uuid,
    project_id: Option<Uuid>,
) -> Result<Vec<SecretResponse>, ServiceError> {
    struct SecretMetadataRow {
        key: String,
        version: i32,
        created_at: Option<chrono::NaiveDateTime>,
    }

    let records = match project_id {
        Some(pid) => {
            sqlx::query_as_unchecked!(
                SecretMetadataRow,
                "SELECT key, version, created_at FROM secrets WHERE tenant_id = $1 AND project_id = $2 ORDER BY key ASC",
                tenant_id,
                pid
            )
            .fetch_all(pool)
            .await
            .map_err(|e| ServiceError::Database(e.to_string()))?
        },
        None => {
            sqlx::query_as_unchecked!(
                SecretMetadataRow,
                "SELECT key, version, created_at FROM secrets WHERE tenant_id = $1 AND project_id IS NULL ORDER BY key ASC",
                tenant_id
            )
            .fetch_all(pool)
            .await
            .map_err(|e| ServiceError::Database(e.to_string()))?
        }
    };

    let response = records.into_iter().map(|r| SecretResponse {
        key: r.key,
        version: r.version,
        created_at: r.created_at.map(|d| d.to_string()).unwrap_or_default(),
    }).collect();

    Ok(response)
}

pub async fn get_runtime_secrets(
    pool: &PgPool,
    tenant_id: Uuid,
    project_id: Option<Uuid>,
) -> Result<HashMap<String, String>, ServiceError> {
    struct EncryptedSecretRow {
        key: String,
        encrypted_value: String,
    }

    // Combine both Tenant-level (project_id IS NULL) and Project-level secrets
    let records = match project_id {
        Some(pid) => {
            sqlx::query_as_unchecked!(
                EncryptedSecretRow,
                r#"
                SELECT key, encrypted_value FROM secrets WHERE tenant_id = $1 AND project_id IS NULL
                UNION ALL
                SELECT key, encrypted_value FROM secrets WHERE tenant_id = $1 AND project_id = $2
                "#,
                tenant_id,
                pid
            )
            .fetch_all(pool)
            .await
            .map_err(|e| ServiceError::Database(e.to_string()))?
        },
        None => {
            sqlx::query_as_unchecked!(
                EncryptedSecretRow,
                "SELECT key, encrypted_value FROM secrets WHERE tenant_id = $1 AND project_id IS NULL",
                tenant_id
            )
            .fetch_all(pool)
            .await
            .map_err(|e| ServiceError::Database(e.to_string()))?
        }
    };

    let mut secrets_map = HashMap::new();
    for row in records {
        let plaintext = decrypt_secret(&row.encrypted_value)?;
        // If a project secret shares the same key as a tenant secret, it overwrites it intuitively.
        secrets_map.insert(row.key, plaintext);
    }

    Ok(secrets_map)
}
