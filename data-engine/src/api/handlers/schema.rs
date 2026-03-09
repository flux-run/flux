use axum::{
    extract::{Query, State},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::Row;
use std::sync::Arc;

use crate::{
    engine::{auth_context::AuthContext, error::EngineError},
    router::DbRouter,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct SchemaQuery {
    pub database: Option<String>,
}

/// Single CTE query that returns tables, columns, relationships, and policies
/// as four JSON arrays in one round-trip to the database.
const SCHEMA_GRAPH_SQL: &str = r#"
WITH tbls AS (
    SELECT COALESCE(json_agg(
        json_build_object(
            'schema',      t.table_schema,
            'table',       t.table_name,
            'description', COALESCE(m.description, '')
        ) ORDER BY t.table_schema, t.table_name
    ), '[]'::json) AS data
    FROM information_schema.tables t
    LEFT JOIN fluxbase_internal.table_metadata m
      ON m.schema_name = t.table_schema AND m.table_name = t.table_name
    WHERE t.table_type = 'BASE TABLE'
      AND CASE
            WHEN $3::text IS NOT NULL THEN t.table_schema = $3
            ELSE t.table_schema LIKE $4
          END
),
cols AS (
    SELECT COALESCE(json_agg(
        json_build_object(
            'schema',          schema_name,
            'table',           table_name,
            'column',          column_name,
            'pg_type',         pg_type,
            'fb_type',         fb_type,
            'computed_expr',   computed_expr,
            'file_visibility', file_visibility
        ) ORDER BY schema_name, table_name, ordinal
    ), '[]'::json) AS data
    FROM fluxbase_internal.column_metadata
    WHERE tenant_id = $1 AND project_id = $2
      AND ($5::text IS NULL OR schema_name LIKE $5 || '%')
),
rels AS (
    SELECT COALESCE(json_agg(
        json_build_object(
            'id',           id,
            'schema',       schema_name,
            'from_table',   from_table,
            'from_column',  from_column,
            'to_table',     to_table,
            'to_column',    to_column,
            'relationship', relationship,
            'alias',        alias
        ) ORDER BY from_table, alias
    ), '[]'::json) AS data
    FROM fluxbase_internal.relationships
    WHERE tenant_id = $1 AND project_id = $2
),
pols AS (
    SELECT COALESCE(json_agg(
        json_build_object(
            'id',               id,
            'table',            table_name,
            'role',             role,
            'operation',        operation,
            'allowed_columns',  allowed_columns,
            'row_condition_sql', row_condition
        ) ORDER BY table_name, role
    ), '[]'::json) AS data
    FROM fluxbase_internal.policies
    WHERE tenant_id = $1 AND project_id = $2
)
SELECT
    (SELECT data FROM tbls)   AS tables,
    (SELECT data FROM cols)   AS columns,
    (SELECT data FROM rels)   AS relationships,
    (SELECT data FROM pols)   AS policies
"#;

/// GET /db/schema?database=main
///
/// Returns the full metadata for all tables in the project (or a specific
/// database), including columns, relationships, and policies.
/// Powers the dashboard table browser, CLI, and SDK code generation.
///
/// Uses a single CTE query so the database executes all four scans in one
/// round-trip instead of four, reducing introspect latency by ~3× RTT.
pub async fn introspect(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<SchemaQuery>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let request_id = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_owned();

    let auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    tracing::info!(
        request_id = %request_id,
        tenant_id  = %auth.tenant_id,
        project_id = %auth.project_id,
        "schema introspect start",
    );

    // $3 — exact schema name when a specific database is requested
    let schema_filter: Option<String> = if let Some(ref db) = params.database {
        Some(DbRouter::schema_name(&auth.tenant_slug, &auth.project_slug, db)?)
    } else {
        None
    };

    // $4 — LIKE prefix covering all schemas for this project (used when $3 IS NULL)
    let schema_prefix = format!(
        "t_{}_{}%",
        auth.tenant_slug.replace('-', "_"),
        auth.project_slug.replace('-', "_"),
    );

    let row = sqlx::query(SCHEMA_GRAPH_SQL)
        .bind(auth.tenant_id)                    // $1
        .bind(auth.project_id)                   // $2
        .bind(schema_filter.as_deref())          // $3 exact schema or NULL
        .bind(&schema_prefix)                    // $4 LIKE pattern
        .bind(params.database.as_deref())        // $5 column schema prefix filter
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!(request_id = %request_id, error = %e, "schema introspect query failed");
            EngineError::Db(e)
        })?;

    let tables:        serde_json::Value = row.get("tables");
    let columns:       serde_json::Value = row.get("columns");
    let relationships: serde_json::Value = row.get("relationships");
    let policies:      serde_json::Value = row.get("policies");

    Ok(Json(json!({
        "tables":        tables,
        "columns":       columns,
        "relationships": relationships,
        "policies":      policies,
        "functions":     [],
    })))
}
