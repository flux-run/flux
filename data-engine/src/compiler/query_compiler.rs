//! Query compiler — translates the JSON query API into parameterised SQL.
//!
//! ## Compilation pipeline
//!
//! ```text
//! QueryRequest (JSON)
//!        ↓
//! PolicyEngine::evaluate_cached()   → allowed_columns, row_condition_sql
//!        ↓
//! SchemaCache lookup                → column metadata, relationships, computed cols
//!        ↓
//! Column allowlist                  → filter requested cols to policy-allowed set
//!        ↓
//! Filter → SQL WHERE clause         → each Filter { column, op, value } → "$N" param
//!        ↓
//! pre_read_sql generation (UPDATE)  → SELECT * … WHERE … FOR UPDATE (fresh $N indices)
//!        ↓
//! CompileResult::Single(CompiledQuery)
//!   — OR —
//! CompileResult::Batched { root, plan }  (when nesting depth ≥ BATCH_DEPTH_THRESHOLD)
//! ```
//!
//! ## `pre_read_sql` (UPDATE)
//!
//! For UPDATE queries, the compiler emits a second SQL string (`pre_read_sql`) with a
//! fresh `$1 … $N` parameter list (no SET clause parameters — only WHERE parameters).
//! The executor runs this SELECT before the UPDATE inside the same transaction so
//! `state_mutations.before_state` is always a row snapshot from immediately before the
//! write, preventing the phantom-read problem.
//!
//! ## Nested / batched queries
//!
//! When the caller requests deeply nested relationships (depth ≥ `BATCH_DEPTH_THRESHOLD`),
//! a single CTE would produce a cartesian explosion. The compiler instead emits a
//! `CompileResult::Batched` plan describing each child level. The executor fetches levels
//! independently and joins in Rust — trading SQL complexity for Rust memory.
use serde::{Deserialize, Serialize};
use crate::engine::error::EngineError;
use crate::policy::PolicyResult;
use crate::router::db_router::{validate_identifier, quote_ident};
use crate::compiler::relational::{
    parse_selectors, expand_nested_deep, build_nested_ctes,
    selector_depth, build_batched_plan, BatchedPlan, BATCH_DEPTH_THRESHOLD,
    ColumnSelector, RelationshipDef,
};

// ─── Compile result ────────────────────────────────────────────────────────────

/// The output of [`QueryCompiler::compile`].
///
/// For non-SELECT operations and shallow nested queries the result is a single
/// SQL template ready for direct execution ([`CompileResult::Single`]).
///
/// For SELECT queries whose nested-selector depth is ≥
/// [`BATCH_DEPTH_THRESHOLD`], the result is a flat root query plus a
/// [`BatchedPlan`] that the executor uses to fetch each child level
/// independently ([`CompileResult::Batched`]).
pub enum CompileResult {
    /// A single compiled SQL statement — the normal path.
    Single(CompiledQuery),
    /// Root flat SELECT + a per-level child fetch plan (deep nesting path).
    Batched {
        /// The root SELECT (flat columns only, no nested expansion).
        root: CompiledQuery,
        /// Plan describing each child level to fetch and join in Rust.
        plan: BatchedPlan,
    },
}

// ─── Public request / response types ─────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct QueryRequest {
    /// Name of the project database (maps to a Postgres schema).
    pub database: String,
    /// Target table name within that schema.
    pub table: String,
    /// "select" | "insert" | "update" | "delete"
    pub operation: String,
    /// Columns to return / write. None = policy decides.
    pub columns: Option<Vec<String>>,
    /// WHERE conditions for select / update / delete.
    pub filters: Option<Vec<Filter>>,
    /// Row data for insert / update (key-value JSON object).
    pub data: Option<serde_json::Value>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Filter {
    pub column: String,
    /// "eq" | "neq" | "gt" | "gte" | "lt" | "lte" | "like" | "ilike" | "is_null" | "not_null"
    pub op: String,
    #[serde(default)]
    pub value: serde_json::Value,
}

