//! In-process implementation of [`DataEngineDispatch`].
//!
//! Executes raw SQL directly against the project database pool — no HTTP.
//! Replicates the essential logic of `data-engine/src/api/handlers/sql.rs`
//! (search_path isolation, statement timeout, param binding, json_agg wrapping).

use async_trait::async_trait;
use serde_json::Value;
use sqlx::postgres::PgArguments;
use sqlx::{Arguments, PgPool, Row};
use std::time::Instant;

use job_contract::dispatch::DataEngineDispatch;

/// Executes SQL against the data-engine's pool in-process — used by the
/// monolithic server so V8 `ctx.db.query()` never makes an HTTP call.
pub struct InProcessDataEngineDispatch {
    pub pool:                 PgPool,
    pub statement_timeout_ms: u64,
}

#[async_trait]
impl DataEngineDispatch for InProcessDataEngineDispatch {
    async fn execute_sql(
        &self,
        sql:        String,
        params:     Vec<Value>,
        database:   String,
        request_id: String,
    ) -> Result<Value, String> {
        // Validate schema/database name to prevent SQL injection via schema name.
        validate_identifier(&database)?;

        let start = Instant::now();
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;

        // Scope search_path to the tenant's database/schema
        sqlx::query(&format!(
            r#"SET LOCAL search_path = "{}", public"#,
            database.replace('"', ""),
        ))
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("set search_path failed: {}", e))?;

        // Apply statement timeout
        sqlx::query(&format!(
            "SET LOCAL statement_timeout = '{}ms'",
            self.statement_timeout_ms
        ))
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("set timeout failed: {}", e))?;

        // Bind params
        let mut args = PgArguments::default();
        for param in &params {
            bind_value(&mut args, param)?;
        }

        let sql_upper = sql.trim().to_uppercase();
        let (rows, affected) = if sql_upper.starts_with("SELECT") || sql_upper.starts_with("WITH") {
            // Wrap in json_agg for uniform array-of-objects output
            let wrapped = format!(
                r#"SELECT COALESCE(json_agg(t), '[]'::json) AS rows FROM ({}) t"#,
                sql.trim_end_matches(';')
            );

            let row = sqlx::query_with(&wrapped, args)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| format!("db query failed: {}", e))?;

            let rows_json: Value = row.try_get("rows")
                .map_err(|e| format!("db result parse failed: {}", e))?;
            let rows: Vec<Value> = rows_json
                .as_array()
                .cloned()
                .unwrap_or_default();
            (rows, 0u64)
        } else {
            // Non-SELECT: execute directly and return affected row count
            let result = sqlx::query_with(&sql, args)
                .execute(&mut *tx)
                .await
                .map_err(|e| format!("db query failed: {}", e))?;
            (vec![], result.rows_affected())
        };

        tx.commit().await.map_err(|e| format!("commit failed: {}", e))?;

        let row_count = if rows.is_empty() { affected as usize } else { rows.len() };
        let elapsed_ms = start.elapsed().as_millis() as u64;

        Ok(serde_json::json!({
            "data": rows,
            "meta": {
                "rows":       row_count,
                "elapsed_ms": elapsed_ms,
                "request_id": request_id,
            }
        }))
    }
}

/// Validate a PostgreSQL identifier: `[a-zA-Z_][a-zA-Z0-9_]*`, max 63 chars.
fn validate_identifier(s: &str) -> Result<(), String> {
    if s.is_empty()
        || s.len() > 63
        || !s.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false)
        || !s.chars().all(|c| c.is_alphanumeric() || c == '_')
    {
        return Err(format!("invalid identifier: '{}'", s));
    }
    Ok(())
}

/// Bind a JSON value to PgArguments.
fn bind_value(args: &mut PgArguments, val: &Value) -> Result<(), String> {
    match val {
        Value::String(s) => {
            args.add(s.clone()).map_err(|e| e.to_string())?;
        }
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                args.add(i).map_err(|e| e.to_string())?;
            } else {
                args.add(n.as_f64().unwrap_or(0.0)).map_err(|e| e.to_string())?;
            }
        }
        Value::Bool(b) => {
            args.add(*b).map_err(|e| e.to_string())?;
        }
        Value::Null => {
            args.add(Option::<String>::None).map_err(|e| e.to_string())?;
        }
        other => {
            // Complex types (arrays, objects) — encode as JSON text.
            args.add(other.to_string()).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
