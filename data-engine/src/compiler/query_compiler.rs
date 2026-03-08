use serde::{Deserialize, Serialize};
use crate::engine::error::EngineError;
use crate::policy::PolicyResult;
use crate::router::db_router::{validate_identifier, quote_ident};

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

/// Compilation-time options supplied by the caller (sourced from `AppState`).
#[derive(Debug, Clone, Copy)]
pub struct CompilerOptions {
    /// Rows returned per query when the caller omits a LIMIT.
    pub default_limit: i64,
}

impl Default for CompilerOptions {
    fn default() -> Self {
        Self { default_limit: 1000 }
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

        // Columns to operate on: intersect request columns with policy-allowed columns.
        let cols = resolve_columns(req.columns.as_deref(), &policy.allowed_columns)?;

        // Accumulated params — row_condition params come first so their $N indices
        // are stable; filter params are appended after.
        let mut params: Vec<serde_json::Value> = policy.row_condition_params.clone();
        let mut next_param = params.len() + 1; // 1-based

        match req.operation.as_str() {
            "select" => compile_select(req, schema, &cols, policy, &mut params, &mut next_param, opts.default_limit),
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
    policy: &PolicyResult,
    params: &mut Vec<serde_json::Value>,
    next: &mut usize,
    default_limit: i64,
) -> Result<CompiledQuery, EngineError> {
    let col_list = if cols.is_empty() {
        "*".to_string()
    } else {
        cols.iter().map(|c| quote_ident(c)).collect::<Vec<_>>().join(", ")
    };

    let mut sql = format!(
        "SELECT {} FROM {}.{}",
        col_list,
        quote_ident(schema),
        quote_ident(&req.table),
    );

    let where_parts = build_where(policy, req.filters.as_deref(), params, next)?;
    if !where_parts.is_empty() {
        sql.push_str(&format!(" WHERE {}", where_parts.join(" AND ")));
    }

    if let Some(limit) = req.limit {
        params.push(serde_json::Value::Number(limit.into()));
        sql.push_str(&format!(" LIMIT ${}", *next));
        *next += 1;
    } else {
        // Enforce a hard cap when the caller omits LIMIT to protect large tables.
        params.push(serde_json::Value::Number(default_limit.into()));
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
        let col = quote_ident(&f.column);
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