/// A fully compiled SQL statement ready for execution, plus ordered bind values.
///
/// `schema` identifies the PostgreSQL schema this query targets. It is set at
/// compile time from the authenticated request's `DbRouter::schema_name()` result
/// and used by `db_executor::execute()` to enforce `SET LOCAL search_path` for
/// the transaction, providing a database-level defense against cross-tenant access.
#[derive(Debug)]
pub struct CompiledQuery {
    pub sql: String,
    pub params: Vec<serde_json::Value>,
    /// The PostgreSQL schema this query was compiled to target.
    /// Must match the tenant+project the caller authenticated as.
    pub schema: String,
    /// UPDATE only: a `SELECT * FROM schema.table WHERE {same conditions} FOR UPDATE`
    /// statement with its own param list (fresh $N indices, no SET params).
    /// The executor runs this inside the transaction before the UPDATE to capture
    /// before_state and compute changed_fields.  None for SELECT/INSERT/DELETE.
    pub pre_read_sql: Option<String>,
    pub pre_read_params: Vec<serde_json::Value>,
}

/// A computed column to be injected into SELECT expressions.
#[derive(Debug, Clone)]
pub struct ComputedCol {
    /// Output column alias in the result set.
    pub name: String,
    /// Raw SQL expression, e.g. `"first_name || ' ' || last_name"`.
    pub expr: String,
}

/// Compilation-time options supplied by the caller (sourced from `AppState`).
#[derive(Debug, Clone, Default)]
pub struct CompilerOptions {
    /// Rows returned per query when the caller omits a LIMIT.
    pub default_limit: i64,
    /// Hard ceiling — clamped even if the caller supplies a larger LIMIT.
    pub max_limit: i64,
    /// Computed columns to inject into SELECT expressions as `expr AS "name"`.
    /// These are appended after the resolved column list.
    pub computed_cols: Vec<ComputedCol>,
    /// Relationships for this table — used to expand nested selectors like `posts(id,title)`.
    pub relationships: Vec<RelationshipDef>,
}

impl CompilerOptions {
    pub fn with_limits(default_limit: i64, max_limit: i64) -> Self {
        Self {
            default_limit,
            max_limit,
            computed_cols: vec![],
            relationships: vec![],
        }
    }
}

// ─── Compiler ────────────────────────────────────────────────────────────────

pub struct QueryCompiler;

impl QueryCompiler {
    /// Compile a `QueryRequest` into a `CompiledQuery` given the resolved
    /// `PolicyResult` and the target Postgres `schema` name.
    pub fn compile(
        req: &QueryRequest,
        policy: &PolicyResult,
        schema: &str,
        opts: &CompilerOptions,
    ) -> Result<CompileResult, EngineError> {
        validate_identifier(schema)?;
        validate_identifier(&req.table)?;

        // Split user-supplied columns into flat names and nested selectors.
        // Nested selectors (e.g. "posts(id,title)") must not pass through
        // validate_identifier, so we extract them before resolve_columns.
        let (flat_user_cols, nested_sels): (Vec<String>, Vec<ColumnSelector>) =
            if let Some(ref user_cols) = req.columns {
                let parsed = parse_selectors(user_cols);
                let mut flat = vec![];
                let mut nested: Vec<ColumnSelector> = vec![];
                for sel in parsed {
                    match sel {
                        ColumnSelector::Flat(c) => flat.push(c),
                        sel @ ColumnSelector::Nested { .. } => nested.push(sel),
                    }
                }
                (flat, nested)
            } else {
                (vec![], vec![])
            };

        // Columns to operate on: intersect flat request columns with policy-allowed columns.
        let flat_opt = if flat_user_cols.is_empty() && req.columns.is_some() {
            // User sent only nested selectors with no flat cols → select *
            None
        } else if flat_user_cols.is_empty() {
            None
        } else {
            Some(flat_user_cols.as_slice())
        };
        let cols = resolve_columns(flat_opt, &policy.allowed_columns)?;

        // Accumulated params — row_condition params come first so their $N indices
        // are stable; filter params are appended after.
        let mut params: Vec<serde_json::Value> = policy.row_condition_params.clone();
        let mut next_param = params.len() + 1; // 1-based

        match req.operation.as_str() {
            "select" => compile_select(req, schema, &cols, &nested_sels, policy, &mut params, &mut next_param, opts),
            "insert" => compile_insert(req, schema, &cols, &mut params, &mut next_param).map(CompileResult::Single),
            "update" => compile_update(req, schema, &cols, policy, &mut params, &mut next_param).map(CompileResult::Single),
            "delete" => compile_delete(req, schema, policy, &mut params, &mut next_param).map(CompileResult::Single),
            op => Err(EngineError::UnsupportedOperation(op.to_string())),
        }
    }
}

