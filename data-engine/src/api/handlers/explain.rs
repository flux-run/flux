/// `POST /db/explain` — dry-run a query and return the full compiler output.
///
/// Runs the same pipeline as `/db/query` — auth → guard → policy → compile —
/// but stops before execution.  Nothing is written to or read from the database.
///
/// Response shape:
/// ```json
/// {
///   "query_plan": {
///     "table":     "users",
///     "operation": "select",
///     "database":  null
///   },
///   "policies_applied": {
///     "role":            "authenticated",
///     "allowed_columns": [],            // empty = all allowed
///     "row_condition":   "tenant_id = $1",
///     "row_params":      ["5b5f77d1-..."]
///   },
///   "compiled_sql":    "SELECT id, name FROM t_acme_main.users WHERE...",
///   "guard": {
///     "complexity_score": 4,
///     "max_complexity":   500,
///     "over_limit":       false,
///     "filters":          2,
///     "selector_depth":   0
///   }
/// }
/// ```
use axum::{extract::State, http::HeaderMap, Json};
use serde_json::json;
use std::sync::Arc;

use crate::{
    compiler::{
        query_compiler::{QueryCompiler, QueryRequest},
        CompilerOptions,
    },
    engine::{auth_context::AuthContext, error::EngineError},
    policy::PolicyEngine,
    query_guard::score_request,
    router::DbRouter,
    state::AppState,
};

/// POST /db/explain
///
/// Accepts the same request body as `POST /db/query`.
/// Returns the query plan, applied policies, compiled SQL, and guard score
/// without touching the database.
pub async fn handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<QueryRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    // ── 1. Auth ───────────────────────────────────────────────────────────────
    let auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    // ── 2. Schema name ────────────────────────────────────────────────────────
    let schema = DbRouter::schema_name(&auth.tenant_slug, &auth.project_slug, &req.database)?;

    // ── 3. Guard (compute score; do NOT reject — this is an explain) ──────────
    let complexity_score = score_request(&req);
    let over_limit = complexity_score > state.query_guard.max_complexity;

    // Depth check (informational; count it but don't abort).
    let filter_count = req.filters.as_ref().map(|f| f.len()).unwrap_or(0);
    let selector_depth = {
        use crate::compiler::relational::{parse_selectors, selector_depth};
        let cols = req.columns.clone().unwrap_or_default();
        let sels = parse_selectors(&cols);
        sels.iter().map(|s| selector_depth(s)).max().unwrap_or(0)
    };

    // ── 4. Policy evaluation ──────────────────────────────────────────────────
    let policy = PolicyEngine::evaluate_cached(
        &state.pool,
        &auth,
        &req.table,
        &req.operation,
        &state.cache.policy_cache,
    )
    .await?;

    // ── 5. Compilation (no execution) ─────────────────────────────────────────
    let opts = CompilerOptions::with_limits(state.default_query_limit, state.max_query_limit);
    let compile_result = QueryCompiler::compile(&req, &policy, &schema, &opts);

    let compiled_sql = match compile_result {
        Ok(ref cr) => {
            use crate::compiler::query_compiler::CompileResult;
            match cr {
                CompileResult::Single(q) => q.sql.clone(),
                CompileResult::Batched { root, .. } => root.sql.clone(),
            }
        }
        Err(ref e) => format!("<compile error: {}>", e),
    };

    // ── 6. Assemble response ──────────────────────────────────────────────────
    Ok(Json(json!({
        "query_plan": {
            "table":     req.table,
            "operation": req.operation,
            "database":  req.database,
            "schema":    schema,
        },
        "policies_applied": {
            "role":            auth.role,
            "allowed_columns": policy.allowed_columns,
            "row_condition":   policy.row_condition_sql,
            "row_params":      policy.row_condition_params,
        },
        "compiled_sql": compiled_sql,
        "guard": {
            "complexity_score": complexity_score,
            "max_complexity":   state.query_guard.max_complexity,
            "over_limit":       over_limit,
            "filters":          filter_count,
            "selector_depth":   selector_depth,
        }
    })))
}
