//! Query execution pipeline — orchestrates all steps for `POST /db/query`.
//!
//! ## Why a pipeline struct?
//!
//! The original query handler was a 380-line function that sequentially
//! executed 14 distinct responsibilities: auth, routing, guard, policy,
//! schema, compile, hooks, execute, transform, events… (SRP violation).
//!
//! `QueryPipeline` extracts those steps into one place, giving the handler a
//! one-liner `pipeline.run(&headers, req).await?` and making each step
//! independently inspectable and testable.
//!
//! ## What the pipeline does NOT do
//!
//! * **Write `trace_requests`** — that table is the gateway's responsibility.
//!   The gateway already records every request that passes through it.
//!   The data-engine only writes to `state_mutations`.

use axum::http::HeaderMap;
use std::sync::Arc;
use std::time::Instant;

use crate::{
    cache::{self, QueryPlan, SchemaCacheEntry},
    compiler::{
        query_compiler::{
            CompileResult, CompiledQuery, ComputedCol, QueryCompiler, QueryRequest,
        },
        relational::{build_batched_plan, load_all_relationships, parse_selectors, ColumnSelector},
        CompilerOptions,
    },
    engine::{auth_context::AuthContext, error::EngineError, schema_rules::SchemaRuleEngine},
    events::EventEmitter,
    executor::{self, MutationContext},
    hooks::{HookEngine, HookEvent},
    policy::PolicyEngine,
    router::DbRouter,
    state::AppState,
    transform::TransformEngine,
};

// ── Public output types ────────────────────────────────────────────────────────

/// Metadata returned alongside the query result — surfaced in the API response
/// `meta` field and used in observability tooling.
pub struct QueryMeta {
    pub strategy: &'static str,
    pub complexity: u64,
    pub elapsed_ms: u128,
    pub rows: usize,
    pub compiled_sql: String,
    pub request_id: String,
}

// ── Pipeline ──────────────────────────────────────────────────────────────────

/// Stateless orchestrator: the `state` it holds is the shared `AppState` for
/// the lifetime of the request.
pub struct QueryPipeline<'a> {
    state: &'a AppState,
}