// --- Operation compilers ---

fn compile_select(
    req: &QueryRequest,
    schema: &str,
    cols: &[String],
    nested_sels: &[ColumnSelector],
    policy: &PolicyResult,
    params: &mut Vec<serde_json::Value>,
    next: &mut usize,
    opts: &CompilerOptions,
) -> Result<CompileResult, EngineError> {
    // Guard: OFFSET without an explicit LIMIT is ambiguous — the caller has no
    // stable contract for what rows they'll receive because the default limit
    // may change.  Require an explicit limit whenever an offset is supplied.
    if req.offset.is_some() && req.limit.is_none() {
        return Err(EngineError::MissingField(
            "limit is required when offset is specified".into(),
        ));
    }

    // ── Batched execution path (depth ≥ BATCH_DEPTH_THRESHOLD) ─────────────
    //
    // When the selector tree is deeper than the CTE threshold, PostgreSQL's
    // planner struggles with the large CTE graph.  Instead:
    //   1. Compile a flat root SELECT (no nested expansion in SQL).
    //   2. Return a BatchedPlan describing per-level child fetches.
    //   3. The executor runs each level as a standalone ANY($…) query and
    //      assembles the JSON tree in Rust.
    let max_depth = nested_sels.iter().map(selector_depth).max().unwrap_or(0);
    if max_depth >= BATCH_DEPTH_THRESHOLD {
        // Root SELECT: flat columns + the join-key column for each nested
        // selector's relationship (needed for Rust-side child attachment).
        let mut col_parts: Vec<String> = if cols.is_empty() {
            vec!["t.*".to_string()]
        } else {
            let mut c: Vec<String> =
                cols.iter().map(|c| format!("t.{}", quote_ident(c))).collect();
            for sel in nested_sels {
                if let ColumnSelector::Nested { alias, .. } = sel {
                    if let Some(rel) = opts.relationships.iter().find(|r| {
                        r.from_table == req.table && &r.alias == alias
                    }) {
                        let fc = format!("t.{}", quote_ident(&rel.from_column));
                        if !c.contains(&fc) {
                            c.push(fc);
                        }
                    }
                }
            }
            c
        };
        for cc in &opts.computed_cols {
            col_parts.push(format!("{} AS {}", cc.expr, quote_ident(&cc.name)));
        }
        let col_list = col_parts.join(", ");
        let mut sql = format!(
            "SELECT {} FROM {}.{} t",
            col_list, quote_ident(schema), quote_ident(&req.table),
        );
        let where_parts = build_where(policy, req.filters.as_deref(), params, next)?;
        if !where_parts.is_empty() {
            sql.push_str(&format!(" WHERE {}", where_parts.join(" AND ")));
        }
        let effective_limit = match req.limit {
            Some(l) => l.min(opts.max_limit).max(1),
            None    => opts.default_limit,
        };
        params.push(serde_json::Value::Number(effective_limit.into()));
        sql.push_str(&format!(" LIMIT ${}", *next));
        *next += 1;
        if let Some(offset) = req.offset {
            params.push(serde_json::Value::Number(offset.into()));
            sql.push_str(&format!(" OFFSET ${}", *next));
            *next += 1;
        }
        let batched_plan =
            build_batched_plan(schema, &req.table, nested_sels, &opts.relationships);
        return Ok(CompileResult::Batched {
            root: CompiledQuery { sql, params: params.clone(), schema: schema.to_string(), pre_read_sql: None, pre_read_params: vec![] },
            plan: batched_plan,
        });
    }

    // Base column list from policy + user request.
    let mut col_parts: Vec<String> = if cols.is_empty() {
        vec!["t.*".to_string()] // use alias so nested subqueries can reference outer cols by alias
    } else {
        cols.iter().map(|c| format!("t.{}", quote_ident(c))).collect()
    };

    // Inject computed columns — appended so they don't disturb * semantics.
    for cc in &opts.computed_cols {
        col_parts.push(format!("{} AS {}", cc.expr, quote_ident(&cc.name)));
    }

    // Expand nested selectors.
    //
    // Strategy: use the CTE aggregation plan (each related table scanned once,
    // hash-joined) rather than per-row correlated subqueries.  For deep
    // queries like `users → posts → comments → likes` this is 3–5× faster.
    //
    // Fall back to `expand_nested_deep` only for lone depth-1 relationships
    // that have no nested children — lateral is equally fast there and avoids
    // the WITH overhead for the simplest case.
    let only_shallow = nested_sels.iter().all(|sel| {
        if let ColumnSelector::Nested { cols, .. } = sel {
            !cols.iter().any(|c| matches!(c, ColumnSelector::Nested { .. }))
        } else {
            true
        }
    });

    let cte_plan = if !nested_sels.is_empty() && !only_shallow {
        // Deep nested: use CTE aggregation plan.
        let plan = build_nested_ctes(schema, "t", &req.table, nested_sels, &opts.relationships);
        // Register CTE select expressions now; JOIN frags used when building FROM.
        col_parts.extend(plan.select_exprs.clone());
        Some(plan)
    } else {
        // Shallow (depth 1): use correlated lateral — no CTE overhead.
        for sel in nested_sels {
            if let ColumnSelector::Nested { alias, cols: inner_sels } = sel {
                if let Some(rel) = opts.relationships.iter().find(|r| {
                    r.from_table == req.table && &r.alias == alias
                }) {
                    col_parts.push(expand_nested_deep(
                        schema,
                        "t",
                        rel,
                        inner_sels,
                        &opts.relationships,
                    ));
                } else {
                    tracing::warn!(
                        alias = %alias,
                        table = %req.table,
                        "nested selector has no matching relationship — skipped",
                    );
                }
            }
        }
        None
    };

    let col_list = col_parts.join(", ");

    // Build FROM clause: main table alias + any CTE LEFT JOINs.
    let from_clause = if let Some(ref plan) = cte_plan {
        format!(
            "{}.{} t{}",
            quote_ident(schema),
            quote_ident(&req.table),
            if plan.join_frags.is_empty() {
                String::new()
            } else {
                format!(" {}", plan.join_frags.join(" "))
            },
        )
    } else {
        format!("{}.{} t", quote_ident(schema), quote_ident(&req.table))
    };

    // Prepend WITH clause when CTEs were generated.
    let select_prefix = if let Some(ref plan) = cte_plan {
        if plan.cte_defs.is_empty() {
            String::new()
        } else {
            format!("WITH {} ", plan.cte_defs.join(",\n"))
        }
    } else {
        String::new()
    };

    let mut sql = format!("{}SELECT {} FROM {}", select_prefix, col_list, from_clause);

    let where_parts = build_where(policy, req.filters.as_deref(), params, next)?;
    if !where_parts.is_empty() {
        sql.push_str(&format!(" WHERE {}", where_parts.join(" AND ")));
    }

    // Enforce LIMIT: clamp caller value to max_limit; inject default when omitted.
    {
        let effective_limit = match req.limit {
            Some(l) => l.min(opts.max_limit).max(1),
            None => opts.default_limit,
        };
        params.push(serde_json::Value::Number(effective_limit.into()));
        sql.push_str(&format!(" LIMIT ${}", *next));
        *next += 1;
    }
    if let Some(offset) = req.offset {
        params.push(serde_json::Value::Number(offset.into()));
        sql.push_str(&format!(" OFFSET ${}", *next));
        *next += 1;
    }

    Ok(CompileResult::Single(CompiledQuery { sql, params: params.clone(), schema: schema.to_string(), pre_read_sql: None, pre_read_params: vec![] }))
}

