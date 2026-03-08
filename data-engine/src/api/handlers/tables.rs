use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use std::collections::HashSet;
use std::sync::Arc;

use crate::{
    engine::{auth_context::AuthContext, error::EngineError},
    router::db_router::{quote_ident, validate_identifier, DbRouter},
    state::AppState,
};

// ─── Type allowlist (prevents arbitrary/dangerous type injection) ─────────────

static ALLOWED_TYPES: &[&str] = &[
    "text", "varchar", "char", "character varying",
    "integer", "int", "int4", "bigint", "int8", "smallint", "int2",
    "boolean", "bool",
    "uuid",
    "timestamptz", "timestamp with time zone", "timestamp", "timestamp without time zone",
    "date", "time",
    "jsonb", "json",
    "float4", "real", "float8", "double precision",
    "numeric", "decimal",
    "bytea",
    "serial", "bigserial",
];

fn validate_column_type(t: &str) -> Result<(), EngineError> {
    let lower = t.to_lowercase();
    // Also allow numeric(p,s) / varchar(n) patterns.
    let base = lower.split('(').next().unwrap_or("").trim();
    if ALLOWED_TYPES.contains(&base) {
        Ok(())
    } else {
        Err(EngineError::UnsupportedOperation(format!("column type '{}' is not allowed", t)))
    }
}

// ─── Request / response types ─────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ColumnDef {
    pub name: String,
    #[serde(rename = "type")]
    pub col_type: String,
    #[serde(default)]
    pub not_null: bool,
    #[serde(default)]
    pub primary_key: bool,
    #[serde(default)]
    pub unique: bool,
    pub default: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateTableRequest {
    pub database: String,
    pub name: String,
    pub columns: Vec<ColumnDef>,
}

// ─── POST /db/tables ─────────────────────────────────────────────────────────

pub async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<CreateTableRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;
    let schema = DbRouter::schema_name(&auth.tenant_slug, &auth.project_slug, &body.database)?;
    DbRouter::assert_exists(&state.pool, &schema).await?;

    validate_identifier(&body.name)?;
    if body.columns.is_empty() {
        return Err(EngineError::MissingField("at least one column required".into()));
    }

    // Validate all column defs before touching the DB.
    let mut pk_seen = false;
    let mut names_seen: HashSet<String> = HashSet::new();
    for col in &body.columns {
        validate_identifier(&col.name)?;
        validate_column_type(&col.col_type)?;
        if !names_seen.insert(col.name.clone()) {
            return Err(EngineError::MissingField(
                format!("duplicate column name '{}'", col.name),
            ));
        }
        if col.primary_key {
            pk_seen = true;
        }
    }

    // Build CREATE TABLE SQL.
    let mut col_defs: Vec<String> = vec![];
    let mut pk_cols: Vec<String> = vec![];

    for col in &body.columns {
        let mut def = format!("{} {}", quote_ident(&col.name), col.col_type);
        if col.not_null || col.primary_key {
            def.push_str(" NOT NULL");
        }
        if col.unique && !col.primary_key {
            def.push_str(" UNIQUE");
        }
        if let Some(ref dflt) = col.default {
            // Defaults are literals/function calls — we quote them as-is (trusted admin input).
            def.push_str(&format!(" DEFAULT {}", dflt));
        }
        if col.primary_key {
            pk_cols.push(quote_ident(&col.name));
        }
        col_defs.push(def);
    }
    if pk_seen {
        col_defs.push(format!("PRIMARY KEY ({})", pk_cols.join(", ")));
    }

    let create_sql = format!(
        "CREATE TABLE IF NOT EXISTS {}.{} ({})",
        quote_ident(&schema),
        quote_ident(&body.name),
        col_defs.join(", "),
    );

    let mut tx = state.pool.begin().await.map_err(EngineError::Db)?;
    sqlx::query(&create_sql).execute(&mut *tx).await.map_err(EngineError::Db)?;

    // Register in metadata registry.
    let columns_json = serde_json::to_value(&body.columns)
        .map_err(|e| EngineError::Internal(anyhow::anyhow!(e)))?;
    sqlx::query(
        "INSERT INTO fluxbase_internal.table_metadata \
             (tenant_id, project_id, schema_name, table_name, columns) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (tenant_id, project_id, schema_name, table_name) \
         DO UPDATE SET columns = EXCLUDED.columns, updated_at = now()",
    )
    .bind(auth.tenant_id)
    .bind(auth.project_id)
    .bind(&schema)
    .bind(&body.name)
    .bind(columns_json)
    .execute(&mut *tx)
    .await
    .map_err(EngineError::Db)?;

    tx.commit().await.map_err(EngineError::Db)?;

    Ok(Json(json!({
        "database": body.database,
        "table":    body.name,
        "status":   "created",
    })))
}

// ─── GET /db/tables/:database ────────────────────────────────────────────────

pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(database): Path<String>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;
    let schema = DbRouter::schema_name(&auth.tenant_slug, &auth.project_slug, &database)?;
    DbRouter::assert_exists(&state.pool, &schema).await?;

    let rows = sqlx::query(
        "SELECT table_name, columns FROM fluxbase_internal.table_metadata \
         WHERE tenant_id = $1 AND project_id = $2 AND schema_name = $3 \
         ORDER BY table_name",
    )
    .bind(auth.tenant_id)
    .bind(auth.project_id)
    .bind(&schema)
    .fetch_all(&state.pool)
    .await
    .map_err(EngineError::Db)?;

    let tables: Vec<serde_json::Value> = rows.iter().map(|r| {
        let name: String = r.get("table_name");
        let cols: serde_json::Value = r.get("columns");
        json!({ "name": name, "columns": cols })
    }).collect();

    Ok(Json(json!({ "database": database, "tables": tables })))
}

// ─── DELETE /db/tables/:database/:table ──────────────────────────────────────

pub async fn drop_table(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((database, table)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;
    let schema = DbRouter::schema_name(&auth.tenant_slug, &auth.project_slug, &database)?;
    DbRouter::assert_exists(&state.pool, &schema).await?;
    DbRouter::assert_table_exists(&state.pool, &schema, &table).await?;

    let drop_sql = format!(
        "DROP TABLE IF EXISTS {}.{} CASCADE",
        quote_ident(&schema),
        quote_ident(&table),
    );

    let mut tx = state.pool.begin().await.map_err(EngineError::Db)?;
    sqlx::query(&drop_sql).execute(&mut *tx).await.map_err(EngineError::Db)?;
    sqlx::query(
        "DELETE FROM fluxbase_internal.table_metadata \
         WHERE tenant_id = $1 AND project_id = $2 AND schema_name = $3 AND table_name = $4",
    )
    .bind(auth.tenant_id)
    .bind(auth.project_id)
    .bind(&schema)
    .bind(&table)
    .execute(&mut *tx)
    .await
    .map_err(EngineError::Db)?;
    tx.commit().await.map_err(EngineError::Db)?;

    Ok(Json(json!({ "database": database, "table": table, "status": "dropped" })))
}