impl<'a> QueryPipeline<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    /// Execute the full query pipeline and return `(data, meta)`.
    ///
    /// Steps (in order):
    ///  1. Extract auth context from headers
    ///  2. Resolve Postgres schema name
    ///  3. Guard: complexity + nesting depth
    ///  4. Assert schema + table exist
    ///  5. Evaluate row-level policy (cached)
    ///  5.5 Evaluate schema rules from flux db push (RuleExpr AST, mutations only)
    ///  6. Load schema metadata + relationships (L1 cache)
    ///  7. Compile SQL (L2 plan cache for SELECT)
    ///  8. Before-hook (mutations only, skipped on replay)
    ///  9. Execute (single or batched, wrapped in timeout)
    /// 10. After-hook (non-fatal, skipped on replay)
    /// 11. Transform: S3 file columns → presigned URLs
    /// 12. Emit db mutation event (skipped on replay)
    pub async fn run(
        &self,
        headers: &HeaderMap,
        req: QueryRequest,
    ) -> Result<(serde_json::Value, QueryMeta), EngineError> {
        // ── Step 1: Auth ──────────────────────────────────────────────────────
        let auth = AuthContext::from_headers(headers).map_err(EngineError::MissingField)?;

        // ── Step 2: Schema name ───────────────────────────────────────────────
        let schema =
            DbRouter::schema_name(&req.database)?;

        // ── Step 3: Guards ────────────────────────────────────────────────────
        // Fast CPU-only checks — reject before touching the DB.
        let complexity = self.state.query_guard.check_complexity(&req)?;
        let _ = self.state.query_guard.check_depth(&req)?;

        // ── Step 4: Existence ─────────────────────────────────────────────────
        DbRouter::assert_exists(&self.state.pool, &schema).await?;
        DbRouter::assert_table_exists(&self.state.pool, &schema, &req.table).await?;

        // ── Step 5: Policy ────────────────────────────────────────────────────
        let policy = PolicyEngine::evaluate_cached(
            &self.state.pool,
            &auth,
            &req.table,
            &req.operation,
            &self.state.cache.policy_cache,
        )
        .await?;

        // ── Step 5.5: Schema rules (RuleExpr from flux db push) ───────────────
        // Evaluates compiled TypeScript rules stored in table_metadata.schema_rules.
        // Skipped in replay mode (rules would re-deny replayed mutations).
        if !auth.is_replay && req.operation != "select" {
            let input = req.data.as_ref().unwrap_or(&serde_json::Value::Null);
            let rid = headers
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("-");
            SchemaRuleEngine::enforce(
                &self.state.pool,
                &auth,
                &req.table,
                &req.operation,
                input,
                &serde_json::Value::Null, // pre-read row not yet available at this stage
                rid,
            )
            .await?;
        }

        let sk =
            cache::schema_key(&schema, &req.table);
        let (col_meta, relationships) = match self.state.cache.schema_cache.get(&sk) {
            Some(entry) => {
                tracing::debug!(key = %sk, "schema cache hit");
                (entry.col_meta.clone(), entry.relationships.clone())
            }
            None => {
                let cm = TransformEngine::load_columns(
                    &self.state.pool,
                    &schema,
                    &req.table,
                )
                .await?;
                let rels = load_all_relationships(
                    &self.state.pool,
                    &schema,
                )
                .await?;
                self.state.cache.schema_cache.insert(
                    sk,
                    SchemaCacheEntry {
                        col_meta: cm.clone(),
                        relationships: rels.clone(),
                    },
                );
                (cm, rels)
            }
        };

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

        // ── Step 7: Compile (L2 plan cache for SELECT) ────────────────────────
        let opts = CompilerOptions {
            default_limit: self.state.default_query_limit,
            max_limit: self.state.max_query_limit,
            computed_cols,
            relationships,
        };

        // Parse nested selectors once — needed for plan-cache reconstruction
        // and the batched-path depth decision inside the compiler.
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
            let plan_key =
                cache::build_plan_key(&schema, &req, &policy);
            match self.state.cache.plan_cache.get(&plan_key) {
                Some(plan) => {
                    tracing::debug!("plan cache hit");
                    let params = cache::extract_select_params(
                        &req,
                        &policy,
                        opts.default_limit,
                        opts.max_limit,
                    );
                    let root_cq = CompiledQuery {
                        sql: plan.sql,
                        params,
                        schema: schema.clone(),
                        pre_read_sql: None,
                        pre_read_params: vec![],
                    };
                    if plan.is_batched {
                        let batched_plan = build_batched_plan(
                            &schema,
                            &req.table,
                            &nested_sels_for_plan,
                            &opts.relationships,
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
                        CompileResult::Single(cq) => (cq.sql.clone(), false),
                        CompileResult::Batched { root, .. } => (root.sql.clone(), true),
                    };
                    self.state.cache.plan_cache.insert(
                        plan_key,
                        QueryPlan { sql: cache_sql, has_file_cols, is_batched },
                    );
                    cr
                }
            }
        } else {
            QueryCompiler::compile(&req, &policy, &schema, &opts)?
        };

        // Extract meta we'll need after execution (borrow compile_result before
        // the async block consumes it).
        let strategy: &'static str = match &compile_result {
            CompileResult::Single(_)      => "single",
            CompileResult::Batched { .. } => "batched",
        };
        let compiled_sql = match &compile_result {
            CompileResult::Single(cq)          => cq.sql.clone(),
            CompileResult::Batched { root, .. } => root.sql.clone(),
        };

        let request_id = headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-")
            .to_string();
        let span_id_owned = headers
            .get("x-span-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        // ── Step 8: Before hook ───────────────────────────────────────────────
        // Hooks fire before the SQL write; a failure aborts the operation.
        // Skipped in replay mode — hooks must not re-fire on replay.
        if !auth.is_replay {
            if let Some(before) = Self::before_event(&req.operation) {
                HookEngine::run(
                    &self.state.pool,
                    &self.state.http_client,
                    &self.state.runtime_url,
                    &auth,
                    &req.table,
                    before,
                    &req.data.clone().unwrap_or(serde_json::Value::Null),
                    &request_id,
                )
                .await?;
            }
        }

        let stmt_timeout_ms = if auth.is_replay {
            self.state.statement_timeout_ms * 6
        } else {
            self.state.statement_timeout_ms
        };
        let mut_ctx = MutationContext {
            schema: &schema,
            request_id: &request_id,
            span_id: span_id_owned.as_deref(),
            table: &req.table,
            operation: &req.operation,
            user_id: &auth.user_id,
            statement_timeout_ms: stmt_timeout_ms,
        };

        // ── Step 9: Execute ───────────────────────────────────────────────────
        // Wrapped in the query guard's timeout so runaway queries cannot hold
        // a Postgres connection indefinitely.
        let t_exec = Instant::now();
        let result = self
            .state
            .query_guard
            .with_timeout(async {
                match compile_result {
                    CompileResult::Single(ref cq) => {
                        executor::execute(&self.state.pool, cq, &mut_ctx).await
                    }
                    CompileResult::Batched { ref root, ref plan } => {
                        let root_result =
                            executor::execute(&self.state.pool, root, &mut_ctx).await?;
                        let mut rows = root_result.as_array().cloned().unwrap_or_default();
                        executor::execute_batched(&self.state.pool, &mut rows, &plan.stages)
                            .await?;
                        Ok(serde_json::Value::Array(rows))
                    }
                }
            })
            .await?;

        let elapsed_ms = t_exec.elapsed().as_millis();
        let rows_returned = result.as_array().map_or(0, |a| a.len());
        tracing::info!(
            op    = %req.operation,
            table = %req.table,
            complexity,
            strategy,
            elapsed_ms = %elapsed_ms,
            rows       = rows_returned,
            request_id = %request_id,
            "query executed",
        );

        // ── Step 10: After hook (non-fatal) ───────────────────────────────────
        if !auth.is_replay {
            if let Some(after) = Self::after_event(&req.operation) {
                if let Err(e) = HookEngine::run(
                    &self.state.pool,
                    &self.state.http_client,
                    &self.state.runtime_url,
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

        // ── Step 11: Transform (SELECT only) ─────────────────────────────────
        // Replace S3 object keys with presigned URLs for file-typed columns.
        let result = if req.operation == "select" {
            TransformEngine::apply(
                result,
                &col_meta,
                self.state.file_engine.as_deref(),
                &auth,
            )
            .await?
        } else {
            result
        };

        // ── Step 12: Emit event ───────────────────────────────────────────────
        // INSERT / UPDATE / DELETE → emit DB event for subscription delivery.
        // Skipped in replay mode — would re-trigger webhooks / functions.
        if !auth.is_replay {
            if let Some(op) = EventEmitter::verb_for(&req.operation) {
                let record_id = EventEmitter::extract_record_id(&result);
                EventEmitter::emit(
                    &self.state.pool,
                    &req.table,
                    op,
                    record_id.as_deref(),
                    &result,
                    Some(&request_id),
                )
                .await;
            }
        }

        Ok((
            result,
            QueryMeta {
                strategy,
                complexity,
                elapsed_ms,
                rows: rows_returned,
                compiled_sql,
                request_id,
            },
        ))
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn before_event(op: &str) -> Option<HookEvent> {
        match op {
            "insert" => Some(HookEvent::BeforeInsert),
            "update" => Some(HookEvent::BeforeUpdate),
            "delete" => Some(HookEvent::BeforeDelete),
            _ => None,
        }
    }

    fn after_event(op: &str) -> Option<HookEvent> {
        match op {
            "insert" => Some(HookEvent::AfterInsert),
            "update" => Some(HookEvent::AfterUpdate),
            "delete" => Some(HookEvent::AfterDelete),
            _ => None,
        }
    }
}