fn compile_insert(
    req: &QueryRequest,
    schema: &str,
    cols: &[String],
    params: &mut Vec<serde_json::Value>,
    next: &mut usize,
) -> Result<CompiledQuery, EngineError> {
    let data = req.data.as_ref().and_then(|d| d.as_object()).ok_or_else(|| {
        EngineError::MissingField("data (object) required for insert".into())
    })?;

    // If the policy has a column restriction, filter the incoming data to allowed cols.
    let allowed: std::collections::HashSet<&str> =
        if cols.is_empty() { std::collections::HashSet::new() } else { cols.iter().map(|s| s.as_str()).collect() };

    let mut insert_cols: Vec<String> = vec![];
    let mut placeholders: Vec<String> = vec![];

    for (k, v) in data {
        validate_identifier(k)?;
        // Skip columns not in the policy allowlist (CLS enforcement on write).
        if !allowed.is_empty() && !allowed.contains(k.as_str()) {
            continue;
        }
        insert_cols.push(quote_ident(k));
        placeholders.push(format!("${}", *next));
        params.push(v.clone());
        *next += 1;
    }

    if insert_cols.is_empty() {
        return Err(EngineError::MissingField("no writable columns in data".into()));
    }

    let sql = format!(
        "INSERT INTO {}.{} ({}) VALUES ({}) RETURNING *",
        quote_ident(schema),
        quote_ident(&req.table),
        insert_cols.join(", "),
        placeholders.join(", "),
    );

    Ok(CompiledQuery { sql, params: params.clone(), schema: schema.to_string(), pre_read_sql: None, pre_read_params: vec![] })
}

