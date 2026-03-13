//! Database executor — wraps user queries in an explicit Postgres transaction.
//!
//! ## Transaction pipeline (4 ordered steps)
//!
//! Every call to [`execute`] runs the following steps inside a single transaction:
//!
//! ### Step 1 — `SET LOCAL search_path = "{schema}", public`
//!
//! Prevents unqualified table references from resolving to the wrong tenant's schema.
//! `SET LOCAL` means the override is scoped to the transaction and automatically reverts
//! on `COMMIT`/`ROLLBACK` — safe with connection pooling.
//!
//! ### Step 2 — `SET LOCAL statement_timeout = 'Nms'`
//!
//! Postgres cancels the query at the DB engine level (SQLSTATE 57014) if it runs longer
//! than the configured budget. This fires even if the Rust `tokio::timeout` has already
//! dropped the future, protecting BYODB customer databases from runaway hash-joins and
//! sequential scans.
//!
//! ### Step 3 — UPDATE pre-read (`SELECT … FOR UPDATE`)
//!
//! For UPDATE operations only: a `SELECT * FROM schema.table WHERE {same conditions} FOR UPDATE`
//! is executed before the mutation to capture `before_state`. The `FOR UPDATE` also serialises
//! concurrent writes to the same rows, eliminating lost-update races in the mutation log.
//!
//! ### Step 4 — User query + `state_mutations` write
//!
//! The user query is wrapped in `json_agg(row_to_json(_r))` so the executor always receives
//! a JSON array. For INSERT/UPDATE/DELETE, a `state_mutations` row is written for each
//! affected row inside the same transaction — the **atomic guarantee**: either the data change
//! and its mutation log entry both commit, or neither does.
//!
//! ## Trace comment
//!
//! Every SQL statement is prefixed with `/* flux_req:{request_id},span:{span_id} */`.
//! This makes the query identifiable in `pg_stat_activity`, `auto_explain`, and `pgaudit`
//! without any extra configuration.
use anyhow::anyhow;
use sqlx::postgres::PgArguments;
use sqlx::{Arguments, PgPool, Row};
use crate::compiler::CompiledQuery;
use crate::engine::error::EngineError;

/// Map a `sqlx::Error` to `EngineError`, converting Postgres SQLSTATE `57014`
/// (query_canceled — fired when `statement_timeout` expires) into the typed
/// `EngineError::QueryTimeout` so the API layer returns HTTP 408 rather than 500.
fn map_db_error(e: sqlx::Error) -> EngineError {
    if let sqlx::Error::Database(ref db_err) = e {
        if db_err.code().as_deref() == Some("57014") {
            return EngineError::QueryTimeout;
        }
    }
    EngineError::Db(e)
}

/// Per-request context passed to `execute` so the executor can:
///   • enforce a transaction-scoped `search_path`  (Gap 5)
///   • prepend a trace comment to every SQL statement (Gap 3)
///   • write a row to `fluxbase_internal.state_mutations` for mutations (Gap 1)
pub struct MutationContext<'a> {
    /// Postgres schema name, e.g. `main` — already validated by DbRouter.
    pub schema: &'a str,
    /// Forwarded from `x-request-id`; used in SQL comment + state_mutations.request_id.
    pub request_id: &'a str,
    /// Forwarded from `x-span-id`; links each mutation to the span that caused it.
    /// Enables intra-request time-travel: reconstruct state at any point in execution.
    pub span_id: Option<&'a str>,
    /// User-facing table name (not schema-qualified).
    pub table: &'a str,
    /// "select" | "insert" | "update" | "delete"
    pub operation: &'a str,
    /// Authenticated user performing the operation; stored as actor_id.
    pub user_id: &'a str,
    /// Postgres-level statement timeout for this request in milliseconds.
    /// Injected as `SET LOCAL statement_timeout = 'Nms'` inside the transaction
    /// so Postgres cancels the query if it exceeds the budget, even if the
    /// outer tokio::timeout has already dropped the Rust future.
    pub statement_timeout_ms: u64,
}

