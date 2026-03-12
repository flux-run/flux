/// Unified schema graph endpoint.
///
/// Merges:
///   • tables / columns / relationships / policies  — from Data Engine
///   • function definitions (input + output JSON schemas) — from API DB
///
/// This is the single source of truth consumed by the SDK generator, the
/// TypeScript type emitter, and the dashboard type-checker.
use axum::{
    extract::{Extension, Query, State},
    http::HeaderMap,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;

use crate::{
    types::{
        context::RequestContext,
        response::{ApiError, ApiResponse},
    },
    AppState,
};

type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

#[derive(Debug, Deserialize)]
pub struct SchemaQuery {
    pub database: Option<String>,
}

/// Build a `reqwest::HeaderMap` forwarding the auth/context headers that the
/// Data Engine needs to scope its response to the right tenant/project.
pub fn forward_headers(headers: &HeaderMap) -> reqwest::header::HeaderMap {
    let mut map = reqwest::header::HeaderMap::new();
    for key in &[
        "authorization",
        "x-fluxbase-tenant",
        "x-fluxbase-project",
        "x-tenant-id",
        "x-project-id",
        "x-tenant-slug",
        "x-project-slug",
        "x-user-id",
        "x-user-role",
        "x-request-id",
        "x-flux-replay",
        "content-type",
    ] {
        if let Some(v) = headers.get(*key) {
            if let Ok(val) = reqwest::header::HeaderValue::from_bytes(v.as_bytes()) {
                if let Ok(name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
                    map.insert(name, val);
                }
            }
        }
    }

    // Data Engine contract requires x-tenant-id / x-project-id.
    // API clients send x-fluxbase-tenant / x-fluxbase-project, so mirror them.
    if !map.contains_key("x-tenant-id") {
        if let Some(v) = headers.get("x-fluxbase-tenant") {
            if let Ok(val) = reqwest::header::HeaderValue::from_bytes(v.as_bytes()) {
                map.insert("x-tenant-id", val);
            }
        }
    }
    if !map.contains_key("x-project-id") {
        if let Some(v) = headers.get("x-fluxbase-project") {
            if let Ok(val) = reqwest::header::HeaderValue::from_bytes(v.as_bytes()) {
                map.insert("x-project-id", val);
            }
        }
    }

    // Internal service token so the Data Engine trusts the call.
    let token = std::env::var("INTERNAL_SERVICE_TOKEN")
        .unwrap_or_else(|_| "fluxbase_secret_token".to_string());
    if let Ok(val) = reqwest::header::HeaderValue::from_str(&token) {
        map.insert("x-service-token", val);
    }
    map
}

// ─── Handler ─────────────────────────────────────────────────────────────────

/// GET /schema/graph?database=<optional>
///
/// Returns the unified schema graph for the authenticated project.
pub async fn graph(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    headers: HeaderMap,
    Query(params): Query<SchemaQuery>,
) -> ApiResult<Value> {
    let request_id = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_owned();

    let project_id = ctx.project_id;

    // ── 1. Fetch DB schema from Data Engine ───────────────────────────────
    let mut de_url = format!("{}/db/schema", state.data_engine_url);
    if let Some(ref db) = params.database {
        de_url.push_str(&format!("?database={}", db));
    }

    tracing::info!(
        request_id = %request_id,
        project_id = %project_id,
        de_url     = %de_url,
        "calling data-engine",
    );

    let de_resp = state
        .http_client
        .get(&de_url)
        .headers(forward_headers(&headers))
        .send()
        .await
        .map_err(|e| ApiError::internal(&format!("data_engine_unreachable: {}", e)))?;

    // Guard: surface Data Engine errors clearly rather than failing at JSON parse.
    if !de_resp.status().is_success() {
        let status = de_resp.status().as_u16();
        let body = de_resp.text().await.unwrap_or_default();
        tracing::error!(request_id = %request_id, status, body = %body, "data_engine returned error");
        return Err(ApiError::internal(&format!("data_engine_error({status}): {body}")));
    }

    let db_schema: Value = de_resp
        .json()
        .await
        .map_err(|e| ApiError::internal(&format!("data_engine_parse: {}", e)))?;

    // ── 2. Load function definitions from API DB ──────────────────────────
    let funcs = sqlx::query(
        "SELECT name, description, input_schema, output_schema \
         FROM functions WHERE project_id = $1 ORDER BY name",
    )
    .bind(project_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| ApiError::internal("db_error"))?;

    let functions: Vec<Value> = funcs
        .into_iter()
        .map(|f| {
            json!({
                "name":          f.get::<String, _>("name"),
                "description":   f.get::<Option<String>, _>("description"),
                "input_schema":  f.get::<Option<serde_json::Value>, _>("input_schema"),
                "output_schema": f.get::<Option<serde_json::Value>, _>("output_schema"),
            })
        })
        .collect();

    Ok(ApiResponse::new(json!({
        "tables":        db_schema.get("tables").cloned().unwrap_or(json!([])),
        "columns":       db_schema.get("columns").cloned().unwrap_or(json!([])),
        "relationships": db_schema.get("relationships").cloned().unwrap_or(json!([])),
        "policies":      db_schema.get("policies").cloned().unwrap_or(json!([])),
        "functions":     functions,
    })))
}
