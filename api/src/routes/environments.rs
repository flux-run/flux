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
pub struct EnvironmentRow {
    pub id: Uuid,
    pub name: String,
    pub is_default: bool,
    pub config: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct CreateEnvPayload {
    pub name: String,
    pub slug: String,
    pub config: Option<Value>,
}

#[derive(Deserialize)]
pub struct CloneEnvPayload {
    pub source: String,
    pub target: String,
}

pub async fn list_environments(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Query(page): Query<PaginationQuery>,
) -> ApiResult<Vec<Value>> {
    let (limit, offset) = page.clamped();
    let rows = sqlx::query_as::<_, EnvironmentRow>(
        "SELECT id, name, is_default, config, created_at \
         FROM flux.environments ORDER BY created_at \
         LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(db_err)?;

    if rows.is_empty() {
        return Ok(ApiResponse::new(vec![
            serde_json::json!({"name":"production","slug":"production","is_default":true,"config":{}}),
            serde_json::json!({"name":"development","slug":"development","is_default":false,"config":{}}),
        ]));
    }

    let values: Vec<Value> = rows
        .into_iter()
        .map(|r| serde_json::to_value(r).unwrap_or(Value::Null))
        .collect();

    Ok(ApiResponse::new(values))
}

pub async fn create_environment(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Json(payload): Json<CreateEnvPayload>,
) -> ApiResult<EnvironmentRow> {
    let row = sqlx::query_as::<_, EnvironmentRow>(
        "INSERT INTO flux.environments (name, config) \
         VALUES ($1, $2) \
         RETURNING id, name, is_default, config, created_at",
    )
    .bind(&payload.name)
    .bind(payload.config.unwrap_or(Value::Object(Default::default())))
    .fetch_one(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::created(row))
}

pub async fn delete_environment(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Path(name): Path<String>,
) -> ApiResult<serde_json::Value> {
    if name == "production" {
        return Err(ApiError::bad_request("cannot delete production environment"));
    }

    sqlx::query("DELETE FROM flux.environments WHERE name = $1")
        .bind(&name)
        .execute(&state.pool)
        .await
        .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "success": true })))
}

pub async fn clone_environment(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Json(payload): Json<CloneEnvPayload>,
) -> ApiResult<EnvironmentRow> {
    let source_row = sqlx::query(
        "SELECT config FROM flux.environments WHERE name = $1",
    )
    .bind(&payload.source)
    .fetch_optional(&state.pool)
    .await
    .map_err(db_err)?
    .ok_or_else(|| ApiError::not_found("source_environment_not_found"))?;

    let config: Value = source_row.get("config");

    let row = sqlx::query_as::<_, EnvironmentRow>(
        "INSERT INTO flux.environments (name, config) \
         VALUES ($1, $2) \
         RETURNING id, name, is_default, config, created_at",
    )
    .bind(&payload.target)
    .bind(config)
    .fetch_one(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::created(row))
}
