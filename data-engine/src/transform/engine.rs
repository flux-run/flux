use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::engine::{auth_context::AuthContext, error::EngineError};
use crate::file_engine::FileEngine;

/// Metadata for a single column, loaded from `fluxbase_internal.column_metadata`.
#[derive(Debug, Clone)]
pub struct ColumnMeta {
    pub name: String,
    /// Postgres base type.
    pub pg_type: String,
    /// Extended Fluxbase type: "default" | "file" | "computed" | "relation".
    pub fb_type: String,
    pub computed_expr: Option<String>,
    pub file_visibility: Option<String>,
}

pub struct TransformEngine;

impl TransformEngine {
    /// Load column metadata for a given table from the registry.
    ///
    /// Returns an empty vec when no metadata is registered (e.g. the table was
    /// created outside the Fluxbase API and has no extended type information).
    pub async fn load_columns(
        pool: &PgPool,
        tenant_id: Uuid,
        project_id: Uuid,
        schema: &str,
        table: &str,
    ) -> Result<Vec<ColumnMeta>, EngineError> {
        let rows = sqlx::query(
            "SELECT column_name, pg_type, fb_type, computed_expr, file_visibility \
             FROM fluxbase_internal.column_metadata \
             WHERE tenant_id = $1 AND project_id = $2 \
               AND schema_name = $3 AND table_name = $4 \
             ORDER BY ordinal",
        )
        .bind(tenant_id)
        .bind(project_id)
        .bind(schema)
        .bind(table)
        .fetch_all(pool)
        .await
        .map_err(EngineError::Db)?;

        Ok(rows
            .iter()
            .map(|r| ColumnMeta {
                name: r.get("column_name"),
                pg_type: r.get("pg_type"),
                fb_type: r.get("fb_type"),
                computed_expr: r.get("computed_expr"),
                file_visibility: r.get("file_visibility"),
            })
            .collect())
    }

    /// Apply post-query transformations to a JSON array of rows:
    ///
    /// 1. For `fb_type = "file"` columns: replace the stored S3 key with a
    ///    presigned GET URL (private) or the public CDN URL (public).
    /// 2. Null/missing file columns are left as-is.
    ///
    /// Computed columns are handled at compile time (injected into SELECT SQL),
    /// so this function does not need to evaluate expressions.
    pub async fn apply(
        rows: serde_json::Value,
        cols: &[ColumnMeta],
        file_engine: Option<&FileEngine>,
        _auth: &AuthContext,
    ) -> Result<serde_json::Value, EngineError> {
        // Fast path: no file columns or no file engine configured.
        let file_cols: Vec<&ColumnMeta> = cols.iter().filter(|c| c.fb_type == "file").collect();
        if file_cols.is_empty() || file_engine.is_none() {
            return Ok(rows);
        }

        let engine = file_engine.unwrap();

        let arr = match rows.as_array() {
            Some(a) => a,
            None => return Ok(rows),
        };

        let mut result = Vec::with_capacity(arr.len());
        for row in arr {
            let mut obj = match row.as_object().cloned() {
                Some(m) => m,
                None => {
                    result.push(row.clone());
                    continue;
                }
            };

            for col in &file_cols {
                if let Some(key_val) = obj.get(&col.name) {
                    if let Some(key) = key_val.as_str() {
                        let visibility = col
                            .file_visibility
                            .as_deref()
                            .unwrap_or("private");

                        let url = if visibility == "public" {
                            // Public files — return a plain URL without signing.
                            // In production the bucket has a CDN in front of it.
                            format!("https://cdn.placeholder/{}", key)
                        } else {
                            // Private files — generate a short-lived presigned URL.
                            match engine.download_url(key, None).await {
                                Ok(u) => u,
                                Err(e) => {
                                    tracing::warn!(
                                        column = %col.name,
                                        key = %key,
                                        error = %e,
                                        "failed to generate presigned URL (returning key)"
                                    );
                                    key.to_string()
                                }
                            }
                        };

                        obj.insert(col.name.clone(), serde_json::Value::String(url));
                    }
                }
            }

            result.push(serde_json::Value::Object(obj));
        }

        Ok(serde_json::Value::Array(result))
    }
}
