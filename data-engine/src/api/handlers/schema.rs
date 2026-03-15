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
/// as four JSONB arrays in one round-trip to the database.
///
/// Design notes:
/// - NOT MATERIALIZED: prevents Postgres 12+ from fencing CTEs as optimisation
///   barriers; the planner can inline and optimise each CTE freely.
/// - jsonb_agg / jsonb_build_object: JSONB is the native binary type; avoids
///   the internal text→binary conversion that json_agg incurs on the wire.
/// - COALESCE(..., '[]'::jsonb): guarantees an empty array, never NULL.
const SCHEMA_GRAPH_SQL: &str = r#"
WITH
tbls AS NOT MATERIALIZED (
    -- Use pg_catalog instead of information_schema.tables.
    -- information_schema views evaluate row-level visibility checks for every
    -- row in the entire cluster; on a 2000-table BYODB database that can cost
    -- 50-200 ms. pg_catalog uses a direct index lookup on pg_namespace.nspname
    -- and is O(1) regardless of how many tables exist in other schemas.
    SELECT COALESCE(jsonb_agg(
        jsonb_build_object(
            'schema',      n.nspname,
            'table',       c.relname,
            'description', COALESCE(m.description, '')
        ) ORDER BY n.nspname, c.relname
    ), '[]'::jsonb) AS data
    FROM pg_catalog.pg_class c
    JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
    LEFT JOIN flux_internal.table_metadata m
      ON m.schema_name = n.nspname AND m.table_name = c.relname
    WHERE c.relkind = 'r'
      AND n.nspname NOT IN ('pg_catalog', 'information_schema', 'flux_internal')
      AND n.nspname NOT LIKE 'pg_%'
      AND ($1::text IS NULL OR n.nspname = $1)
),
cols AS NOT MATERIALIZED (
    SELECT COALESCE(jsonb_agg(
        jsonb_build_object(
            'schema',          schema_name,
            'table',           table_name,
            'column',          column_name,
            'pg_type',         pg_type,
            'fb_type',         fb_type,
            'computed_expr',   computed_expr,
            'file_visibility', file_visibility
        ) ORDER BY schema_name, table_name, ordinal
    ), '[]'::jsonb) AS data
    FROM flux_internal.column_metadata
    WHERE ($1::text IS NULL OR schema_name = $1)
),
rels AS NOT MATERIALIZED (
    SELECT COALESCE(jsonb_agg(
        jsonb_build_object(
            'id',           id,
            'schema',       schema_name,
            'from_table',   from_table,
            'from_column',  from_column,
            'to_table',     to_table,
            'to_column',    to_column,
            'relationship', relationship,
            'alias',        alias
        ) ORDER BY from_table, alias
    ), '[]'::jsonb) AS data
    FROM flux_internal.relationships
    WHERE ($1::text IS NULL OR schema_name = $1)
),
pols AS NOT MATERIALIZED (
    SELECT COALESCE(jsonb_agg(
        jsonb_build_object(
            'id',                id,
            'table',             table_name,
            'role',              role,
            'operation',         operation,
            'allowed_columns',   allowed_columns,
            'row_condition_sql', row_condition
        ) ORDER BY table_name, role
    ), '[]'::jsonb) AS data
    FROM flux_internal.policies
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

    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    tracing::info!(request_id = %request_id, "schema introspect start");

    // $1 — exact schema name when a specific database is requested (NULL = all)
    let schema_filter: Option<String> = if let Some(ref db) = params.database {
        Some(DbRouter::schema_name(db)?)
    } else {
        None
    };

    let row = sqlx::query(SCHEMA_GRAPH_SQL)
        .bind(schema_filter.as_deref())    // $1 exact schema or NULL
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
