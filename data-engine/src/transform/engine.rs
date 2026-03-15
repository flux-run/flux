use sqlx::{PgPool, Row};

use crate::engine::error::EngineError;

/// Metadata for a single column, loaded from `flux_internal.column_metadata`.
#[derive(Debug, Clone)]
pub struct ColumnMeta {
    pub name: String,
    /// Postgres base type.
    pub pg_type: String,
    /// Extended Flux type: "default" | "computed".
    pub fb_type: String,
    pub computed_expr: Option<String>,
}

pub struct TransformEngine;

impl TransformEngine {
    /// Load column metadata for a given table from the registry.
    ///
    /// Returns an empty vec when no metadata is registered (e.g. the table was
    /// created outside the Flux API and has no extended type information).
    pub async fn load_columns(
        pool: &PgPool,
        schema: &str,
        table: &str,
    ) -> Result<Vec<ColumnMeta>, EngineError> {
        let rows = sqlx::query(
            "SELECT column_name, pg_type, fb_type, computed_expr \
             FROM flux_internal.column_metadata \
             WHERE schema_name = $1 AND table_name = $2 \
             ORDER BY ordinal",
        )
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
            })
            .collect())
    }
}
