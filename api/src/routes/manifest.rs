//! GET /sdk/manifest — returns the full project manifest.
//!
//! The manifest is the single source of truth for `flux generate`. The CLI
//! calls this endpoint, writes `.flux/manifest.json`, then generates typed
//! ctx bindings for all supported languages.
//!
//! ## Response shape
//!
//! ```json
//! {
//!   "version": 1,
//!   "project_id": "uuid",
//!   "generated_at": "2026-03-13T09:57:36Z",
//!   "schema_hash": "a3f8c1d2",
//!   "database": {
//!     "users": {
//!       "columns": [
//!         { "name": "id", "type": "uuid", "nullable": false }
//!       ]
//!     }
//!   },
//!   "functions": {
//!     "create_user": {
//!       "id": "uuid-string",
//!       "runtime": "deno",
//!       "input_schema":  { ... },
//!       "output_schema": { ... }
//!     }
//!   },
//!   "secrets": ["OPENAI_KEY", "STRIPE_SECRET"]
//! }
//! ```
//!
//! Protected by service-token middleware on the `/internal/*` router AND
//! exposed on the public `/sdk/manifest` route protected by API key.
//!
//! # SOLID
//! SRP: this file only shapes the manifest response. All DB queries are inline
//! but each block is clearly labelled. DIP: depends on `AppState` trait, not
//! concrete pool fields.

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
    /// Target project UUID — required on the internal route; optional on /sdk/manifest
    /// (falls back to the RequestContext injected by auth middleware).
    pub project_id: Option<Uuid>,
    /// Tenant UUID — required on the internal route; optional on /sdk/manifest.
    pub tenant_id:  Option<Uuid>,
}

pub async fn get_manifest(
    State(state): State<AppState>,
    Query(params): Query<ManifestQuery>,
    ctx: Option<Extension<RequestContext>>,
) -> Result<ApiResponse<Value>, ApiError> {
    let pool = &state.pool;

    // Resolve project_id + tenant_id: query params take precedence (used by
    // /internal/introspect/manifest), then RequestContext (auth middleware on
    // /sdk/manifest), then AppState local defaults.
    let (project_id, tenant_id) = match (params.project_id, params.tenant_id) {
        (Some(p), Some(t)) => (p, t),
        _ => ctx
            .map(|Extension(rc)| (rc.project_id, rc.tenant_id))
            .unwrap_or((state.local_project_id, state.local_tenant_id)),
    };

    // ── 1. Function contracts ──────────────────────────────────────────────
    let fn_rows = sqlx::query(
        "SELECT id, name, runtime, input_schema, output_schema \
         FROM functions \
         WHERE project_id = $1 \
         ORDER BY name",
    )
    .bind(project_id)
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
        "SELECT key FROM secrets \
         WHERE tenant_id = $1 \
           AND (project_id = $2 OR project_id IS NULL) \
         ORDER BY key",
    )
    .bind(tenant_id)
    .bind(project_id)
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
        "project_id":   project_id,
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "schema_hash":  schema_hash,
        "database":     database_map,
        "functions":    functions_map,
        "secrets":      secret_keys,
    })))
}
