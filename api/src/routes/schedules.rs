use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
pub struct CronJobRow {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub schedule: String,
    pub action_type: String,
    pub action_config: Value,
    pub enabled: bool,
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct CreateSchedulePayload {
    pub name: String,
    pub schedule: String,
    pub action_type: String,
    pub action_config: Option<Value>,
}

pub async fn list_schedules(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Query(page): Query<PaginationQuery>,
) -> ApiResult<Vec<CronJobRow>> {
    let (limit, offset) = page.clamped();
    let rows = sqlx::query_as::<_, CronJobRow>(
        "SELECT id, tenant_id, project_id, name, schedule, action_type, action_config, \
         enabled, last_run_at, next_run_at, created_at, updated_at \
         FROM fluxbase_internal.cron_jobs WHERE project_id = $1 ORDER BY created_at DESC \
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

pub async fn create_schedule(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Json(payload): Json<CreateSchedulePayload>,
) -> ApiResult<CronJobRow> {
    let row = sqlx::query_as::<_, CronJobRow>(
        "INSERT INTO fluxbase_internal.cron_jobs \
         (tenant_id, project_id, name, schedule, action_type, action_config) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         RETURNING id, tenant_id, project_id, name, schedule, action_type, action_config, \
         enabled, last_run_at, next_run_at, created_at, updated_at",
    )
    .bind(ctx.tenant_id)
    .bind(ctx.project_id)
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
    Extension(ctx): Extension<RequestContext>,
    Path(name): Path<String>,
) -> ApiResult<serde_json::Value> {
    sqlx::query(
        "DELETE FROM fluxbase_internal.cron_jobs WHERE name = $1 AND project_id = $2",
    )
    .bind(&name)
    .bind(ctx.project_id)
    .execute(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "success": true })))
}

pub async fn pause_schedule(
    Path(name): Path<String>,
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    sqlx::query(
        "UPDATE fluxbase_internal.cron_jobs SET enabled = false, updated_at = now() \
         WHERE name = $1 AND project_id = $2",
    )
    .bind(&name)
    .bind(ctx.project_id)
    .execute(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "success": true })))
}

pub async fn resume_schedule(
    Path(name): Path<String>,
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    sqlx::query(
        "UPDATE fluxbase_internal.cron_jobs SET enabled = true, updated_at = now() \
         WHERE name = $1 AND project_id = $2",
    )
    .bind(&name)
    .bind(ctx.project_id)
    .execute(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "success": true })))
}

pub async fn run_schedule_now(
    Path(name): Path<String>,
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    sqlx::query(
        "UPDATE fluxbase_internal.cron_jobs SET next_run_at = now(), updated_at = now() \
         WHERE name = $1 AND project_id = $2",
    )
    .bind(&name)
    .bind(ctx.project_id)
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