fn compile_update(
    req: &QueryRequest,
    schema: &str,
    cols: &[String],
    policy: &PolicyResult,
    params: &mut Vec<serde_json::Value>,
    next: &mut usize,
) -> Result<CompiledQuery, EngineError> {
    let data = req.data.as_ref().and_then(|d| d.as_object()).ok_or_else(|| {
        EngineError::MissingField("data (object) required for update".into())
    })?;

    let allowed: std::collections::HashSet<&str> =
        if cols.is_empty() { std::collections::HashSet::new() } else { cols.iter().map(|s| s.as_str()).collect() };

    let mut set_parts: Vec<String> = vec![];
    for (k, v) in data {
        validate_identifier(k)?;
        if !allowed.is_empty() && !allowed.contains(k.as_str()) {
            continue;
        }
        set_parts.push(format!("{} = ${}", quote_ident(k), *next));
        params.push(v.clone());
        *next += 1;
    }
    if set_parts.is_empty() {
        return Err(EngineError::MissingField("no writable columns in data".into()));
    }

    let mut sql = format!(
        "UPDATE {}.{} SET {}",
        quote_ident(schema),
        quote_ident(&req.table),
        set_parts.join(", "),
    );

    let where_parts = build_where(policy, req.filters.as_deref(), params, next)?;
    if !where_parts.is_empty() {
        sql.push_str(&format!(" WHERE {}", where_parts.join(" AND ")));
    }
    sql.push_str(" RETURNING *");

    // ── Pre-read SELECT (before-state capture) ─────────────────────────────────
    // Rebuild WHERE with a fresh $N counter that starts at 1 (no SET params).
    // The executor runs this inside the same transaction BEFORE the UPDATE
    // with FOR UPDATE to lock the rows, then stores the result as before_state.
    let mut pre_params: Vec<serde_json::Value> = policy.row_condition_params.clone();
    let mut pre_next = pre_params.len() + 1;
    let pre_where_parts = build_where(policy, req.filters.as_deref(), &mut pre_params, &mut pre_next)
        .unwrap_or_default();
    let pre_read_sql = {
        let mut s = format!("SELECT * FROM {}.{}", quote_ident(schema), quote_ident(&req.table));
        if !pre_where_parts.is_empty() {
            s.push_str(&format!(" WHERE {}", pre_where_parts.join(" AND ")));
        }
        s.push_str(" FOR UPDATE");
        s
    };

    Ok(CompiledQuery {
        sql,
        params: params.clone(),
        schema: schema.to_string(),
        pre_read_sql: Some(pre_read_sql),
        pre_read_params: pre_params,
    })
}

