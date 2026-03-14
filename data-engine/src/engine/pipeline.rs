//! Query execution pipeline — orchestrates all steps for `POST /db/query`.
//!
//! ## Why a pipeline struct?
//!
//! `QueryPipeline` extracts the query path into one place, giving the handler a
//! one-liner `pipeline.run(&headers, req).await?` and making each step
//! independently inspectable and testable.
//!
//! ## What the pipeline does
//!
//! 1. Extract auth context from headers (request_id, user_id, is_replay flag)
//! 2. Resolve Postgres schema name
//! 3. Guard: complexity + nesting depth
//! 4. Assert schema + table exist
//! 5. Load schema metadata + relationships (L1 cache)
//! 6. Compile SQL (L2 plan cache for SELECT)
//! 7. Execute (single or batched, wrapped in timeout) → mutation recording inside executor
//!
//! ## What the pipeline does NOT do
//!
//! * **Policy / RLS / hooks** — access control lives in function code, not here.
//!   The function calling `ctx.db` is already the policy layer.
//! * **Event emission** — use `ctx.queue.push()` from function code for side-effects.
//! * **File URL transforms** — file serving is a function responsibility.
//! * **Write `trace_requests`** — that table is the gateway's responsibility.

use axum::http::HeaderMap;
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
    engine::{auth_context::AuthContext, error::EngineError},
    executor::{self, MutationContext},
    router::DbRouter,
    state::AppState,
    transform::TransformEngine,
};

// ── Public output types ────────────────────────────────────────────────────────

pub struct QueryMeta {
    pub strategy: &'static str,
    pub complexity: u64,
    pub elapsed_ms: u128,
    pub rows: usize,
    pub compiled_sql: String,
    pub request_id: String,
}

// ── Pipeline ──────────────────────────────────────────────────────────────────

pub struct QueryPipeline<'a> {
    state: &'a AppState,
}

impl<'a> QueryPipeline<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    pub async fn run(
        &self,
        headers: &HeaderMap,
        req: QueryRequest,
    ) -> Result<(serde_json::Value, QueryMeta), EngineError> {
        // ── Step 1: Auth context ──────────────────────────────────────────────
        let auth = AuthContext::from_headers(headers).map_err(EngineError::MissingField)?;

        let request_id = headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-")
            .to_string();

        // ── Step 2: Schema name ───────────────────────────────────────────────
        let schema = DbRouter::schema_name(&req.database)?;

        // ── Step 3: Guards ────────────────────────────────────────────────────
        let complexity = self.state.query_guard.check_complexity(&req)?;
        let _ = self.state.query_guard.check_depth(&req)?;

        // ── Step 4: Existence ─────────────────────────────────────────────────
        DbRouter::assert_exists(&self.state.pool, &schema).await?;
        DbRouter::assert_table_exists(&self.state.pool, &schema, &req.table).await?;

        // ── Step 5: Schema cache (column metadata + relationships) ────────────
        let sk = cache::schema_key(&schema, &req.table);
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
                let rels = load_all_relationships(&self.state.pool, &schema).await?;
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

        // ── Step 6: Compile (L2 plan cache for SELECT) ────────────────────────
        let opts = CompilerOptions {
            default_limit: self.state.default_query_limit,
            max_limit: self.state.max_query_limit,
            computed_cols,
            relationships,
        };

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

        // Treat policy as empty (no enforcement) — the function calling ctx.db is the policy.
        let no_policy = Default::default();

        let compile_result: CompileResult = if req.operation == "select" {
            let plan_key = cache::build_plan_key(&schema, &req, &no_policy);
            match self.state.cache.plan_cache.get(&plan_key) {
                Some(plan) => {
                    tracing::debug!("plan cache hit");
                    let params = cache::extract_select_params(
                        &req,
                        &no_policy,
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
                    let cr = QueryCompiler::compile(&req, &no_policy, &schema, &opts)?;
                    let (cache_sql, is_batched) = match &cr {
                        CompileResult::Single(cq) => (cq.sql.clone(), false),
                        CompileResult::Batched { root, .. } => (root.sql.clone(), true),
                    };
                    self.state.cache.plan_cache.insert(
                        plan_key,
                        QueryPlan { sql: cache_sql, has_file_cols: false, is_batched },
                    );
                    cr
                }
            }
        } else {
            QueryCompiler::compile(&req, &no_policy, &schema, &opts)?
        };

        let strategy: &'static str = match &compile_result {
            CompileResult::Single(_)      => "single",
            CompileResult::Batched { .. } => "batched",
        };
        let compiled_sql = match &compile_result {
            CompileResult::Single(cq)          => cq.sql.clone(),
            CompileResult::Batched { root, .. } => root.sql.clone(),
        };

        let span_id_owned = headers
            .get("x-span-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

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

        // ── Step 7: Execute + record mutations ────────────────────────────────
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
}
