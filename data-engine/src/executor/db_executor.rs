use anyhow::anyhow;
use sqlx::postgres::PgArguments;
use sqlx::{Arguments, PgPool, Row};
use uuid::Uuid;
use crate::compiler::CompiledQuery;
use crate::engine::error::EngineError;

/// Per-request context passed to `execute` so the executor can:
///   • enforce a transaction-scoped `search_path`  (Gap 5)
///   • prepend a trace comment to every SQL statement (Gap 3)
///   • write a row to `fluxbase_internal.state_mutations` for mutations (Gap 1)
pub struct MutationContext<'a> {
    /// Postgres schema name, e.g. `t_acme_auth_main` — already validated by DbRouter.
    pub schema: &'a str,
    /// Forwarded from `x-request-id`; used in SQL comment + state_mutations.request_id.
    pub request_id: &'a str,
    /// Forwarded from `x-span-id`; links each mutation to the span that caused it.
    /// Enables intra-request time-travel: reconstruct state at any point in execution.
    pub span_id: Option<&'a str>,
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    /// User-facing table name (not schema-qualified).
    pub table: &'a str,
    /// "select" | "insert" | "update" | "delete"
    pub operation: &'a str,
    /// Authenticated user performing the operation; stored as actor_id.
    pub user_id: &'a str,
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

    let mut args = PgArguments::default();
    for param in &query.params {
        bind_value(&mut args, param).map_err(EngineError::Internal)?;
    }

    // ── Gap 3: SQL trace comment ──────────────────────────────────────────
    // Prepend a comment to the SQL so the query appears in pg_stat_activity
    // and any DB-level logging tool with full request context.
    let traced_sql = format!(
        "/* flux_req:{req},tenant:{tenant} */ {sql}",
        req    = ctx.request_id,
        tenant = ctx.tenant_id,
        sql    = query.sql,
    );

    // Wrap the inner SQL so we always get a JSON array back via json_agg.
    let outer = format!(
        r#"SELECT COALESCE(json_agg(row_to_json("_r")), '[]'::json) FROM ({}) AS "_r""#,
        traced_sql
    );

    let row = sqlx::query_with(&outer, args)
        .fetch_one(&mut *tx)
        .await
        .map_err(EngineError::Db)?;

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
                //   UPDATE → after = new state (before = NULL in v1; pre-read added in v2)
                //   DELETE → row is gone       (after  = NULL)
                let (before_state, after_state): (Option<serde_json::Value>, Option<serde_json::Value>) =
                    match ctx.operation {
                        "insert" => (None, Some(affected_row.clone())),
                        "update" => (None, Some(affected_row.clone())),
                        "delete" => (Some(affected_row.clone()), None),
                        _ => continue,
                    };

                // Monotonically increasing version per (tenant, project, table, record_pk).
                // FOR UPDATE serialises concurrent increments on the same record.
                let version: i64 = sqlx::query_scalar(
                    r#"
                    SELECT COALESCE(MAX(version), 0) + 1
                    FROM   fluxbase_internal.state_mutations
                    WHERE  tenant_id  = $1
                      AND  project_id = $2
                      AND  table_name = $3
                      AND  record_pk  = $4
                    FOR UPDATE
                    "#,
                )
                .bind(ctx.tenant_id)
                .bind(ctx.project_id)
                .bind(ctx.table)
                .bind(&record_pk)
                .fetch_one(&mut *tx)
                .await
                .map_err(EngineError::Db)?;

                sqlx::query(
                    r#"
                    INSERT INTO fluxbase_internal.state_mutations
                        (tenant_id, project_id, table_name, record_pk,
                         operation, before_state, after_state,
                         version, actor_id, request_id, span_id)
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                    "#,
                )
                .bind(ctx.tenant_id)
                .bind(ctx.project_id)
                .bind(ctx.table)
                .bind(&record_pk)
                .bind(ctx.operation)
                .bind(&before_state)
                .bind(&after_state)
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
