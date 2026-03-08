use anyhow::anyhow;
use sqlx::postgres::PgArguments;
use sqlx::{Arguments, PgPool, Row};
use crate::compiler::CompiledQuery;
use crate::engine::error::EngineError;

/// Execute a `CompiledQuery` inside an explicit transaction and return the
/// results as a JSON array.
///
/// All operations (SELECT / INSERT RETURNING / UPDATE RETURNING / DELETE RETURNING)
/// are wrapped in a CTE so the output is always a uniform JSON array:
///
///   `[{ "col": val, ... }, ...]`
///
/// Running even SELECT inside a transaction ensures repeatable reads and provides
/// a clean hook for future before/after trigger logic.
pub async fn execute(pool: &PgPool, query: &CompiledQuery) -> Result<serde_json::Value, EngineError> {
    let mut tx = pool.begin().await.map_err(EngineError::Db)?;

    let mut args = PgArguments::default();
    for param in &query.params {
        bind_value(&mut args, param).map_err(EngineError::Internal)?;
    }

    // Wrap the inner SQL so we always get a JSON array back via json_agg.
    let outer = format!(
        r#"SELECT COALESCE(json_agg(row_to_json("_r")), '[]'::json) FROM ({}) AS "_r""#,
        query.sql
    );

    let row = sqlx::query_with(&outer, args)
        .fetch_one(&mut *tx)
        .await
        .map_err(EngineError::Db)?;

    tx.commit().await.map_err(EngineError::Db)?;

    let result: serde_json::Value = row.get(0);
    Ok(result)
}

fn bind_value(args: &mut PgArguments, val: &serde_json::Value) -> Result<(), anyhow::Error> {
    match val {
        serde_json::Value::String(s) => {
            args.add(s.clone()).map_err(|e| anyhow!("{e}"))?;
        }
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                args.add(i).map_err(|e| anyhow!("{e}"))?;
            } else {
                args.add(n.as_f64().unwrap_or(0.0)).map_err(|e| anyhow!("{e}"))?;
            }
        }
        serde_json::Value::Bool(b) => {
            args.add(*b).map_err(|e| anyhow!("{e}"))?;
        }
        serde_json::Value::Null => {
            args.add(Option::<String>::None).map_err(|e| anyhow!("{e}"))?;
        }
        other => {
            // Complex types (arrays, objects) — encode as JSON text.
            args.add(other.to_string()).map_err(|e| anyhow!("{e}"))?;
        }
    }
    Ok(())
}
