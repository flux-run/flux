use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;
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
pub struct QueueConfigRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub max_attempts: i32,
    pub visibility_timeout_ms: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow, Serialize)]
pub struct DeadLetterJobRow {
    pub id: Uuid,
    pub tenant_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub function_id: Option<Uuid>,
    pub payload: Option<Value>,
    pub error: Option<String>,
    pub failed_at: Option<chrono::NaiveDateTime>,
}

#[derive(Deserialize)]
pub struct CreateQueuePayload {
    pub name: String,
    pub description: Option<String>,
    pub max_attempts: Option<i32>,
    pub visibility_timeout_ms: Option<i64>,
}

#[derive(Deserialize)]
pub struct PublishMessagePayload {
    pub function_id: Uuid,
    pub payload: Option<Value>,
    pub delay_ms: Option<i64>,
}

pub async fn list_queues(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Query(page): Query<PaginationQuery>,
) -> ApiResult<Vec<QueueConfigRow>> {
    let (limit, offset) = page.clamped();
    let rows = sqlx::query_as::<_, QueueConfigRow>(
        "SELECT id, project_id, name, description, max_attempts, visibility_timeout_ms, created_at \
         FROM flux.queue_configs WHERE project_id = $1 ORDER BY created_at DESC \
         LIMIT $2 OFFSET $3",
    )
    .bind(ctx.project_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(rows))
}

pub async fn create_queue(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Json(payload): Json<CreateQueuePayload>,
) -> ApiResult<QueueConfigRow> {
    let max_attempts = payload.max_attempts.unwrap_or(5);
    let visibility_timeout_ms = payload.visibility_timeout_ms.unwrap_or(30000);

    let row = sqlx::query_as::<_, QueueConfigRow>(
        "INSERT INTO flux.queue_configs \
         (project_id, name, description, max_attempts, visibility_timeout_ms) \
         VALUES ($1, $2, $3, $4, $5) \
         RETURNING id, project_id, name, description, max_attempts, visibility_timeout_ms, created_at",
    )
    .bind(ctx.project_id)
    .bind(&payload.name)
    .bind(&payload.description)
    .bind(max_attempts)
    .bind(visibility_timeout_ms)
    .fetch_one(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::created(row))
}

pub async fn get_queue(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Path(name): Path<String>,
) -> ApiResult<serde_json::Value> {
    let queue = sqlx::query_as::<_, QueueConfigRow>(
        "SELECT id, project_id, name, description, max_attempts, visibility_timeout_ms, created_at \
         FROM flux.queue_configs WHERE name = $1 AND project_id = $2",
    )
    .bind(&name)
    .bind(ctx.project_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(db_err)?
    .ok_or_else(|| ApiError::not_found("queue_not_found"))?;

    let count_row = sqlx::query("SELECT COUNT(*) as count FROM jobs WHERE project_id = $1 AND status = 'pending'")
        .bind(ctx.project_id)
        .fetch_one(&state.pool)
        .await
        .map_err(db_err)?;
    let pending: i64 = count_row.get("count");

    Ok(ApiResponse::new(serde_json::json!({
        "id": queue.id,
        "name": queue.name,
        "description": queue.description,
        "max_attempts": queue.max_attempts,
        "visibility_timeout_ms": queue.visibility_timeout_ms,
        "created_at": queue.created_at,
        "pending_jobs": pending,
    })))
}

pub async fn delete_queue(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Path(name): Path<String>,
) -> ApiResult<serde_json::Value> {
    sqlx::query("DELETE FROM flux.queue_configs WHERE name = $1 AND project_id = $2")
        .bind(&name)
        .bind(ctx.project_id)
        .execute(&state.pool)
        .await
        .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "success": true })))
}

pub async fn publish_message(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Path(_name): Path<String>,
    Json(payload): Json<PublishMessagePayload>,
) -> ApiResult<serde_json::Value> {
    let run_at = payload
        .delay_ms
        .map(|ms| chrono::Utc::now() + chrono::Duration::milliseconds(ms))
        .unwrap_or_else(chrono::Utc::now);

    let run_at_naive = run_at.naive_utc();

    let row = sqlx::query(
        "INSERT INTO jobs (tenant_id, project_id, function_id, payload, run_at) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(state.local_tenant_id)
    .bind(ctx.project_id)
    .bind(payload.function_id)
    .bind(payload.payload.unwrap_or(Value::Object(Default::default())))
    .bind(run_at_naive)
    .fetch_one(&state.pool)
    .await
    .map_err(db_err)?;

    let id: Uuid = row.get("id");
    Ok(ApiResponse::created(serde_json::json!({ "id": id })))
}

pub async fn list_bindings(
    Path(_name): Path<String>,
    State(_state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Query(_page): Query<PaginationQuery>,
) -> ApiResult<Vec<Value>> {
    Ok(ApiResponse::new(vec![]))
}

pub async fn create_binding(
    Path(_name): Path<String>,
) -> ApiResult<serde_json::Value> {
    Ok(ApiResponse::new(serde_json::json!({ "status": "ok" })))
}

pub async fn purge_queue(
    Path(_name): Path<String>,
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    let result = sqlx::query("DELETE FROM jobs WHERE project_id = $1 AND status = 'pending'")
        .bind(ctx.project_id)
        .execute(&state.pool)
        .await
        .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({
        "status": "purged",
        "deleted": result.rows_affected(),
    })))
}

pub async fn list_dlq(
    Path(_name): Path<String>,
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Query(page): Query<PaginationQuery>,
) -> ApiResult<Vec<DeadLetterJobRow>> {
    let (limit, offset) = page.clamped();
    let rows = sqlx::query_as::<_, DeadLetterJobRow>(
        "SELECT id, tenant_id, project_id, function_id, payload, error, failed_at \
         FROM dead_letter_jobs WHERE project_id = $1 \
         ORDER BY failed_at DESC LIMIT $2 OFFSET $3",
    )
    .bind(ctx.project_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(rows))
}

pub async fn replay_dlq(
    Path(_name): Path<String>,
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    sqlx::query(
        "INSERT INTO jobs (tenant_id, project_id, function_id, payload) \
         SELECT tenant_id, project_id, function_id, payload FROM dead_letter_jobs WHERE project_id = $1",
    )
    .bind(ctx.project_id)
    .execute(&state.pool)
    .await
    .map_err(db_err)?;

    sqlx::query("DELETE FROM dead_letter_jobs WHERE project_id = $1")
        .bind(ctx.project_id)
        .execute(&state.pool)
        .await
        .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "status": "replayed" })))
}
