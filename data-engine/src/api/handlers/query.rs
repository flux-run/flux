use axum::{extract::State, http::HeaderMap, Json};
use serde_json::json;
use std::sync::Arc;

use crate::{
    cache,
    cache::SchemaCacheEntry,
    compiler::{
        query_compiler::{CompiledQuery, ComputedCol, QueryCompiler, QueryRequest},
        relational::load_relationships,
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

    // 5. Schema metadata — schema cache (L1) or DB.  Saves two round-trips
    //    (`load_columns` + `load_relationships`) on every request where the
    //    table's metadata hasn't changed since the last 60-second TTL window.
    let sk = cache::schema_key(auth.tenant_id, auth.project_id, &schema, &req.table);
    let (col_meta, relationships) = {
        // Fast path — read lock only, no DB.
        let hit = {
            let guard = state.schema_cache.read().await;
            guard
                .get(&sk)
                .filter(|e| e.is_fresh())
                .map(|e| (e.col_meta.clone(), e.relationships.clone()))
        };
        if let Some(pair) = hit {
            tracing::debug!(key = %sk, "schema cache hit");
            pair
        } else {
            // Slow path — load both, then populate cache.
            let cm = TransformEngine::load_columns(
                &state.pool, auth.tenant_id, auth.project_id, &schema, &req.table,
            ).await?;
            let rels = load_relationships(
                &state.pool, auth.tenant_id, auth.project_id, &schema, &req.table,
            ).await?;
            {
                let mut guard = state.schema_cache.write().await;
                guard.insert(sk, SchemaCacheEntry {
                    col_meta: cm.clone(),
                    relationships: rels.clone(),
                    inserted_at: std::time::Instant::now(),
                });
            }
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
    //    For SELECT: if we've seen this exact query shape + policy before, skip
    //    the compiler entirely and reconstruct only the bind-parameter list
    //    (O(filters) instead of O(cols × filters)).
    let opts = CompilerOptions {
        default_limit: state.default_query_limit,
        max_limit: state.max_query_limit,
        computed_cols,
        relationships,
    };
    let compiled = if req.operation == "select" {
        let plan_key = cache::build_plan_key(
            auth.tenant_id, auth.project_id, &schema, &req, &policy,
            opts.default_limit, opts.max_limit,
        );
        let cached_sql = {
            let guard = state.plan_cache.read().await;
            guard.get(&plan_key).filter(|p| p.is_fresh()).map(|p| p.sql.clone())
        };
        if let Some(sql) = cached_sql {
            tracing::debug!("plan cache hit");
            let params = cache::extract_select_params(&req, &policy, opts.default_limit, opts.max_limit);
            CompiledQuery { sql, params }
        } else {
            let cq = QueryCompiler::compile(&req, &policy, &schema, &opts)?;
            {
                let mut guard = state.plan_cache.write().await;
                guard.insert(plan_key, cache::CachedPlan {
                    sql: cq.sql.clone(),
                    inserted_at: std::time::Instant::now(),
                });
            }
            cq
        }
    } else {
        QueryCompiler::compile(&req, &policy, &schema, &opts)?
    };

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
