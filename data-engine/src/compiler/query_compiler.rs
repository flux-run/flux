use serde::{Deserialize, Serialize};
use crate::engine::error::EngineError;
use crate::policy::PolicyResult;
use crate::router::db_router::{validate_identifier, quote_ident};
use crate::compiler::relational::{parse_selectors, expand_nested, ColumnSelector, RelationshipDef};

// ─── Public request / response types ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
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
#[derive(Debug)]
pub struct CompiledQuery {
    pub sql: String,
    pub params: Vec<serde_json::Value>,
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
    ) -> Result<CompiledQuery, EngineError> {
        validate_identifier(schema)?;
        validate_identifier(&req.table)?;

        // Split user-supplied columns into flat names and nested selectors.
        // Nested selectors (e.g. "posts(id,title)") must not pass through
        // validate_identifier, so we extract them before resolve_columns.
        let (flat_user_cols, nested_sels): (Vec<String>, Vec<_>) =
            if let Some(ref user_cols) = req.columns {
                let parsed = parse_selectors(user_cols);
                let mut flat = vec![];
                let mut nested = vec![];
                for sel in parsed {
                    match sel {
                        ColumnSelector::Flat(c) => flat.push(c),
                        ColumnSelector::Nested { alias, cols } => nested.push((alias, cols)),
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
            "insert" => compile_insert(req, schema, &cols, &mut params, &mut next_param),
            "update" => compile_update(req, schema, &cols, policy, &mut params, &mut next_param),
            "delete" => compile_delete(req, schema, policy, &mut params, &mut next_param),
            op => Err(EngineError::UnsupportedOperation(op.to_string())),
        }
    }
}

// ─── Operation compilers ──────────────────────────────────────────────────────

fn compile_select(
    req: &QueryRequest,
    schema: &str,
    cols: &[String],
    nested_sels: &[(String, Vec<String>)],  // (alias, inner_cols) from nested selectors
    policy: &PolicyResult,
    params: &mut Vec<serde_json::Value>,
    next: &mut usize,
    opts: &CompilerOptions,
) -> Result<CompiledQuery, EngineError> {
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

    // Expand nested selectors into lateral subqueries using the relationships registry.
    for (alias, inner_cols) in nested_sels {
        // Look up the relationship by its alias.
        if let Some(rel) = opts.relationships.iter().find(|r| &r.alias == alias) {
            let subquery = expand_nested(schema, "t", rel, inner_cols);
            col_parts.push(subquery);
        } else {
            tracing::warn!(alias = %alias, "nested selector has no matching relationship — skipped");
        }
    }

    let col_list = col_parts.join(", ");

    let mut sql = format!(
        "SELECT {} FROM {}.{} t",
        col_list,
        quote_ident(schema),
        quote_ident(&req.table),
    );

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

    Ok(CompiledQuery { sql, params: params.clone() })
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

    Ok(CompiledQuery { sql, params: params.clone() })
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

    Ok(CompiledQuery { sql, params: params.clone() })
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

    Ok(CompiledQuery { sql, params: params.clone() })
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
