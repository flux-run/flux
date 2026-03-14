//! GET /sdk/manifest — returns the full project manifest.
//!
//! The manifest is the single source of truth for `flux generate`. The CLI
//! calls this endpoint, writes `.flux/manifest.json`, then generates typed
//! ctx bindings for all supported languages.

use axum::extract::{Extension, Query, State};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    types::{context::RequestContext, response::ApiError},
    AppState,
};
use crate::error::ApiResponse;

#[derive(Deserialize)]
pub struct ManifestQuery {
    /// Optional database name for data-engine schema lookup.
    pub database: Option<String>,
}

pub async fn get_manifest(
    State(state): State<AppState>,
    Query(_params): Query<ManifestQuery>,
    _ctx: Option<Extension<RequestContext>>,
) -> Result<ApiResponse<Value>, ApiError> {
    let pool = &state.pool;

    // ── 1. Function contracts ──────────────────────────────────────────────
    let fn_rows = sqlx::query(
        "SELECT id, name, runtime, input_schema, output_schema \
         FROM functions \
         ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "manifest: failed to query functions");
        ApiError::internal("db_error")
    })?;

    let mut functions_map = serde_json::Map::new();
    for r in &fn_rows {
        let name: String = r.get("name");
        functions_map.insert(name, json!({
            "id":            r.get::<Uuid, _>("id"),
            "runtime":       r.get::<String, _>("runtime"),
            "input_schema":  r.try_get::<Option<Value>, _>("input_schema").ok().flatten(),
            "output_schema": r.try_get::<Option<Value>, _>("output_schema").ok().flatten(),
        }));
    }

    // ── 2. Secret keys (names only — never values) ────────────────────────
    let secret_keys: Vec<String> = sqlx::query_scalar(
        "SELECT key FROM secrets ORDER BY key",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "manifest: failed to query secrets");
        ApiError::internal("db_error")
    })?;

    // ── 3. DB table shapes ────────────────────────────────────────────────
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
        tracing::error!(error = %e, "manifest: failed to query information_schema");
        ApiError::internal("db_error")
    })?;

    let mut table_btree: std::collections::BTreeMap<String, Vec<Value>> =
        std::collections::BTreeMap::new();
    for row in &col_rows {
        let table: String    = row.get("table_name");
        let col: String      = row.get("column_name");
        let dtype: String    = row.get("data_type");
        let nullable: String = row.get("is_nullable");
        table_btree.entry(table).or_default().push(json!({
            "name":     col,
            "type":     dtype,
            "nullable": nullable == "YES",
        }));
    }

    let mut database_map = serde_json::Map::new();
    for (table, columns) in table_btree {
        database_map.insert(table, json!({ "columns": columns }));
    }

    // ── 4. Schema hash — SHA-256 of serialised functions + db_tables ──────
    let hash_input = format!(
        "{}{}",
        serde_json::to_string(&functions_map).unwrap_or_default(),
        serde_json::to_string(&database_map).unwrap_or_default(),
    );
    let digest = Sha256::digest(hash_input.as_bytes());
    let schema_hash = hex::encode(&digest[..4]); // 8 hex chars, same style as sdk.rs

    Ok(ApiResponse::new(json!({
        "version":      1,
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "schema_hash":  schema_hash,
        "database":     database_map,
        "functions":    functions_map,
        "secrets":      secret_keys,
    })))
}
