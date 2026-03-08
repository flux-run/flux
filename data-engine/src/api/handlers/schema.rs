use axum::{
    extract::{Query, State},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;
use serde_json::json;
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

/// GET /db/schema?database=main
///
/// Returns the full metadata for all tables in the project (or a specific
/// database), including columns, relationships, and policies.
/// Powers the dashboard table browser, CLI, and SDK code generation.
pub async fn introspect(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<SchemaQuery>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    // Determine schema name (or all schemas for this project).
    let schema_filter: Option<String> = if let Some(ref db) = params.database {
        Some(DbRouter::schema_name(&auth.tenant_slug, &auth.project_slug, db)?)
    } else {
        None
    };

    let tables = fetch_tables(&state.pool, &auth, schema_filter.as_deref()).await?;
    let columns = fetch_columns(&state.pool, &auth, params.database.as_deref()).await?;
    let relationships = fetch_relationships(&state.pool, &auth, params.database.as_deref()).await?;
    let policies = fetch_policies(&state.pool, &auth, params.database.as_deref()).await?;

    Ok(Json(json!({
        "tables":        tables,
        "columns":       columns,
        "relationships": relationships,
        "policies":      policies,
    })))
}

// ─── Sub-queries ──────────────────────────────────────────────────────────────

async fn fetch_tables(
    pool: &sqlx::PgPool,
    auth: &AuthContext,
    schema_filter: Option<&str>,
) -> Result<serde_json::Value, EngineError> {
    use sqlx::Row;

    // Pull from information_schema so we get every table including those created
    // outside the Fluxbase API. Supplement with fluxbase_internal.table_metadata.
    let prefix = format!(
        "t_{}_{}",
        auth.tenant_slug.replace('-', "_"),
        auth.project_slug.replace('-', "_")
    );

    let rows = if let Some(schema) = schema_filter {
        sqlx::query(
            "SELECT t.table_schema, t.table_name, \
                    COALESCE(m.description, '') AS description \
             FROM information_schema.tables t \
             LEFT JOIN fluxbase_internal.table_metadata m \
               ON m.schema_name = t.table_schema AND m.table_name = t.table_name \
             WHERE t.table_schema = $1 AND t.table_type = 'BASE TABLE' \
             ORDER BY t.table_name",
        )
        .bind(schema)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query(
            "SELECT t.table_schema, t.table_name, \
                    COALESCE(m.description, '') AS description \
             FROM information_schema.tables t \
             LEFT JOIN fluxbase_internal.table_metadata m \
               ON m.schema_name = t.table_schema AND m.table_name = t.table_name \
             WHERE t.table_schema LIKE $1 AND t.table_type = 'BASE TABLE' \
             ORDER BY t.table_schema, t.table_name",
        )
        .bind(format!("{}%", prefix))
        .fetch_all(pool)
        .await
    }
    .map_err(EngineError::Db)?;

    Ok(json!(rows
        .iter()
        .map(|r| json!({
            "schema":      r.get::<String, _>("table_schema"),
            "table":       r.get::<String, _>("table_name"),
            "description": r.get::<String, _>("description"),
        }))
        .collect::<Vec<_>>()))
}

async fn fetch_columns(
    pool: &sqlx::PgPool,
    auth: &AuthContext,
    database: Option<&str>,
) -> Result<serde_json::Value, EngineError> {
    use sqlx::Row;

    let rows = sqlx::query(
        "SELECT schema_name, table_name, column_name, pg_type, fb_type, \
                computed_expr, file_visibility, ordinal \
         FROM fluxbase_internal.column_metadata \
         WHERE tenant_id = $1 AND project_id = $2 \
           AND ($3::text IS NULL OR schema_name LIKE $3 || '%') \
         ORDER BY schema_name, table_name, ordinal",
    )
    .bind(auth.tenant_id)
    .bind(auth.project_id)
    .bind(database)
    .fetch_all(pool)
    .await
    .map_err(EngineError::Db)?;

    Ok(json!(rows
        .iter()
        .map(|r| json!({
            "schema":      r.get::<String, _>("schema_name"),
            "table":       r.get::<String, _>("table_name"),
            "column":      r.get::<String, _>("column_name"),
            "pg_type":     r.get::<String, _>("pg_type"),
            "fb_type":     r.get::<String, _>("fb_type"),
            "computed_expr":    r.get::<Option<String>, _>("computed_expr"),
            "file_visibility":  r.get::<Option<String>, _>("file_visibility"),
        }))
        .collect::<Vec<_>>()))
}

async fn fetch_relationships(
    pool: &sqlx::PgPool,
    auth: &AuthContext,
    _database: Option<&str>,
) -> Result<serde_json::Value, EngineError> {
    use sqlx::Row;
    use uuid::Uuid;

    let rows = sqlx::query(
        "SELECT id, schema_name, from_table, from_column, to_table, to_column, \
                relationship, alias \
         FROM fluxbase_internal.relationships \
         WHERE tenant_id = $1 AND project_id = $2 \
         ORDER BY from_table, alias",
    )
    .bind(auth.tenant_id)
    .bind(auth.project_id)
    .fetch_all(pool)
    .await
    .map_err(EngineError::Db)?;

    Ok(json!(rows
        .iter()
        .map(|r| json!({
            "id":           r.get::<Uuid, _>("id"),
            "schema":       r.get::<String, _>("schema_name"),
            "from_table":   r.get::<String, _>("from_table"),
            "from_column":  r.get::<String, _>("from_column"),
            "to_table":     r.get::<String, _>("to_table"),
            "to_column":    r.get::<String, _>("to_column"),
            "relationship": r.get::<String, _>("relationship"),
            "alias":        r.get::<String, _>("alias"),
        }))
        .collect::<Vec<_>>()))
}

async fn fetch_policies(
    pool: &sqlx::PgPool,
    auth: &AuthContext,
    _database: Option<&str>,
) -> Result<serde_json::Value, EngineError> {
    use sqlx::Row;
    use uuid::Uuid;

    let rows = sqlx::query(
        "SELECT id, table_name, role_name, operation, allowed_columns, \
                row_condition_sql \
         FROM fluxbase_internal.policies \
         WHERE tenant_id = $1 AND project_id = $2 \
         ORDER BY table_name, role_name",
    )
    .bind(auth.tenant_id)
    .bind(auth.project_id)
    .fetch_all(pool)
    .await
    .map_err(EngineError::Db)?;

    Ok(json!(rows
        .iter()
        .map(|r| json!({
            "id":               r.get::<Uuid, _>("id"),
            "table":            r.get::<String, _>("table_name"),
            "role":             r.get::<String, _>("role_name"),
            "operation":        r.get::<String, _>("operation"),
            "allowed_columns":  r.get::<serde_json::Value, _>("allowed_columns"),
            "row_condition_sql": r.get::<Option<String>, _>("row_condition_sql"),
        }))
        .collect::<Vec<_>>()))
}
