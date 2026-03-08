use axum::{extract::State, http::HeaderMap, Json};
use serde_json::json;
use std::sync::Arc;

use crate::{
    compiler::{
        query_compiler::{QueryCompiler, QueryRequest},
        CompilerOptions,
    },
    engine::{auth_context::AuthContext, error::EngineError},
    executor,
    policy::PolicyEngine,
    router::DbRouter,
    state::AppState,
};

/// POST /db/query
pub async fn handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<QueryRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    // 1. Auth context.
    let auth = AuthContext::from_headers(&headers)
        .map_err(|e| EngineError::MissingField(e))?;

    // 2. Resolve schema name.
    let schema = DbRouter::schema_name(&auth.tenant_slug, &auth.project_slug, &req.database)?;

    // 3. Schema existence check.
    DbRouter::assert_exists(&state.pool, &schema).await?;

    // 4. Table whitelist — blocks pg_catalog, information_schema, etc.
    DbRouter::assert_table_exists(&state.pool, &schema, &req.table).await?;

    // 5. Policy evaluation (cache-first).
    let policy = PolicyEngine::evaluate_cached(
        &state.pool,
        &auth,
        &req.table,
        &req.operation,
        &state.policy_cache,
    )
    .await?;

    // 6. Compile with CLS + RLS applied, enforcing a default row limit.
    let opts = CompilerOptions { default_limit: state.default_query_limit };
    let compiled = QueryCompiler::compile(&req, &policy, &schema, &opts)?;

    tracing::debug!(sql = %compiled.sql, "executing compiled query");

    // 7. Execute inside a transaction and return uniform JSON array.
    let result = executor::execute(&state.pool, &compiled).await?;

    Ok(Json(json!({ "data": result })))
}
