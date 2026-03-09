//! Batched nested-query executor.
//!
//! Executes a [`BatchedPlan`] by issuing one `IN (…)` SQL query per nesting
//! level and assembling the result tree in Rust.
//!
//! ## Complexity
//!
//! | Approach | Queries | Scans |
//! |---|---|---|
//! | Lateral (depth 1) | 1 | N_parents (nested loop) |
//! | CTE aggregation (depth 2-3) | 1 | O(1) per level |
//! | **Batched** (depth ≥ 4) | **1 per level** | **O(1) per level** |
//!
//! The batched approach produces more round-trips than the CTE plan, but each
//! individual query is trivially planned by PostgreSQL (`index seek + sort`),
//! whereas a CTE tree of depth ≥ 4 triggers complex planning heuristics.
//! At depth ≥ 4 batched is therefore faster end-to-end.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;

use serde_json::{json, Value};
use sqlx::PgPool;

use crate::compiler::relational::BatchStage;
use crate::engine::error::EngineError;
use crate::router::db_router::quote_ident;

use super::db_executor::bind_value;

// ─── Public API ───────────────────────────────────────────────────────────────

/// Attach all `stages` to `parent_rows` in place.
///
/// Expressed as a plain `fn` returning a boxed future so that Rust can handle
/// the indirect recursion (`execute_batched` → `attach_stage` →
/// `execute_batched`).  Callers use it exactly like an `async fn`:
///
/// ```ignore
/// executor::execute_batched(pool, &mut rows, &plan.stages).await?;
/// ```
pub fn execute_batched<'a>(
    pool: &'a PgPool,
    parent_rows: &'a mut Vec<Value>,
    stages: &'a [BatchStage],
) -> Pin<Box<dyn Future<Output = Result<(), EngineError>> + Send + 'a>> {
    Box::pin(async move {
        for stage in stages {
            attach_stage(pool, parent_rows, stage).await?;
        }
        Ok(())
    })
}

// ─── Per-stage attachment ────────────────────────────────────────────────────

fn attach_stage<'a>(
    pool: &'a PgPool,
    parent_rows: &'a mut Vec<Value>,
    stage: &'a BatchStage,
) -> Pin<Box<dyn Future<Output = Result<(), EngineError>> + Send + 'a>> {
    Box::pin(async move {
    // ── Collect unique parent-side key values ──────────────────────────────
    let unique_parent_ids: Vec<Value> = {
        let mut seen = HashSet::new();
        parent_rows
            .iter()
            .filter_map(|row| row.get(&stage.parent_col).cloned())
            .filter(|v| seen.insert(value_key(v)))
            .collect()
    };

    if unique_parent_ids.is_empty() {
        fill_empty(parent_rows, stage);
        return Ok(());
    }

    // ── Build the child SELECT ─────────────────────────────────────────────
    // Always include the FK column so we can group results back to parents.
    let col_list = if stage.cols.is_empty() {
        "*".to_owned()
    } else {
        let mut cols = stage.cols.clone();
        if !cols.iter().any(|c| c == &stage.fk_col) {
            cols.insert(0, stage.fk_col.clone());
        }
        cols.iter().map(|c| quote_ident(c)).collect::<Vec<_>>().join(", ")
    };

    // `$1, $2, … $N`  — one placeholder per unique parent ID.
    let placeholders: Vec<String> =
        (1..=unique_parent_ids.len()).map(|i| format!("${i}")).collect();

    let inner_sql = format!(
        "SELECT {cols} FROM {schema}.{table} \
         WHERE {fk} IN ({phs}) \
         ORDER BY {fk}",
        cols = col_list,
        schema = quote_ident(&stage.schema),
        table = quote_ident(&stage.table),
        fk = quote_ident(&stage.fk_col),
        phs = placeholders.join(", "),
    );

    // Wrap in json_agg so the result comes back as a single JSON-array row.
    let outer_sql = format!(
        r#"SELECT COALESCE(json_agg(row_to_json("_r")), '[]'::json) FROM ({inner_sql}) AS "_r""#
    );

    // ── Execute ────────────────────────────────────────────────────────────
    use sqlx::postgres::PgArguments;
    use sqlx::Row;

    let mut args = PgArguments::default();
    for id in &unique_parent_ids {
        bind_value(&mut args, id).map_err(EngineError::Internal)?;
    }

    let row = sqlx::query_with(&outer_sql, args)
        .fetch_one(pool)
        .await
        .map_err(EngineError::Db)?;

    let child_json: Value = row.get(0);
    let mut child_rows: Vec<Value> = serde_json::from_value(child_json).unwrap_or_default();

    // ── Recurse: attach grandchildren before grouping ──────────────────────
    if !stage.children.is_empty() {
        execute_batched(pool, &mut child_rows, &stage.children).await?;
    }

    // ── Group child rows by their FK value ─────────────────────────────────
    let mut by_parent: HashMap<String, Vec<Value>> = HashMap::new();
    for child in child_rows {
        let key = child
            .get(&stage.fk_col)
            .map(value_key)
            .unwrap_or_default();
        by_parent.entry(key).or_default().push(child);
    }

    // ── Attach to parent rows ──────────────────────────────────────────────
    for row in parent_rows.iter_mut() {
        let Value::Object(map) = row else { continue };
        let parent_key = map
            .get(&stage.parent_col)
            .map(value_key)
            .unwrap_or_default();
        let children = by_parent.remove(&parent_key).unwrap_or_default();
        if stage.is_array {
            map.insert(stage.alias.clone(), json!(children));
        } else {
            map.insert(
                stage.alias.clone(),
                children.into_iter().next().unwrap_or(Value::Null),
            );
        }
    }

    Ok(())
    })
}

/// Fill every parent row with an empty default for stages that had no parent IDs.
fn fill_empty(parent_rows: &mut Vec<Value>, stage: &BatchStage) {
    for row in parent_rows.iter_mut() {
        if let Value::Object(map) = row {
            map.insert(
                stage.alias.clone(),
                if stage.is_array { json!([]) } else { Value::Null },
            );
        }
    }
}

/// Produce a stable string key for grouping JSON values.
/// Strips surrounding quotes from JSON strings so `"1"` and `1` compare equal.
fn value_key(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
