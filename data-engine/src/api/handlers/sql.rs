use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::postgres::PgArguments;
use std::sync::Arc;
use std::time::Instant;

use crate::{engine::error::EngineError, state::AppState};

#[derive(Debug, Deserialize)]
pub struct RawSqlRequest {
    pub sql:        String,
    #[serde(default)]
    pub params:     Vec<serde_json::Value>,
    pub database:   String,
    #[serde(default)]
    pub request_id: String,
}

#[derive(Debug, Serialize)]
pub struct RawSqlResponse {
    pub data: Vec<serde_json::Value>,
    pub meta: RawSqlMeta,
}

#[derive(Debug, Serialize)]
pub struct RawSqlMeta {
    pub rows:       usize,
    pub elapsed_ms: u64,
    pub request_id: String,
}

pub async fn handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RawSqlRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    // Validate schema/database name (prevent SQL injection via schema name)
    crate::router::db_router::validate_identifier(&req.database)?;

    let start = Instant::now();
    let mut tx = state.pool.begin().await.map_err(EngineError::Db)?;

    // Scope search_path to the tenant's database/schema
    sqlx::query(&format!(
        r#"SET LOCAL search_path = "{}", public"#,
        req.database.replace('"', ""),
    ))
    .execute(&mut *tx)
    .await
    .map_err(EngineError::Db)?;

    // Apply statement timeout
    sqlx::query(&format!(
        "SET LOCAL statement_timeout = '{}ms'",
        state.statement_timeout_ms
    ))
    .execute(&mut *tx)
    .await
    .map_err(EngineError::Db)?;

    // Bind params using the same helper as db_executor
    let mut args = PgArguments::default();
    for param in &req.params {
        crate::executor::db_executor::bind_value(&mut args, param)
            .map_err(EngineError::Internal)?;
    }

    let sql_upper = req.sql.trim().to_uppercase();
    let (rows, affected) = if sql_upper.starts_with("SELECT") || sql_upper.starts_with("WITH") {
        // Wrap in json_agg for uniform array-of-objects output
        let wrapped = format!(
            r#"SELECT COALESCE(json_agg(t), '[]'::json) AS rows FROM ({}) t"#,
            req.sql.trim_end_matches(';')
        );

        let row = sqlx::query_with(&wrapped, args)
            .fetch_one(&mut *tx)
            .await
            .map_err(EngineError::Db)?;

        use sqlx::Row;
        let rows_json: serde_json::Value = row.try_get("rows").map_err(EngineError::Db)?;
        let rows: Vec<serde_json::Value> = rows_json
            .as_array()
            .cloned()
            .unwrap_or_default();
        (rows, 0u64)
    } else {
        // Non-SELECT: execute directly and return affected row count
        let result = sqlx::query_with(&req.sql, args)
            .execute(&mut *tx)
            .await
            .map_err(EngineError::Db)?;
        (vec![], result.rows_affected())
    };

    tx.commit().await.map_err(EngineError::Db)?;

    let row_count = if rows.is_empty() { affected as usize } else { rows.len() };
    let elapsed_ms = start.elapsed().as_millis() as u64;

    Ok(Json(json!({
        "data": rows,
        "meta": {
            "rows":       row_count,
            "elapsed_ms": elapsed_ms,
            "request_id": req.request_id,
        }
    })))
}
