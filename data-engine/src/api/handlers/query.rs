use axum::{extract::State, http::HeaderMap, Json};
use serde_json::json;
use std::sync::Arc;

use crate::{
    compiler::query_compiler::{QueryCompiler, QueryRequest},
    engine::{auth_context::AuthContext, error::EngineError},
    executor,
    policy::PolicyEngine,
    router::DbRouter,
    state::AppState,
};

/// POST /db/query
///
/// Body: `QueryRequest` JSON.
/// Headers: x-tenant-id, x-project-id, x-tenant-slug, x-project-slug,
///          x-user-id, x-user-role
pub async fn handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<QueryRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    // 1. Extract authentication context from headers.
    let auth = AuthContext::from_headers(&headers)
        .map_err(|e| EngineError::MissingField(e))?;

    // 2. Resolve the schema name (e.g. "t_acme_auth_main").
    let schema = DbRouter::schema_name(&auth.tenant_slug, &auth.project_slug, &req.database)?;

    // 3. Ensure the schema exists.
    DbRouter::assert_exists(&state.pool, &schema).await?;

    // 4. Evaluate policy for this (role, table, operation) triple.
    let policy = PolicyEngine::evaluate(
        &state.pool,
        &auth,
        &req.table,
        &req.operation,
    )
    .await?;

    // 5. Compile the query with CLS + RLS applied.
    let compiled = QueryCompiler::compile(&req, &policy, &schema)?;

    tracing::debug!(sql = %compiled.sql, "executing compiled query");

    // 6. Execute and return the JSON array.
    let result = executor::execute(&state.pool, &compiled).await?;

    Ok(Json(json!({ "data": result })))
}
