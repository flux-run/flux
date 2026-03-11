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
    executor::{self, MutationContext},
    hooks::{HookEngine, HookEvent},
    policy::PolicyEngine,
    router::DbRouter,
    state::AppState,
    transform::TransformEngine,
};

/// Extract only the safe, non-sensitive headers needed for replay.
/// Never stores Authorization or any credential header.
fn safe_headers(headers: &HeaderMap) -> serde_json::Value {
    let keys = [
        "x-tenant-id", "x-project-id", "x-tenant-slug", "x-project-slug",
        "x-user-id",   "x-user-role",  "x-request-id",  "x-span-id",
        "x-flux-replay", "content-type",
    ];
    let map: serde_json::Map<String, serde_json::Value> = keys
        .iter()
        .filter_map(|k| {
            headers.get(*k)
                .and_then(|v| v.to_str().ok())
                .map(|v| (k.to_string(), serde_json::Value::String(v.to_string())))
        })
        .collect();
    serde_json::Value::Object(map)
}

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

    // 2.5 Complexity guard + depth guard — fast CPU checks before any DB work.
    //     Both reject immediately, no schema lookup or compilation.
    let complexity  = state.query_guard.check_complexity(&req)?;
    let _nest_depth = state.query_guard.check_depth(&req)?;;

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
                let root_cq = CompiledQuery { sql: plan.sql, params, schema: schema.clone(), pre_read_sql: None, pre_read_params: vec![] };
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

    // Pull request_id and span_id from headers — forwarded by the runtime for
    // every DB call so mutations can be linked back to the span that caused them.
    let request_id = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_string();
    let span_id_owned = headers
        .get("x-span-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // 7. Before hook (runs before the SQL; can abort the operation).
    //    Skipped in replay mode — hooks must not fire again on replayed mutations.
    let hook_events = hook_events(&req.operation);
    if !auth.is_replay {
        if let Some((before, _)) = hook_events {
            HookEngine::run(
                &state.pool,
                &state.http_client,
                &state.runtime_url,
                &auth,
                &req.table,
                before,
                &req.data.clone().unwrap_or(serde_json::Value::Null),
                &request_id,
            )
            .await?;
        }
    }

    // Build once and share across both executor call sites below.
    let mut_ctx = MutationContext {
        schema:     &schema,
        request_id: &request_id,
        span_id:    span_id_owned.as_deref(),
        tenant_id:  auth.tenant_id,
        project_id: auth.project_id,
        table:      &req.table,
        operation:  &req.operation,
        user_id:    &auth.user_id,
    };

    // 8. Execute — single SQL or batched per-level fetches.
    //    Wrapped in a timeout so runaway queries don't hold connections forever.
    let t_exec = std::time::Instant::now();
    let result = state.query_guard.with_timeout(async {
        match compile_result {
            CompileResult::Single(ref cq) => {
                executor::execute(&state.pool, cq, &mut_ctx).await
            }
            CompileResult::Batched { ref root, ref plan } => {
                let root_result = executor::execute(&state.pool, root, &mut_ctx).await?;
                let mut rows = root_result.as_array().cloned().unwrap_or_default();
                executor::execute_batched(&state.pool, &mut rows, &plan.stages).await?;
                Ok(serde_json::Value::Array(rows))
            }
        }
    }).await?;

    let elapsed_ms = t_exec.elapsed().as_millis();
    let strategy = match &compile_result {
        CompileResult::Single(_)           => "single",
        CompileResult::Batched { .. }      => "batched",
    };
    let compiled_sql = match &compile_result {
        CompileResult::Single(cq)          => cq.sql.clone(),
        CompileResult::Batched { root, .. } => root.sql.clone(),
    };
    let rows_returned = result.as_array().map_or(0, |a| a.len());
    tracing::info!(
        op    = %req.operation,
        table = %req.table,
        complexity,
        strategy,
        elapsed_ms = %elapsed_ms,
        rows = rows_returned,
        request_id = %request_id,
        "query executed",
    );

    // 9. After hook (non-fatal: errors are logged, response still returns data).
    //    Skipped in replay mode — same reason as before-hook.
    if !auth.is_replay {
        if let Some((_, after)) = hook_events {
            if let Err(e) = HookEngine::run(
                &state.pool,
                &state.http_client,
                &state.runtime_url,
                &auth,
                &req.table,
                after,
                &result,
                &request_id,
            )
            .await
            {
                tracing::warn!(error = %e, "after-hook failed (non-fatal)");
            }
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
    //    Skipped in replay mode — events would re-trigger webhooks/functions.
    if !auth.is_replay {
        if let Some(op) = EventEmitter::verb_for(&req.operation) {
            let record_id = EventEmitter::extract_record_id(&result);
            EventEmitter::emit(
                &state.pool,
                &auth,
                &req.table,
                op,
                record_id.as_deref(),
                &result,
                Some(&request_id),
            )
            .await;
        }
    }

    // 12. Persist request envelope to trace_requests (fire-and-forget).
    //     Enables flux incident replay without depending on gateway log retention.
    //     Non-fatal: failure is logged as a warning; the user response is unaffected.
    {
        let pool2      = state.pool.clone();
        let rid        = request_id.clone();
        let tenant_id  = auth.tenant_id;
        let project_id = auth.project_id;
        let safe_hdrs  = safe_headers(&headers);
        let body_val   = serde_json::to_value(&req).unwrap_or(serde_json::Value::Null);
        // Truncate result to first 100 rows to cap storage overhead.
        let resp_val = match result.as_array() {
            Some(arr) if arr.len() > 100 => {
                serde_json::Value::Array(arr[..100].to_vec())
            }
            _ => result.clone(),
        };
        let dur = elapsed_ms as i32;
        tokio::spawn(async move {
            let res = sqlx::query(
                r#"
                INSERT INTO fluxbase_internal.trace_requests
                    (request_id, tenant_id, project_id,
                     method, path, headers, body,
                     response_status, response_body, duration_ms)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                ON CONFLICT (request_id) DO NOTHING
                "#,
            )
            .bind(&rid)
            .bind(tenant_id)
            .bind(project_id)
            .bind("POST")
            .bind("/db/query")
            .bind(&safe_hdrs)
            .bind(&body_val)
            .bind(200_i32)
            .bind(&resp_val)
            .bind(dur)
            .execute(&pool2)
            .await;
            if let Err(e) = res {
                tracing::warn!(error = %e, request_id = %rid, "trace_requests write failed");
            }
        });
    }

    Ok(Json(json!({
        "data": result,
        "meta": {
            "strategy": strategy,
            "complexity": complexity,
            "elapsed_ms": elapsed_ms,
            "rows": rows_returned,
            "sql": compiled_sql,
            "request_id": request_id,
        }
    })))
}
