use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use serde_json::Value;

use crate::{
    error::{ApiError, ApiResponse},
    types::context::RequestContext,
    validation::PaginationQuery,
    AppState,
};
use api_contract::schedules::{CreateSchedulePayload, CronJobRow};

type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

fn db_err(e: sqlx::Error) -> ApiError {
    ApiError::internal(e.to_string())
}

pub async fn list_schedules(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Query(page): Query<PaginationQuery>,
) -> ApiResult<Vec<CronJobRow>> {
    let (limit, offset) = page.clamped();
    let rows = sqlx::query_as::<_, CronJobRow>(
        "SELECT id, name, schedule, action_type, action_config, \
         enabled, last_run_at, next_run_at, created_at, updated_at \
         FROM fluxbase_internal.cron_jobs ORDER BY created_at DESC \
         LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(rows))
}

pub async fn create_schedule(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Json(payload): Json<CreateSchedulePayload>,
) -> ApiResult<CronJobRow> {
    let row = sqlx::query_as::<_, CronJobRow>(
        "INSERT INTO fluxbase_internal.cron_jobs \
         (name, schedule, action_type, action_config) \
         VALUES ($1, $2, $3, $4) \
         RETURNING id, name, schedule, action_type, action_config, \
         enabled, last_run_at, next_run_at, created_at, updated_at",
    )
    .bind(&payload.name)
    .bind(&payload.schedule)
    .bind(&payload.action_type)
    .bind(payload.action_config.unwrap_or(Value::Object(Default::default())))
    .fetch_one(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::created(row))
}

pub async fn delete_schedule(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Path(name): Path<String>,
) -> ApiResult<serde_json::Value> {
    sqlx::query(
        "DELETE FROM fluxbase_internal.cron_jobs WHERE name = $1",
    )
    .bind(&name)
    .execute(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "success": true })))
}

pub async fn pause_schedule(
    Path(name): Path<String>,
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    sqlx::query(
        "UPDATE fluxbase_internal.cron_jobs SET enabled = false, updated_at = now() \
         WHERE name = $1",
    )
    .bind(&name)
    .execute(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "success": true })))
}

pub async fn resume_schedule(
    Path(name): Path<String>,
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    sqlx::query(
        "UPDATE fluxbase_internal.cron_jobs SET enabled = true, updated_at = now() \
         WHERE name = $1",
    )
    .bind(&name)
    .execute(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "success": true })))
}

pub async fn run_schedule_now(
    Path(name): Path<String>,
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    sqlx::query(
        "UPDATE fluxbase_internal.cron_jobs SET next_run_at = now(), updated_at = now() \
         WHERE name = $1",
    )
    .bind(&name)
    .execute(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "success": true })))
}

pub async fn schedule_history(
    Path(_name): Path<String>,
) -> ApiResult<serde_json::Value> {
    Ok(ApiResponse::new(serde_json::json!({ "data": [], "count": 0 })))
}
