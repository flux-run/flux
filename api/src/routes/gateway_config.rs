//! Gateway routing configuration — CRUD endpoints for route sync and lookup.
//!
//! ## Endpoints
//! - `POST /routes/sync`       — replace all active routes for a project (called by `flux deploy`)
//! - `GET  /routes`            — list active routes for the authenticated project
//! - `GET  /internal/routes`   — load route table for the gateway (no project auth required)

use axum::{
    extract::{Extension, Query, State},
    Json,
};
use crate::error::{ApiError, ApiResponse, ApiResult};
use crate::types::context::RequestContext;
use crate::validation::{validate_route_path, PaginationQuery};
use crate::AppState;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Row structs ───────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow, Serialize)]
struct RouteConfigRow {
    id:                   Uuid,
    path:                 String,
    method:               String,
    function_name:        String,
    middleware:           Vec<String>,
    rate_limit_per_minute: Option<i32>,
}

// ── Payloads ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SyncRoutesPayload {
    pub project_deployment_id: Option<Uuid>,
    pub routes: Vec<RoutePayloadEntry>,
}

#[derive(Deserialize)]
pub struct RoutePayloadEntry {
    pub path:                  String,
    pub method:                String,
    pub function_name:         String,
    #[serde(default)]
    pub middleware:            Vec<String>,
    pub rate_limit_per_minute: Option<i32>,
}

#[derive(Deserialize)]
pub struct GatewayRoutesQuery {
    pub project_id: Uuid,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `POST /routes/sync` — Replace all active routes for the project with the new set.
/// Called by `flux deploy` after a successful deployment.
pub async fn sync_routes(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Json(payload): Json<SyncRoutesPayload>,
) -> ApiResult<serde_json::Value> {
    let project_id = context.project_id;

    // Validate all route paths before touching the DB.
    for route in &payload.routes {
        validate_route_path(&route.path)
            .map_err(|e| ApiError::bad_request(format!("invalid route path {:?}: {}", route.path, e)))?;
    }

    let mut tx = state.pool.begin().await.map_err(ApiError::from)?;

    // Deactivate all current active routes for this project.
    sqlx::query(
        "UPDATE flux.routes SET is_active = false \
         WHERE project_id = $1 AND is_active = true",
    )
    .bind(project_id)
    .execute(&mut *tx)
    .await
    .map_err(ApiError::from)?;

    // Insert each new route as active.
    let count = payload.routes.len();
    for route in &payload.routes {
        sqlx::query(
            "INSERT INTO flux.routes \
               (id, project_id, project_deployment_id, path, method, function_name, middleware, rate_limit_per_minute, is_active) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, true)",
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(payload.project_deployment_id)
        .bind(&route.path)
        .bind(&route.method)
        .bind(&route.function_name)
        .bind(&route.middleware)
        .bind(route.rate_limit_per_minute)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::from)?;
    }

    tx.commit().await.map_err(ApiError::from)?;

    Ok(ApiResponse::new(serde_json::json!({ "synced": count })))
}

/// `GET /routes` — List all active routes for the authenticated project.
pub async fn list_routes(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Query(page): Query<PaginationQuery>,
) -> ApiResult<serde_json::Value> {
    let project_id = context.project_id;
    let (limit, offset) = page.clamped();

    let rows = sqlx::query_as::<_, RouteConfigRow>(
        "SELECT id, path, method, function_name, middleware, rate_limit_per_minute \
         FROM flux.routes \
         WHERE project_id = $1 AND is_active = true \
         ORDER BY path, method \
         LIMIT $2 OFFSET $3",
    )
    .bind(project_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::from)?;

    Ok(ApiResponse::new(serde_json::json!({ "routes": rows, "limit": limit, "offset": offset })))
}

/// `GET /internal/routes?project_id=uuid` — Load the route table for the gateway.
/// No project auth context required; the gateway calls this on startup and periodically.
pub async fn get_routes_for_gateway(
    State(state): State<AppState>,
    Query(params): Query<GatewayRoutesQuery>,
) -> Result<ApiResponse<serde_json::Value>, ApiError> {
    let rows = sqlx::query_as::<_, RouteConfigRow>(
        "SELECT id, path, method, function_name, middleware, rate_limit_per_minute \
         FROM flux.routes \
         WHERE project_id = $1 AND is_active = true \
         ORDER BY path, method",
    )
    .bind(params.project_id)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::from)?;

    Ok(ApiResponse::new(serde_json::json!({ "routes": rows })))
}
