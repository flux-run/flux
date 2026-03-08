use axum::{extract::State, http::HeaderMap, Json};
use serde_json::json;
use std::sync::Arc;

use crate::{
    compiler::{
        query_compiler::{ComputedCol, QueryCompiler, QueryRequest},
        CompilerOptions,
    },
    engine::{auth_context::AuthContext, error::EngineError},
    events::EventEmitter,
    executor,
    hooks::{HookEngine, HookEvent},
    policy::PolicyEngine,
    router::DbRouter,
    state::AppState,
    transform::TransformEngine,
};

/// Map operation string to the before/after hook event pair.
fn hook_events(op: &str) -> Option<(HookEvent, HookEvent)> {
    match op {
        "insert" => Some((HookEvent::BeforeInsert, HookEvent::AfterInsert)),
        "update" => Some((HookEvent::BeforeUpdate, HookEvent::AfterUpdate)),
        "delete" => Some((HookEvent::BeforeDelete, HookEvent::AfterDelete)),
        _ => None, // SELECT has no hooks
    }
}

/// POST /db/query
pub async fn handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<QueryRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    // 1. Auth context.
    let auth = AuthContext::from_headers(&headers)
        .map_err(EngineError::MissingField)?;

    // 2. Schema name.
    let schema = DbRouter::schema_name(&auth.tenant_slug, &auth.project_slug, &req.database)?;

    // 3. Schema + table existence (blocks system catalog access).
    DbRouter::assert_exists(&state.pool, &schema).await?;
    DbRouter::assert_table_exists(&state.pool, &schema, &req.table).await?;

    // 4. Policy evaluation with cache.
    let policy = PolicyEngine::evaluate_cached(
        &state.pool,
        &auth,
        &req.table,
        &req.operation,
        &state.policy_cache,
    )
    .await?;

    // 5. Load column metadata (computed cols, file cols).
    let col_meta = TransformEngine::load_columns(
        &state.pool,
        auth.tenant_id,
        auth.project_id,
        &schema,
        &req.table,
    )
    .await?;

    // Build computed col list for the compiler.
    let computed_cols: Vec<ComputedCol> = col_meta
        .iter()
        .filter(|c| c.fb_type == "computed")
        .filter_map(|c| {
            c.computed_expr.as_ref().map(|expr| ComputedCol {
                name: c.name.clone(),
                expr: expr.clone(),
            })
        })
        .collect();

    // 6. Compile (CLS + RLS + limit enforcement + computed cols).
    let opts = CompilerOptions {
        default_limit: state.default_query_limit,
        max_limit: state.max_query_limit,
        computed_cols,
    };
    let compiled = QueryCompiler::compile(&req, &policy, &schema, &opts)?;

    tracing::debug!(sql = %compiled.sql, "executing compiled query");

    // 7. Before hook (runs before the SQL; can abort the operation).
    let hook_events = hook_events(&req.operation);
    if let Some((before, _)) = hook_events {
        HookEngine::run(
            &state.pool,
            &state.http_client,
            &state.runtime_url,
            &auth,
            &req.table,
            before,
            &req.data.clone().unwrap_or(serde_json::Value::Null),
        )
        .await?;
    }

    // 8. Execute inside a transaction.
    let result = executor::execute(&state.pool, &compiled).await?;

    // 9. After hook (non-fatal: errors are logged, response still returns data).
    if let Some((_, after)) = hook_events {
        if let Err(e) = HookEngine::run(
            &state.pool,
            &state.http_client,
            &state.runtime_url,
            &auth,
            &req.table,
            after,
            &result,
        )
        .await
        {
            tracing::warn!(error = %e, "after-hook failed (non-fatal)");
        }
    }

    // 10. Transform: replace S3 file keys with presigned URLs (SELECT only).
    let result = if req.operation == "select" {
        TransformEngine::apply(
            result,
            &col_meta,
            state.file_engine.as_deref(),
            &auth,
        )
        .await?
    } else {
        result
    };

    // 11. Emit event for mutations (INSERT / UPDATE / DELETE).
    if let Some(verb) = EventEmitter::verb_for(&req.operation) {
        EventEmitter::emit(&state.pool, &auth, &req.table, verb, &result).await;
    }

    Ok(Json(json!({ "data": result })))
}
