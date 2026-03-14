use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResponse},
    types::context::RequestContext,
    validation::PaginationQuery,
    AppState,
};
use api_contract::queue::{
    CreateQueuePayload, DeadLetterJobRow, PublishMessagePayload, QueueConfigRow,
};

type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

fn db_err(e: sqlx::Error) -> ApiError {
    ApiError::internal(e.to_string())
}

pub async fn list_queues(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Query(page): Query<PaginationQuery>,
) -> ApiResult<Vec<QueueConfigRow>> {
    let (limit, offset) = page.clamped();
    let rows = sqlx::query_as::<_, QueueConfigRow>(
        "SELECT id, name, description, max_attempts, visibility_timeout_ms, created_at \
         FROM flux.queue_configs ORDER BY created_at DESC \
         LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(rows))
}

pub async fn create_queue(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Json(payload): Json<CreateQueuePayload>,
) -> ApiResult<QueueConfigRow> {
    let max_attempts = payload.max_attempts.unwrap_or(5);
    let visibility_timeout_ms = payload.visibility_timeout_ms.unwrap_or(30000);

    let row = sqlx::query_as::<_, QueueConfigRow>(
        "INSERT INTO flux.queue_configs \
         (name, description, max_attempts, visibility_timeout_ms) \
         VALUES ($1, $2, $3, $4) \
         RETURNING id, name, description, max_attempts, visibility_timeout_ms, created_at",
    )
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
    Extension(_ctx): Extension<RequestContext>,
    Path(name): Path<String>,
) -> ApiResult<serde_json::Value> {
    let queue = sqlx::query_as::<_, QueueConfigRow>(
        "SELECT id, name, description, max_attempts, visibility_timeout_ms, created_at \
         FROM flux.queue_configs WHERE name = $1",
    )
    .bind(&name)
    .fetch_optional(&state.pool)
    .await
    .map_err(db_err)?
    .ok_or_else(|| ApiError::not_found("queue_not_found"))?;

    let count_row = sqlx::query(
        "SELECT COUNT(*) as count FROM jobs WHERE status = 'pending' AND queue_name = $1",
    )
    .bind(&name)
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
    Extension(_ctx): Extension<RequestContext>,
    Path(name): Path<String>,
) -> ApiResult<serde_json::Value> {
    sqlx::query("DELETE FROM flux.queue_configs WHERE name = $1")
        .bind(&name)
        .execute(&state.pool)
        .await
        .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "success": true })))
}

pub async fn publish_message(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Path(_name): Path<String>,
    Json(payload): Json<PublishMessagePayload>,
) -> ApiResult<serde_json::Value> {
    let run_at = payload
        .delay_seconds
        .map(|secs| chrono::Utc::now() + chrono::Duration::seconds(secs))
        .unwrap_or_else(chrono::Utc::now);

    let run_at_naive = run_at.naive_utc();

    let row = sqlx::query(
        "INSERT INTO jobs (function_id, payload, run_at, queue_name) \
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(payload.function_id)
    .bind(payload.payload.unwrap_or(Value::Object(Default::default())))
    .bind(run_at_naive)
    .bind(&_name)
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
    Err(ApiError::new(
        axum::http::StatusCode::NOT_IMPLEMENTED,
        "NOT_IMPLEMENTED",
        "Queue bindings are not yet supported on this server",
    ))
}

pub async fn create_binding(
    Path(_name): Path<String>,
) -> ApiResult<serde_json::Value> {
    Err(ApiError::new(
        axum::http::StatusCode::NOT_IMPLEMENTED,
        "NOT_IMPLEMENTED",
        "Queue bindings are not yet supported on this server",
    ))
}

pub async fn purge_queue(
    Path(name): Path<String>,
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    let result =
        sqlx::query("DELETE FROM jobs WHERE status = 'pending' AND queue_name = $1")
            .bind(&name)
            .execute(&state.pool)
            .await
            .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({
        "status": "purged",
        "deleted": result.rows_affected(),
    })))
}

pub async fn list_dlq(
    Path(name): Path<String>,
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Query(page): Query<PaginationQuery>,
) -> ApiResult<Vec<DeadLetterJobRow>> {
    let (limit, offset) = page.clamped();
    let rows = sqlx::query_as::<_, DeadLetterJobRow>(
        "SELECT id, function_id, payload, error, failed_at \
         FROM dead_letter_jobs \
         WHERE queue_name = $1 \
         ORDER BY failed_at DESC LIMIT $2 OFFSET $3",
    )
    .bind(&name)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(rows))
}

pub async fn replay_dlq(
    Path(name): Path<String>,
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    // Atomically move up to 500 DLQ entries back into the jobs table using a
    // DELETE...RETURNING CTE so no row is lost if the INSERT fails.
    let result = sqlx::query(
        "WITH batch AS ( \
             DELETE FROM dead_letter_jobs \
             WHERE id IN ( \
                 SELECT id FROM dead_letter_jobs \
                 WHERE queue_name = $1 \
                 ORDER BY failed_at \
                 LIMIT 500 \
             ) \
             RETURNING function_id, payload, queue_name \
         ) \
         INSERT INTO jobs (function_id, payload, queue_name) \
         SELECT function_id, payload, queue_name FROM batch",
    )
    .bind(&name)
    .execute(&state.pool)
    .await
    .map_err(db_err)?;

    let replayed = result.rows_affected();

    let remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM dead_letter_jobs WHERE queue_name = $1",
    )
    .bind(&name)
    .fetch_one(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({
        "replayed":  replayed,
        "remaining": remaining,
    })))
}
