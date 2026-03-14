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
    Path(name): Path<String>,
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Query(page): Query<PaginationQuery>,
) -> ApiResult<serde_json::Value> {
    let (limit, offset) = page.clamped();

    #[derive(sqlx::FromRow)]
    struct RunRow {
        id:           uuid::Uuid,
        job_name:     String,
        scheduled_at: chrono::DateTime<chrono::Utc>,
        started_at:   chrono::DateTime<chrono::Utc>,
        finished_at:  Option<chrono::DateTime<chrono::Utc>>,
        status:       String,
        error:        Option<String>,
        request_id:   Option<uuid::Uuid>,
    }

    let rows = sqlx::query_as::<_, RunRow>(
        "SELECT id, job_name, scheduled_at, started_at, finished_at, \
         status, error, request_id \
         FROM fluxbase_internal.cron_job_runs \
         WHERE job_name = $1 \
         ORDER BY created_at DESC \
         LIMIT $2 OFFSET $3",
    )
    .bind(&name)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(db_err)?;

    let data: Vec<serde_json::Value> = rows.into_iter().map(|r| serde_json::json!({
        "id":           r.id,
        "job_name":     r.job_name,
        "scheduled_at": r.scheduled_at,
        "started_at":   r.started_at,
        "finished_at":  r.finished_at,
        "status":       r.status,
        "error":        r.error,
        "request_id":   r.request_id,
    })).collect();
    let count = data.len();
    Ok(ApiResponse::new(serde_json::json!({ "data": data, "count": count })))
}
