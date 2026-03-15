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
    Json,
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
    let token = crate::middleware::require_secret(
        "INTERNAL_SERVICE_TOKEN",
        "dev-service-token",
        "Internal service token (INTERNAL_SERVICE_TOKEN)",
    );
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
    Extension(_ctx): Extension<RequestContext>,
    headers: HeaderMap,
    Query(params): Query<SchemaQuery>,
) -> ApiResult<Value> {
    let request_id = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_owned();

    // ── 1. Fetch DB schema from Data Engine ───────────────────────────────
    let mut de_url = format!("{}/db/schema", state.data_engine_url);
    if let Some(ref db) = params.database {
        de_url.push_str(&format!("?database={}", db));
    }

    tracing::info!(
        request_id = %request_id,
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
         FROM flux.functions ORDER BY name",
    )
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

// ── push_schema ───────────────────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
pub struct SchemaManifest {
    pub table: String,
    pub file: Option<String>,
    pub columns: serde_json::Value,
    pub indexes: Option<serde_json::Value>,
    pub foreign_keys: Option<serde_json::Value>,
    pub rules: Option<serde_json::Value>,
    pub hooks: Option<serde_json::Value>,
    pub on: Option<serde_json::Value>,
}

pub async fn push_schema(
    State(state): State<AppState>,
    Json(manifest): Json<SchemaManifest>,
) -> ApiResult<serde_json::Value> {
    // Ensure schema_rules column exists
    sqlx::query(
        "ALTER TABLE flux_internal.table_metadata \
         ADD COLUMN IF NOT EXISTS schema_rules JSONB",
    )
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = ?e, "push_schema migrate error");
        ApiError::internal("migrate_error")
    })?;

    // Upsert table metadata
    sqlx::query(
        "INSERT INTO flux_internal.table_metadata \
         (schema_name, table_name, columns, schema_rules, updated_at) \
         VALUES ('public', $1, $2, $3, now()) \
         ON CONFLICT (schema_name, table_name) \
         DO UPDATE SET columns = EXCLUDED.columns, schema_rules = EXCLUDED.schema_rules, updated_at = now()",
    )
    .bind(&manifest.table)
    .bind(&manifest.columns)
    .bind(&manifest.rules)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(error = ?e, "push_schema upsert error");
        ApiError::internal("upsert_error")
    })?;

    // Process hooks — two kinds:
    //   • UUID string → function hook (invoke deployed function)
    //   • JSON object → TransformExpr hook (evaluated in Rust, zero invocation overhead)
    if let Some(hooks_val) = &manifest.hooks {
        if let Some(hooks_obj) = hooks_val.as_object() {
            for (event_name, fn_arr) in hooks_obj {
                if let Some(arr) = fn_arr.as_array() {
                    for fn_item in arr {
                        if let Some(fn_id_str) = fn_item.as_str() {
                            // UUID string → function hook
                            if let Ok(function_id) = uuid::Uuid::parse_str(fn_id_str) {
                                sqlx::query(
                                    "INSERT INTO flux_internal.hooks \
                                     (table_name, event, function_id) \
                                     VALUES ($1, $2, $3) \
                                     ON CONFLICT (table_name, event) \
                                     DO UPDATE SET function_id = EXCLUDED.function_id, \
                                                   transform_expr = NULL, enabled = true",
                                )
                                .bind(&manifest.table)
                                .bind(event_name)
                                .bind(function_id)
                                .execute(&state.pool)
                                .await
                                .map_err(|e| {
                                    tracing::error!(error = ?e, "push_schema function hook upsert error");
                                    ApiError::internal("hook_upsert_error")
                                })?;
                            }
                        } else if fn_item.is_object() {
                            // JSON object → TransformExpr hook (compiled TypeScript transform)
                            sqlx::query(
                                "INSERT INTO flux_internal.hooks \
                                 (table_name, event, transform_expr) \
                                 VALUES ($1, $2, $3) \
                                 ON CONFLICT (table_name, event) \
                                 DO UPDATE SET transform_expr = EXCLUDED.transform_expr, \
                                               function_id = NULL, enabled = true",
                            )
                            .bind(&manifest.table)
                            .bind(event_name)
                            .bind(fn_item)
                            .execute(&state.pool)
                            .await
                            .map_err(|e| {
                                tracing::error!(error = ?e, "push_schema transform hook upsert error");
                                ApiError::internal("hook_upsert_error")
                            })?;
                        }
                    }
                }
            }
        }
    }

    // Process event subscriptions (on field)
    if let Some(on_val) = &manifest.on {
        if let Some(on_arr) = on_val.as_array() {
            for item in on_arr {
                let event_pattern = item
                    .get("event_pattern")
                    .and_then(|v| v.as_str())
                    .unwrap_or("*");
                let target_config = item
                    .get("target_config")
                    .cloned()
                    .unwrap_or(serde_json::Value::Object(Default::default()));

                sqlx::query(
                    "INSERT INTO flux_internal.event_subscriptions \
                     (event_pattern, target_type, target_config) \
                     VALUES ($1, 'function', $2) \
                     ON CONFLICT DO NOTHING",
                )
                .bind(event_pattern)
                .bind(&target_config)
                .execute(&state.pool)
                .await
                .map_err(|e| {
                    tracing::error!(error = ?e, "push_schema subscription upsert error");
                    ApiError::internal("subscription_upsert_error")
                })?;
            }
        }
    }

    Ok(ApiResponse::new(serde_json::json!({
        "status": "applied",
        "table": manifest.table,
    })))
}