fn compile_delete(
    req: &QueryRequest,
    schema: &str,
    policy: &PolicyResult,
    params: &mut Vec<serde_json::Value>,
    next: &mut usize,
) -> Result<CompiledQuery, EngineError> {
    let mut sql = format!(
        "DELETE FROM {}.{}",
        quote_ident(schema),
        quote_ident(&req.table),
    );

    let where_parts = build_where(policy, req.filters.as_deref(), params, next)?;
    if !where_parts.is_empty() {
        sql.push_str(&format!(" WHERE {}", where_parts.join(" AND ")));
    }
    sql.push_str(" RETURNING *");

    Ok(CompiledQuery { sql, params: params.clone(), schema: schema.to_string(), pre_read_sql: None, pre_read_params: vec![] })
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Build the WHERE clause parts from the RLS row_condition + user filters.
fn build_where(
    policy: &PolicyResult,
    filters: Option<&[Filter]>,
    params: &mut Vec<serde_json::Value>,
    next: &mut usize,
) -> Result<Vec<String>, EngineError> {
    let mut parts: Vec<String> = vec![];

    // Row-level security condition (already has $N placeholders at indices
    // 1..row_condition_params.len(); params are already in the params vec).
    if let Some(rls) = &policy.row_condition_sql {
        parts.push(rls.clone());
    }

    // User-supplied filters.
    for f in filters.unwrap_or(&[]) {
        validate_identifier(&f.column)?;
        let col = format!("t.{}", quote_ident(&f.column));
        let fragment = match f.op.as_str() {
            "eq"       => { push_param(params, next, &f.value); format!("{} = ${}", col, *next - 1) }
            "neq"      => { push_param(params, next, &f.value); format!("{} != ${}", col, *next - 1) }
            "gt"       => { push_param(params, next, &f.value); format!("{} > ${}", col, *next - 1) }
            "gte"      => { push_param(params, next, &f.value); format!("{} >= ${}", col, *next - 1) }
            "lt"       => { push_param(params, next, &f.value); format!("{} < ${}", col, *next - 1) }
            "lte"      => { push_param(params, next, &f.value); format!("{} <= ${}", col, *next - 1) }
            "like"     => { push_param(params, next, &f.value); format!("{} LIKE ${}", col, *next - 1) }
            "ilike"    => { push_param(params, next, &f.value); format!("{} ILIKE ${}", col, *next - 1) }
            "is_null"  => format!("{} IS NULL", col),
            "not_null" => format!("{} IS NOT NULL", col),
            op => return Err(EngineError::UnsupportedOperation(format!("filter op '{}'", op))),
        };
        parts.push(fragment);
    }

    Ok(parts)
}

fn push_param(params: &mut Vec<serde_json::Value>, next: &mut usize, val: &serde_json::Value) {
    params.push(val.clone());
    *next += 1;
}

/// Resolve the final column list.
/// - `user_cols`: columns the caller requested (None = no preference).
/// - `policy_cols`: columns the policy allows (empty = all allowed).
///
/// Result: intersection if both are restricted; otherwise the non-empty set.
fn resolve_columns(
    user_cols: Option<&[String]>,
    policy_cols: &[String],
) -> Result<Vec<String>, EngineError> {
    for c in user_cols.unwrap_or(&[]) {
        validate_identifier(c)?;
    }
    match (user_cols, policy_cols.is_empty()) {
        (None, true) => Ok(vec![]),                        // all cols, no restriction
        (None, false) => Ok(policy_cols.to_vec()),          // policy restricts, no user pref
        (Some(uc), true) => Ok(uc.to_vec()),                // user restricts, policy allows all
        (Some(uc), false) => {
            // Intersect: keep only cols that are in the policy allowlist.
            let allowed: std::collections::HashSet<&str> =
                policy_cols.iter().map(|s| s.as_str()).collect();
            Ok(uc.iter().filter(|c| allowed.contains(c.as_str())).cloned().collect())
        }
    }
}