/// Execute a `CompiledQuery` inside an explicit transaction and return the
/// results as a JSON array.
///
/// The execution pipeline inside the transaction (in order):
///
///   1. `SET LOCAL search_path = "{schema}", public`  — prevents cross-tenant leaks
///   2. Prepend trace comment to SQL                  — correlates DB query → request
///   3. Execute the user query via json_agg wrapper   — uniform `[{…}, …]` result
///   4. Write to state_mutations for INSERT/UPDATE/DELETE — deterministic replay log
///
/// All four steps share the same transaction so the mutation log is
/// atomically consistent with the user write: either both commit or both roll back.
pub async fn execute(
    pool: &PgPool,
    query: &CompiledQuery,
    ctx: &MutationContext<'_>,
) -> Result<serde_json::Value, EngineError> {
    let mut tx = pool.begin().await.map_err(EngineError::Db)?;

    // ── Gap 5: transaction-scoped search_path ─────────────────────────────
    // Ensures unqualified table references in policies / computed expressions
    // always resolve to this tenant's schema, never another.
    // schema is already validated through validate_identifier in DbRouter.
    sqlx::query(&format!(
        r#"SET LOCAL search_path = "{}", public"#,
        ctx.schema.replace('"', ""),
    ))
    .execute(&mut *tx)
    .await
    .map_err(EngineError::Db)?;

    // ── Gap 19: Postgres-level statement timeout (BYODB protection) ───────
    // SET LOCAL means the timeout is scoped to this transaction and resets
    // automatically on COMMIT/ROLLBACK — safe with connection pooling.
    // This causes Postgres to cancel the query inside the DB engine itself
    // (SQLSTATE 57014), protecting BYODB customer databases from runaway
    // hash-joins and sequential scans even if the Rust timeout fires first.
    sqlx::query(&format!("SET LOCAL statement_timeout = '{}ms'", ctx.statement_timeout_ms))
        .execute(&mut *tx)
        .await
        .map_err(EngineError::Db)?;

    // ── Gap 14 v2: UPDATE pre-read (before-state capture) ────────────────
    // For UPDATE we SELECT the matching rows FOR UPDATE *before* running the
    // mutation so we can store before_state in state_mutations and compute
    // changed_fields.  The pre-read uses its own param list (fresh $N indices
    // with no SET params) that was built alongside the UPDATE SQL in the
    // compiler.  FOR UPDATE in the pre-read also serialises concurrent writes
    // to the same rows, eliminating lost-update races in the mutation log.
    let pre_read_map: std::collections::HashMap<String, serde_json::Value> =
        if ctx.operation == "update" {
            if let Some(ref pre_sql) = query.pre_read_sql {
                let mut pre_args = PgArguments::default();
                for param in &query.pre_read_params {
                    bind_value(&mut pre_args, param).map_err(EngineError::Internal)?;
                }
                let traced_pre = format!(
                    "/* flux_req:{req},span:{span} */ {sql}",
                    req    = ctx.request_id,
                    span   = ctx.span_id.unwrap_or("-"),
                    sql    = pre_sql,
                );
                let outer_pre = format!(
                    r#"SELECT COALESCE(json_agg(row_to_json("_r")), '[]'::json) FROM ({}) AS "_r""#,
                    traced_pre
                );
                let pre_row = sqlx::query_with(&outer_pre, pre_args)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(map_db_error)?;
                let pre_result: serde_json::Value = pre_row.get(0);
                let mut map = std::collections::HashMap::new();
                if let Some(rows) = pre_result.as_array() {
                    for row in rows {
                        let pk_key = if row.get("id").is_some() {
                            serde_json::json!({ "id": row["id"] })
                        } else {
                            row.clone()
                        };
                        map.insert(pk_key.to_string(), row.clone());
                    }
                }
                map
            } else {
                std::collections::HashMap::new()
            }
        } else {
            std::collections::HashMap::new()
        };

    let mut args = PgArguments::default();
    for param in &query.params {
        bind_value(&mut args, param).map_err(EngineError::Internal)?;
    }

    // ── Gap 3: SQL trace comment ──────────────────────────────────────────
    // Prepend a comment so the query appears in pg_stat_activity,
    // auto_explain, and pgaudit with full request + span context.
    // span: allows pg_stat_activity to show *which runtime span* generated
    // each in-flight query — critical for flux trace debug step-through mode.
    let traced_sql = format!(
        "/* flux_req:{req},span:{span} */ {sql}",
        req  = ctx.request_id,
        span = ctx.span_id.unwrap_or("-"),
        sql  = query.sql,
    );

    // Wrap the inner SQL so we always get a JSON array back via json_agg.
    let outer = format!(
        r#"SELECT COALESCE(json_agg(row_to_json("_r")), '[]'::json) FROM ({}) AS "_r""#,
        traced_sql
    );

    let row = sqlx::query_with(&outer, args)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_db_error)?;

    let result: serde_json::Value = row.get(0);

    // ── Gap 1: state_mutations write ──────────────────────────────────────
    // Fire for INSERT / UPDATE / DELETE only — SELECTs are never logged.
    // All writes use the same transaction so the mutation log is atomic
    // with respect to the user write.
    if ctx.operation != "select" {
        if let Some(rows) = result.as_array() {
            for affected_row in rows {
                // Extract record_pk. Prefer {"id": <value>} when the row has an
                // "id" column; otherwise store the full row as the identity.
                let record_pk: serde_json::Value = if affected_row.get("id").is_some() {
                    serde_json::json!({ "id": affected_row["id"] })
                } else {
                    affected_row.clone()
                };

                // Assign before/after based on operation semantics:
                //   INSERT → no prior state    (before = NULL)
                //   UPDATE → before = pre-read snapshot; after = new state (Gap 14 v2)
                //   DELETE → row is gone       (after  = NULL)
                let (before_state, after_state): (Option<serde_json::Value>, Option<serde_json::Value>) =
                    match ctx.operation {
                        "insert" => (None, Some(affected_row.clone())),
                        "update" => {
                            // Look up the row we captured in the pre-read by its pk key.
                            let before = pre_read_map.get(&record_pk.to_string()).cloned();
                            (before, Some(affected_row.clone()))
                        }
                        "delete" => (Some(affected_row.clone()), None),
                        _ => continue,
                    };

                // Monotonically increasing version per (tenant, project, table, record_pk).
                // FOR UPDATE serialises concurrent increments on the same record.
                let version: i64 = sqlx::query_scalar(
                    r#"
                    SELECT COALESCE(MAX(version), 0) + 1
                    FROM   fluxbase_internal.state_mutations
                    WHERE  table_name = $1
                      AND  record_pk  = $2
                    FOR UPDATE
                    "#,
                )
                .bind(ctx.table)
                .bind(&record_pk)
                .fetch_one(&mut *tx)
                .await
                .map_err(EngineError::Db)?;

                // changed_fields: for UPDATE, compare before/after JSONB key-by-key
                // and store only the names of columns whose values changed.
                // Sorted for deterministic output — enables efficient CLI diffing
                // without a full JSON comparison at read time.
                // NULL for INSERT (no prior state) and DELETE (no new state).
                let changed_fields: Option<Vec<String>> = if ctx.operation == "update" {
                    before_state.as_ref().and_then(|before| {
                        after_state.as_ref().map(|after| {
                            let keys: std::collections::HashSet<String> = before
                                .as_object()
                                .into_iter()
                                .flat_map(|o| o.keys().cloned())
                                .chain(
                                    after
                                        .as_object()
                                        .into_iter()
                                        .flat_map(|o| o.keys().cloned()),
                                )
                                .collect();
                            let mut fields: Vec<String> = keys
                                .into_iter()
                                .filter(|k| before.get(k) != after.get(k))
                                .collect();
                            fields.sort();
                            fields
                        })
                    })
                } else {
                    None
                };

                sqlx::query(
                    r#"
                    INSERT INTO fluxbase_internal.state_mutations
                        (schema_name, table_name, record_pk,
                         operation, before_state, after_state, changed_fields,
                         version, actor_id, request_id, span_id)
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                    "#,
                )
                .bind(ctx.schema)
                .bind(ctx.table)
                .bind(&record_pk)
                .bind(ctx.operation)
                .bind(&before_state)
                .bind(&after_state)
                .bind(changed_fields)
                .bind(version)
                .bind(ctx.user_id)
                .bind(ctx.request_id)
                .bind(ctx.span_id)
                .execute(&mut *tx)
                .await
                .map_err(EngineError::Db)?;
            }
        }
    }

    tx.commit().await.map_err(EngineError::Db)?;
    Ok(result)
}

pub(crate) fn bind_value(args: &mut PgArguments, val: &serde_json::Value) -> Result<(), anyhow::Error> {
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
