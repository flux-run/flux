use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use crate::types::response::{ApiResponse, ApiError};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;
use crate::types::context::RequestContext;

// ── Row structs ────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow, Serialize)]
pub struct RouteRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub path: String,
    pub method: String,
    pub function_id: Uuid,
    pub auth_type: String,
    pub cors_enabled: bool,
    pub rate_limit: Option<i32>,
    pub created_at: chrono::NaiveDateTime,
}

// ── Payloads ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ListRoutesQuery {
    pub project_id: Uuid,
}

#[derive(Deserialize)]
pub struct CreateRoutePayload {
    pub method: String,
    pub path: String,
    pub function_id: Uuid,
    pub auth_type: String,
    pub cors_enabled: bool,
    pub rate_limit: Option<i32>,
}

#[derive(Deserialize)]
pub struct UpdateRoutePayload {
    pub path: Option<String>,
    pub method: Option<String>,
    pub function_id: Option<Uuid>,
    pub auth_type: Option<String>,
    pub cors_enabled: Option<bool>,
    pub rate_limit: Option<Option<i32>>,
}

// ── Helpers ────────────────────────────────────────────────────────────────

type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

fn db_err(e: sqlx::Error) -> ApiError {
    eprintln!("Database error: {:?}", e);
    ApiError::internal("database_error")
}

// ── Handlers ───────────────────────────────────────────────────────────────

pub async fn list_gateway_routes(
    Query(params): Query<ListRoutesQuery>,
    State(pool): State<PgPool>,
    Extension(_context): Extension<RequestContext>,
) -> ApiResult<Vec<RouteRow>> {
    let routes = sqlx::query_as::<_, RouteRow>(
        "SELECT id, project_id, path, method, function_id, auth_type, cors_enabled, rate_limit, created_at \
         FROM routes WHERE project_id = $1 ORDER BY created_at DESC"
    )
    .bind(params.project_id)
    .fetch_all(&pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(routes))
}

pub async fn create_gateway_route(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
    Json(payload): Json<CreateRoutePayload>,
) -> ApiResult<RouteRow> {
    let project_id = context.project_id.ok_or(ApiError::bad_request("missing_project_id"))?;
    
    // Ensure project_id matches if provided in some other way or just use context
    // Here we use context.project_id for security.

    let id = Uuid::new_v4();
    
    let route = sqlx::query_as::<_, RouteRow>(
        "INSERT INTO routes (id, project_id, path, method, function_id, auth_type, cors_enabled, rate_limit) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         RETURNING id, project_id, path, method, function_id, auth_type, cors_enabled, rate_limit, created_at"
    )
    .bind(id)
    .bind(project_id)
    .bind(payload.path)
    .bind(payload.method)
    .bind(payload.function_id)
    .bind(payload.auth_type)
    .bind(payload.cors_enabled)
    .bind(payload.rate_limit)
    .fetch_one(&pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(route))
}

pub async fn update_gateway_route(
    Path(id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
    Json(payload): Json<UpdateRoutePayload>,
) -> ApiResult<RouteRow> {
    let project_id = context.project_id.ok_or(ApiError::bad_request("missing_project_id"))?;

    // Check ownership
    #[derive(sqlx::FromRow)]
    struct RouteId { id: Uuid }
    let exists = sqlx::query_as::<_, RouteId>("SELECT id FROM routes WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project_id)
        .fetch_optional(&pool)
        .await
        .map_err(db_err)?;
    
    if exists.is_none() {
        return Err(ApiError::not_found("route_not_found"));
    }

    // Since sqlx doesn't support easy dynamic queries without external crates, 
    // we'll fetch the current state and merge if necessary, or just use COALESCE for non-None-None fields.
    // For rate_limit (Option<Option<i32>>):
    // - payload.rate_limit is None: No change.
    // - payload.rate_limit is Some(None): Set to NULL.
    // - payload.rate_limit is Some(Some(v)): Set to v.

    if let Some(opt) = payload.rate_limit {
        sqlx::query("UPDATE routes SET rate_limit = $1 WHERE id = $2")
            .bind(opt)
            .bind(id)
            .execute(&pool)
            .await
            .map_err(db_err)?;
    }

    let route = sqlx::query_as::<_, RouteRow>(
        "UPDATE routes SET \
         path = COALESCE($1, path), \
         method = COALESCE($2, method), \
         function_id = COALESCE($3, function_id), \
         auth_type = COALESCE($4, auth_type), \
         cors_enabled = COALESCE($5, cors_enabled) \
         WHERE id = $6 \
         RETURNING id, project_id, path, method, function_id, auth_type, cors_enabled, rate_limit, created_at"
    )
    .bind(payload.path)
    .bind(payload.method)
    .bind(payload.function_id)
    .bind(payload.auth_type)
    .bind(payload.cors_enabled)
    .bind(id)
    .fetch_one(&pool)
    .await
    .map_err(db_err)?;
    
    Ok(ApiResponse::new(route))
}

pub async fn delete_gateway_route(
    Path(id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    let project_id = context.project_id.ok_or(ApiError::bad_request("missing_project_id"))?;

    let result = sqlx::query("DELETE FROM routes WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project_id)
        .execute(&pool)
        .await
        .map_err(db_err)?;

    if result.rows_affected() == 0 {
        return Err(ApiError::not_found("route_not_found"));
    }

    Ok(ApiResponse::new(serde_json::json!({ "success": true })))
}
