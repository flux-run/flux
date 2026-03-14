//! Function registry routes.
//!
//! All functions belong to the active project (from `RequestContext::project_id`).
//! Run URLs point at the local gateway (`AppState::gateway_url/{name}`) instead of
//! cloud-hosted tenant subdomains.
//!
//! ## SOLID note (Single Responsibility)
//! This file is HTTP-only: request parsing, delegation to SQL, response shaping.
//! No auth logic, no tenant resolution, no schema validation beyond what the DB enforces.

use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use crate::error::{ApiError, ApiResponse, ApiResult};
use crate::validation::{validate_name, PaginationQuery};
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;
use crate::types::context::RequestContext;
use crate::AppState;

// ── Row structs ─────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct FunctionRow {
    id:           Uuid,
    name:         String,
    runtime:      String,
    description:  Option<String>,
    input_schema: Option<serde_json::Value>,
    output_schema: Option<serde_json::Value>,
    created_at:   chrono::NaiveDateTime,
}

// ── Payloads ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateFunctionPayload {
    pub name:    String,
    pub runtime: Option<String>,
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Build a local run URL: `http://localhost:PORT/name`
fn run_url(gateway_url: &str, name: &str) -> String {
    format!("{}/{}", gateway_url.trim_end_matches('/'), name)
}

fn row_to_json(r: FunctionRow, gateway_url: &str) -> serde_json::Value {
    serde_json::json!({
        "id":           r.id,
        "name":         r.name,
        "runtime":      r.runtime,
        "description":  r.description,
        "input_schema": r.input_schema,
        "output_schema": r.output_schema,
        "created_at":   r.created_at.to_string(),
        "run_url":      run_url(gateway_url, &r.name),
    })
}

// ── Handlers ────────────────────────────────────────────────────────────────

pub async fn list_functions(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Query(page): Query<PaginationQuery>,
) -> ApiResult<serde_json::Value> {
    let (limit, offset) = page.clamped();
    let records = sqlx::query_as::<_, FunctionRow>(
        "SELECT id, name, runtime, description, input_schema, output_schema, created_at \
         FROM functions \
         ORDER BY created_at DESC \
         LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let functions: Vec<_> = records
        .into_iter()
        .map(|r| row_to_json(r, &state.gateway_url))
        .collect();

    Ok(ApiResponse::new(serde_json::json!({ "functions": functions, "limit": limit, "offset": offset })))
}

pub async fn get_function(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Path(id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    let record = sqlx::query_as::<_, FunctionRow>(
        "SELECT id, name, runtime, description, input_schema, output_schema, created_at \
         FROM functions \
         WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::from)?
    .ok_or_else(|| ApiError::not_found("function not found"))?;

    Ok(ApiResponse::new(row_to_json(record, &state.gateway_url)))
}

pub async fn create_function(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Json(payload): Json<CreateFunctionPayload>,
) -> ApiResult<serde_json::Value> {
    validate_name(&payload.name).map_err(|e| ApiError::bad_request(e))?;

    let runtime = payload.runtime.as_deref().unwrap_or("deno").to_string();
    let function_id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO functions (id, name, runtime) \
         VALUES ($1, $2, $3)",
    )
    .bind(function_id)
    .bind(&payload.name)
    .bind(&runtime)
    .execute(&state.pool)
    .await
    .map_err(ApiError::from)?;

    tracing::info!(
        function_id = %function_id,
        name        = %payload.name,
        "function created",
    );

    Ok(ApiResponse::new(serde_json::json!({
        "function_id": function_id,
        "name":        payload.name,
        "runtime":     runtime,
        "run_url":     run_url(&state.gateway_url, &payload.name),
    })))
}

pub async fn delete_function(
    State(pool): State<PgPool>,
    Extension(_ctx): Extension<RequestContext>,
    Path(id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    let deleted = sqlx::query(
        "DELETE FROM functions WHERE id = $1",
    )
    .bind(id)
    .execute(&pool)
    .await
    .map_err(ApiError::from)?
    .rows_affected();

    if deleted == 0 {
        return Err(ApiError::not_found("function not found"));
    }

    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}

// ── Internal: resolve function name → id for queue / runtime ─────────────────

#[derive(Deserialize)]
pub struct ResolveQuery {
    pub name: String,
}

#[derive(sqlx::FromRow)]
struct ResolveRow {
    id: Uuid,
}

pub async fn resolve_function(
    State(pool): State<PgPool>,
    Query(q): axum::extract::Query<ResolveQuery>,
) -> Result<axum::Json<serde_json::Value>, (axum::http::StatusCode, axum::Json<serde_json::Value>)> {
    let row = sqlx::query_as::<_, ResolveRow>(
        "SELECT id FROM functions WHERE name = $1 LIMIT 1",
    )
    .bind(&q.name)
    .fetch_optional(&pool)
    .await;

    match row {
        Ok(Some(r)) => Ok(axum::Json(serde_json::json!({
            "function_id": r.id,
        }))),
        Ok(None) => Err((
            axum::http::StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({ "error": "NOT_FOUND", "message": "function not found" })),
        )),
        Err(e) => {
            tracing::error!(error = %e, "resolve_function db error");
            Err((
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({ "error": "FUNCTION_ERROR", "message": "database_error" })),
            ))
        }
    }
}
