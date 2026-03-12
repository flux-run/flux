/// GET /internal/introspect?project_id=<uuid>&tenant_id=<uuid>
///
/// Returns the full type-contract envelope for a project:
///
/// ```json
/// {
///   "functions": [
///     { "id": "<uuid>", "name": "hello", "runtime": "deno",
///       "input_schema": { ... }, "output_schema": { ... } }
///   ],
///   "secrets": ["STRIPE_KEY", "OPENAI_API_KEY"],
///   "db_tables": [
///     { "table": "users", "columns": [{ "name": "id", "type": "uuid", "nullable": false }, ...] }
///   ]
/// }
/// ```
///
/// Callers:
/// - SDK codegen: derives TypeScript interfaces from function contracts
/// - CLI `flux why` agent context: knows the full shape of the project
/// - AI agents: single-call context loading — no source-file reading needed
/// - Runtime gateway: pre-warms schema cache on cold-start
///
/// Protected by the service-token middleware applied to the entire
/// `/internal/*` router — no per-handler auth check required.
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    types::response::{ApiError, ApiResponse},
    AppState,
};

#[derive(Deserialize)]
pub struct IntrospectQuery {
    /// Target project UUID.
    pub project_id: Uuid,
    /// Tenant UUID — required to scope the secrets lookup.
    pub tenant_id: Uuid,
}

pub async fn get_project_introspect(
    State(state): State<AppState>,
    Query(params): Query<IntrospectQuery>,
) -> Result<ApiResponse<Value>, ApiError> {
    let pool = &state.pool;

    // ── 1. Function contracts ──────────────────────────────────────────────
    //
    // Returns each function's name, runtime, and the JSON Schema stored on
    // the most recent active deployment.  `input_schema` / `output_schema`
    // may be null for functions deployed before schema support was added.
    let fn_rows = sqlx::query(
        "SELECT id, name, runtime, input_schema, output_schema \
         FROM functions \
         WHERE project_id = $1 \
         ORDER BY name",
    )
    .bind(params.project_id)
    .fetch_all(pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "introspect: failed to query functions");
        ApiError::internal("db_error")
    })?;

    let functions: Vec<Value> = fn_rows
        .iter()
        .map(|r| {
            json!({
                "id":            r.get::<Uuid, _>("id"),
                "name":          r.get::<String, _>("name"),
                "runtime":       r.get::<String, _>("runtime"),
                "input_schema":  r.try_get::<Option<Value>, _>("input_schema").ok().flatten(),
                "output_schema": r.try_get::<Option<Value>, _>("output_schema").ok().flatten(),
            })
        })
        .collect();

    // ── 2. Secret keys (names only — never values) ────────────────────────
    //
    // Returns all secret keys visible to this project: project-scoped keys
    // AND tenant-scoped keys (project_id IS NULL).  Values are never returned.
    let secret_keys: Vec<String> = sqlx::query_scalar(
        "SELECT key FROM secrets \
         WHERE tenant_id = $1 \
           AND (project_id = $2 OR project_id IS NULL) \
         ORDER BY key",
    )
    .bind(params.tenant_id)
    .bind(params.project_id)
    .fetch_all(pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "introspect: failed to query secrets");
        ApiError::internal("db_error")
    })?;

    // ── 3. DB table shapes ────────────────────────────────────────────────
    //
    // Reads information_schema.columns for the `public` schema.  Excludes
    // Postgres system schemas and the internal `flux` platform schema so
    // only user-created tables are surfaced.
    let col_rows = sqlx::query(
        "SELECT table_name, column_name, data_type, is_nullable \
         FROM information_schema.columns \
         WHERE table_schema = 'public' \
           AND table_schema NOT IN ('information_schema', 'pg_catalog', 'pg_toast', 'flux') \
         ORDER BY table_name, ordinal_position",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "introspect: failed to query information_schema");
        ApiError::internal("db_error")
    })?;

    // Group columns by table, preserving ordinal order (the SQL ORDER BY guarantees this).
    let mut table_map: std::collections::BTreeMap<String, Vec<Value>> =
        std::collections::BTreeMap::new();
    for row in &col_rows {
        let table: String    = row.get("table_name");
        let col: String      = row.get("column_name");
        let dtype: String    = row.get("data_type");
        let nullable: String = row.get("is_nullable");
        table_map.entry(table).or_default().push(json!({
            "name":     col,
            "type":     dtype,
            "nullable": nullable == "YES",
        }));
    }

    let db_tables: Vec<Value> = table_map
        .into_iter()
        .map(|(table, columns)| json!({ "table": table, "columns": columns }))
        .collect();

    Ok(ApiResponse::new(json!({
        "functions": functions,
        "secrets":   secret_keys,
        "db_tables": db_tables,
    })))
}
