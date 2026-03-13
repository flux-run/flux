use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use crate::error::{ApiResponse, ApiError};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
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
    pub is_async: bool,
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
    #[serde(default)]
    pub is_async: bool,
    pub auth_type: String,
    pub cors_enabled: bool,
    pub rate_limit: Option<i32>,
}

#[derive(Deserialize)]
pub struct UpdateRoutePayload {
    pub path: Option<String>,
    pub method: Option<String>,
    pub function_id: Option<Uuid>,
    pub is_async: Option<bool>,
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
        "SELECT id, project_id, path, method, function_id, is_async, auth_type, cors_enabled, rate_limit, created_at \
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
    let project_id = context.project_id;

    let id = Uuid::new_v4();
    
    let route = sqlx::query_as::<_, RouteRow>(
           "INSERT INTO routes (id, project_id, path, method, function_id, is_async, auth_type, cors_enabled, rate_limit) \
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
            RETURNING id, project_id, path, method, function_id, is_async, auth_type, cors_enabled, rate_limit, created_at"
    )
    .bind(id)
    .bind(project_id)
    .bind(payload.path)
    .bind(payload.method)
    .bind(payload.function_id)
    .bind(payload.is_async)
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
    let project_id = context.project_id;

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
            is_async = COALESCE($4, is_async), \
            auth_type = COALESCE($5, auth_type), \
            cors_enabled = COALESCE($6, cors_enabled) \
            WHERE id = $7 \
            RETURNING id, project_id, path, method, function_id, is_async, auth_type, cors_enabled, rate_limit, created_at"
    )
    .bind(payload.path)
    .bind(payload.method)
    .bind(payload.function_id)
        .bind(payload.is_async)
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
    let project_id = context.project_id;

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

// ── Gateway extras ─────────────────────────────────────────────────────────

#[derive(sqlx::FromRow, Serialize)]
pub struct RouteFullRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub path: String,
    pub method: String,
    pub function_id: Uuid,
    pub is_async: bool,
    pub auth_type: String,
    pub cors_enabled: bool,
    pub rate_limit: Option<i32>,
    pub created_at: chrono::NaiveDateTime,
    pub jwks_url: Option<String>,
    pub jwt_audience: Option<String>,
    pub jwt_issuer: Option<String>,
    pub cors_origins: Option<Vec<String>>,
    pub cors_headers: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub struct RateLimitPayload {
    pub requests_per_second: i32,
}

#[derive(Deserialize)]
pub struct CorsPayload {
    pub origins: Vec<String>,
    pub headers: Vec<String>,
}

#[derive(Deserialize)]
pub struct MiddlewareCreatePayload {
    pub route_id: Uuid,
    #[serde(rename = "type")]
    pub middleware_type: String,
    pub config: serde_json::Value,
}

pub async fn get_gateway_route_by_id(
    Path(id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<RouteFullRow> {
    let project_id = context.project_id;
    let row = sqlx::query_as::<_, RouteFullRow>(
        "SELECT id, project_id, path, method, function_id, is_async, auth_type, cors_enabled, \
         rate_limit, created_at, jwks_url, jwt_audience, jwt_issuer, cors_origins, cors_headers \
         FROM routes WHERE id = $1 AND project_id = $2",
    )
    .bind(id)
    .bind(project_id)
    .fetch_optional(&pool)
    .await
    .map_err(db_err)?
    .ok_or_else(|| ApiError::not_found("route_not_found"))?;

    Ok(ApiResponse::new(row))
}

pub async fn set_rate_limit(
    Path(id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
    Json(payload): Json<RateLimitPayload>,
) -> ApiResult<serde_json::Value> {
    sqlx::query("UPDATE routes SET rate_limit = $1 WHERE id = $2 AND project_id = $3")
        .bind(payload.requests_per_second)
        .bind(id)
        .bind(context.project_id)
        .execute(&pool)
        .await
        .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "success": true })))
}

pub async fn delete_rate_limit(
    Path(id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    sqlx::query("UPDATE routes SET rate_limit = NULL WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(context.project_id)
        .execute(&pool)
        .await
        .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "success": true })))
}

pub async fn get_cors(
    Path(id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    let row = sqlx::query("SELECT cors_origins, cors_headers FROM routes WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(context.project_id)
        .fetch_optional(&pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| ApiError::not_found("route_not_found"))?;

    let origins: Option<Vec<String>> = row.try_get("cors_origins").unwrap_or(None);
    let headers: Option<Vec<String>> = row.try_get("cors_headers").unwrap_or(None);

    Ok(ApiResponse::new(serde_json::json!({
        "cors_origins": origins,
        "cors_headers": headers,
    })))
}

pub async fn set_cors(
    Path(id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
    Json(payload): Json<CorsPayload>,
) -> ApiResult<serde_json::Value> {
    sqlx::query(
        "UPDATE routes SET cors_origins = $1, cors_headers = $2 WHERE id = $3 AND project_id = $4",
    )
    .bind(&payload.origins)
    .bind(&payload.headers)
    .bind(id)
    .bind(context.project_id)
    .execute(&pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "success": true })))
}

pub async fn create_middleware(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
    Json(payload): Json<MiddlewareCreatePayload>,
) -> ApiResult<serde_json::Value> {
    if payload.middleware_type == "jwt" {
        let jwks_url = payload.config.get("jwks_url").and_then(|v| v.as_str()).map(String::from);
        let audience = payload.config.get("audience").and_then(|v| v.as_str()).map(String::from);
        let issuer = payload.config.get("issuer").and_then(|v| v.as_str()).map(String::from);

        sqlx::query(
            "UPDATE routes SET jwks_url = $1, jwt_audience = $2, jwt_issuer = $3 \
             WHERE id = $4 AND project_id = $5",
        )
        .bind(jwks_url)
        .bind(audience)
        .bind(issuer)
        .bind(payload.route_id)
        .bind(context.project_id)
        .execute(&pool)
        .await
        .map_err(db_err)?;
    }

    Ok(ApiResponse::new(serde_json::json!({ "success": true })))
}

pub async fn delete_middleware(
    Path((route_id, middleware_type)): Path<(Uuid, String)>,
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    if middleware_type == "jwt" {
        sqlx::query(
            "UPDATE routes SET jwks_url = NULL, jwt_audience = NULL, jwt_issuer = NULL \
             WHERE id = $1 AND project_id = $2",
        )
        .bind(route_id)
        .bind(context.project_id)
        .execute(&pool)
        .await
        .map_err(db_err)?;
    }

    Ok(ApiResponse::new(serde_json::json!({ "success": true })))
}
