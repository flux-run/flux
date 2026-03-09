use axum::{extract::State, http::HeaderMap, Json};
use serde_json::json;
use std::sync::Arc;

use crate::{
    cache,
    cache::{SchemaCacheEntry, QueryPlan},
    compiler::{
        query_compiler::{CompileResult, CompiledQuery, ComputedCol, QueryCompiler, QueryRequest},
        relational::{load_all_relationships, parse_selectors, build_batched_plan, ColumnSelector},
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

    // 5. Schema metadata — schema cache (L1) or DB.
    //    Moka handles TTL automatically; no locking or freshness checks needed.
    let sk = cache::schema_key(auth.tenant_id, auth.project_id, &schema, &req.table);
    let (col_meta, relationships) = match state.schema_cache.get(&sk) {
        Some(entry) => {
            tracing::debug!(key = %sk, "schema cache hit");
            (entry.col_meta.clone(), entry.relationships.clone())
        }
        None => {
            let cm = TransformEngine::load_columns(
                &state.pool, auth.tenant_id, auth.project_id, &schema, &req.table,
            ).await?;
            let rels = load_all_relationships(
                &state.pool, auth.tenant_id, auth.project_id, &schema,
            ).await?;
            state.schema_cache.insert(sk, SchemaCacheEntry {
                col_meta: cm.clone(),
                relationships: rels.clone(),
            });
            (cm, rels)
        }
    };

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

    // 6. Compile — plan cache (L2) for SELECT, full compile otherwise.
    //
    //    Cache hit:  reconstruct bind params only (O(filters) walk).
    let opts = CompilerOptions {
        default_limit: state.default_query_limit,
        max_limit: state.max_query_limit,
        computed_cols,
        relationships,
    };

    //    Parse nested selectors once; needed both for cache-hit BatchedPlan
    // reconstruction and for the batched-path depth decision in the compiler.
    let nested_sels_for_plan: Vec<ColumnSelector> = req
        .columns
        .as_ref()
        .map(|cols| {
            parse_selectors(cols)
                .into_iter()
                .filter(|s| matches!(s, ColumnSelector::Nested { .. }))
                .collect()
        })
        .unwrap_or_default();

    let compile_result: CompileResult = if req.operation == "select" {
        let plan_key = cache::build_plan_key(
            auth.tenant_id, auth.project_id, &schema, &req, &policy,
        );
        match state.plan_cache.get(&plan_key) {
            Some(plan) => {
                tracing::debug!("plan cache hit");
                let params = cache::extract_select_params(
                    &req, &policy, opts.default_limit, opts.max_limit,
                );
                let root_cq = CompiledQuery { sql: plan.sql, params };
                if plan.is_batched {
                    // Rebuild BatchedPlan from schema-cached relationships.
                    let batched_plan = build_batched_plan(
                        &schema, &req.table, &nested_sels_for_plan, &opts.relationships,
                    );
                    CompileResult::Batched { root: root_cq, plan: batched_plan }
                } else {
                    CompileResult::Single(root_cq)
                }
            }
            None => {
                let cr = QueryCompiler::compile(&req, &policy, &schema, &opts)?;
                let has_file_cols = col_meta.iter().any(|c| c.fb_type == "file");
                let (cache_sql, is_batched) = match &cr {
                    CompileResult::Single(cq)          => (cq.sql.clone(),   false),
                    CompileResult::Batched { root, .. } => (root.sql.clone(), true),
                };
                state.plan_cache.insert(plan_key, QueryPlan {
                    sql: cache_sql,
                    has_file_cols,
                    is_batched,
                });
                cr
            }
        }
    } else {
        QueryCompiler::compile(&req, &policy, &schema, &opts)?
    };

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

    // 8. Execute — single SQL or batched per-level fetches.
    let result = match compile_result {
        CompileResult::Single(ref cq) => {
            tracing::debug!(sql = %cq.sql, "executing compiled query");
            executor::execute(&state.pool, cq).await?
        }
        CompileResult::Batched { ref root, ref plan } => {
            tracing::debug!(sql = %root.sql, levels = plan.stages.len(), "executing batched query");
            let root_result = executor::execute(&state.pool, root).await?;
            let mut rows = root_result.as_array().cloned().unwrap_or_default();
            executor::execute_batched(&state.pool, &mut rows, &plan.stages).await?;
            serde_json::Value::Array(rows)
        }
    };

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
    if let Some(op) = EventEmitter::verb_for(&req.operation) {
        let record_id = EventEmitter::extract_record_id(&result);
        EventEmitter::emit(
            &state.pool,
            &auth,
            &req.table,
            op,
            record_id.as_deref(),
            &result,
        )
        .await;
    }

    Ok(Json(json!({ "data": result })))
}
