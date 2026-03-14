use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResponse},
    types::context::RequestContext,
    validation::PaginationQuery,
    AppState,
};

type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

fn db_err(e: sqlx::Error) -> ApiError {
    ApiError::internal(e.to_string())
}

#[derive(sqlx::FromRow, Serialize)]
pub struct ApiKeyRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub key_prefix: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
pub struct CreateApiKeyPayload {
    pub name: String,
}

pub async fn list_api_keys(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Query(page): Query<PaginationQuery>,
) -> ApiResult<Vec<ApiKeyRow>> {
    let (limit, offset) = page.clamped();
    let rows = sqlx::query_as::<_, ApiKeyRow>(
        "SELECT id, project_id, name, key_prefix, created_at, last_used_at \
         FROM flux.api_keys WHERE project_id = $1 AND revoked_at IS NULL \
         ORDER BY created_at DESC LIMIT $2 OFFSET $3",
    )
    .bind(ctx.project_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(rows))
}

pub async fn create_api_key(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Json(payload): Json<CreateApiKeyPayload>,
) -> ApiResult<serde_json::Value> {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    let full_key = format!("flux_{}", hex::encode(&bytes));

    let key_hash = hex::encode(Sha256::digest(full_key.as_bytes()));
    let key_prefix = full_key[..8.min(full_key.len())].to_string();

    let row = sqlx::query_as::<_, ApiKeyRow>(
        "INSERT INTO flux.api_keys (id, project_id, name, key_hash, key_prefix) \
         VALUES ($1, $2, $3, $4, $5) \
         RETURNING id, project_id, name, key_prefix, created_at, last_used_at",
    )
    .bind(Uuid::new_v4())
    .bind(ctx.project_id)
    .bind(&payload.name)
    .bind(&key_hash)
    .bind(&key_prefix)
    .fetch_one(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::created(serde_json::json!({
        "id": row.id,
        "name": row.name,
        "key_prefix": row.key_prefix,
        "key": full_key,
        "created_at": row.created_at,
    })))
}

pub async fn delete_api_key(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Path(id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    sqlx::query("UPDATE flux.api_keys SET revoked_at = now() WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(ctx.project_id)
        .execute(&state.pool)
        .await
        .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "success": true })))
}

pub async fn rotate_api_key(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Path(id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    let full_key = format!("flux_{}", hex::encode(&bytes));

    let key_hash = hex::encode(Sha256::digest(full_key.as_bytes()));
    let key_prefix = full_key[..8.min(full_key.len())].to_string();

    sqlx::query(
        "UPDATE flux.api_keys SET key_hash = $1, key_prefix = $2 \
         WHERE id = $3 AND project_id = $4 AND revoked_at IS NULL",
    )
    .bind(&key_hash)
    .bind(&key_prefix)
    .bind(id)
    .bind(ctx.project_id)
    .execute(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({
        "key": full_key,
        "key_prefix": key_prefix,
    })))
}
