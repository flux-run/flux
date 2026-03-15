//! ExecutionRunner — single-responsibility execution layer.
//!
//! Shared by all three bundle-resolution paths (warm WASM, warm Deno, cold).
//!
//! ## Execution pipeline
//!
//! 1. **JSON Schema validation** — if the function has an `input` schema configured
//!    (stored in `SchemaCache`), validate `payload` before touching the isolate.
//!    Returns HTTP 400 with violation details on failure.
//!
//! 2. **`execution_start` span** — fire-and-forget via `TraceEmitter::post_lifecycle`
//!    so the span appears in `flux trace` even if the function panics.
//!
//! 3. **Deno or WASM dispatch** — route to `IsolatePool::execute` (JS) or
//!    `WasmPool::execute` (WASM) based on the bundle type resolved by `BundleResolver`.
//!
//! 4. **Log collection + `execution_end` span** — after the isolate returns,
//!    `TraceEmitter::emit_logs` ships all `ctx.log()` lines and the final `execution_end`
//!    span in a `tokio::spawn` (fire-and-forget). Log I/O never adds to gateway-visible
//!    latency.
//!
//! ## Error handling
//!
//! Execution errors are parsed for a `{ code, message }` JSON envelope (Deno throws these).
//! `INPUT_VALIDATION_ERROR` maps to HTTP 400; all others map to HTTP 500. The error span
//! is emitted before returning so the gateway already has the response on the wire while
//! the span write is in flight.
/// ExecutionRunner — single-responsibility execution layer.
///
/// Shared by all three bundle-resolution paths (warm WASM, warm Deno, cold).
/// Previously each path had identical copy-pasted code for:
///   validate_schema → dispatch to pool → emit trace spans
///
/// Now it lives here exactly once.
use std::collections::HashMap;
use std::time::Instant;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::Value;

use crate::engine::executor::{DbContext, ExecutionResult, PoolDispatchers, QueueContext};
use crate::engine::pool::IsolatePool;
use crate::engine::wasm_pool::WasmPool;
use crate::execute::bundle::ResolvedBundle;
use crate::execute::types::InvocationCtx;
use crate::schema::cache::SchemaCache;
use crate::schema::validator;
use crate::trace::emitter::TraceEmitter;

pub struct ExecutionRunner<'a> {
    pub isolate_pool:     &'a IsolatePool,
    pub wasm_pool:        &'a WasmPool,
    pub schema_cache:     &'a SchemaCache,
    pub http_client:      &'a reqwest::Client,
    pub wasm_http_hosts:  Vec<String>,
    /// Postgres schema name for this project — forwarded into ctx.db.query().
    pub database:         String,
    pub dispatchers:      &'a PoolDispatchers,
}

impl<'a> ExecutionRunner<'a> {
    /// Validate → execute → emit spans → return (status_code, json_body).
    ///
    /// `tracer` must already have `code_sha` set before this call.
    /// The caller wraps this in `.into_response()` for HTTP handlers, or
    /// converts to `ExecuteResponse` for in-process dispatch.
    pub async fn run(
        &self,
        bundle:  ResolvedBundle,
        secrets: HashMap<String, String>,
        ctx:     &InvocationCtx,
        tracer:  &TraceEmitter,
        start:   Instant,
    ) -> (StatusCode, Value) {
        // ── JSON Schema validation ────────────────────────────────────────
        if let Some(schema) = self.schema_cache.get(&ctx.function_id) {
            if let Some(input_schema) = &schema.input {
                if let Err(violations) = validator::validate_input(input_schema, &ctx.payload) {
                    return (StatusCode::BAD_REQUEST, serde_json::json!({
                        "error":      "INPUT_VALIDATION_ERROR",
                        "message":    "Payload does not match the function's input schema",
                        "violations": violations,
                    }));
                }
            }
        }

        // ── execution_start span ──────────────────────────────────────────
        tracer.post_lifecycle("info", "execution_start".into(), "start", "started", None);

        // ── dispatch to the right engine ──────────────────────────────────
        let (result, duration_ms) = match bundle {
            ResolvedBundle::Deno { code } => {
                self.run_deno(code, secrets, ctx, tracer, start).await
            }
            ResolvedBundle::Wasm { bytes } => {
                self.run_wasm(bytes, secrets, ctx, tracer, start).await
            }
        };

        let (execution, duration_ms) = match result {
            Ok(r)  => (r, duration_ms),
            Err(e) => return e,
        };

        // ── emit ctx.log() lines + execution_end span (fire-and-forget) ──
        tracer.emit_logs(execution.logs, duration_ms);

        (StatusCode::OK, serde_json::json!({
            "result":      execution.output,
            "duration_ms": duration_ms,
        }))
    }

    /// Convenience wrapper for axum HTTP handlers.
    pub async fn run_response(
        &self,
        bundle:  ResolvedBundle,
        secrets: HashMap<String, String>,
        ctx:     &InvocationCtx,
        tracer:  &TraceEmitter,
        start:   Instant,
    ) -> Response {
        let (status, body) = self.run(bundle, secrets, ctx, tracer, start).await;
        (status, axum::Json(body)).into_response()
    }

    // ── private ───────────────────────────────────────────────────────────

    async fn run_deno(
        &self,
        code:    String,
        secrets: HashMap<String, String>,
        ctx:     &InvocationCtx,
        tracer:  &TraceEmitter,
        start:   Instant,
    ) -> (Result<ExecutionResult, (StatusCode, Value)>, u64) {
        let queue_ctx = QueueContext {};
        let db_ctx = DbContext {
            database: self.database.clone(),
        };
        let result = self.isolate_pool.execute(
            code,
            secrets,
            ctx.payload.clone(),
            ctx.execution_seed,
            queue_ctx,
            db_ctx,
            Some(ctx.function_id.clone()),
        ).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        let execution = match result {
            Ok(r) => r,
            Err(e) => {
                return (Err(self.execution_error(e, duration_ms, tracer)), duration_ms);
            }
        };
        (Ok(execution), duration_ms)
    }

    async fn run_wasm(
        &self,
        bytes:   Vec<u8>,
        secrets: HashMap<String, String>,
        ctx:     &InvocationCtx,
        tracer:  &TraceEmitter,
        start:   Instant,
    ) -> (Result<ExecutionResult, (StatusCode, Value)>, u64) {
        let result = self.wasm_pool.execute(
            ctx.function_id.clone(),
            bytes,
            secrets,
            ctx.payload.clone(),
            None,
            self.wasm_http_hosts.clone(),
            self.http_client.clone(),
            self.database.clone(),
            self.dispatchers.clone(),
        ).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        let execution = match result {
            Ok(r) => r,
            Err(e) => {
                return (Err(self.execution_error(e, duration_ms, tracer)), duration_ms);
            }
        };
        (Ok(execution), duration_ms)
    }

    fn execution_error(&self, error: String, duration_ms: u64, tracer: &TraceEmitter) -> (StatusCode, Value) {
        // Pool saturation — map to 503 before JSON parsing so callers get a
        // proper Retry-After hint rather than a generic 500.
        if error.starts_with("pool_saturated") {
            tracer.post_lifecycle(
                "warn",
                "pool_saturated: all workers at capacity".into(),
                "end", "error",
                Some(duration_ms),
            );
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                serde_json::json!({
                    "error":       "pool_saturated",
                    "message":     "All function workers are at capacity — retry in a moment",
                    "retry_after": 2,
                }),
            );
        }

        let (err_code, message) = if let Ok(parsed) = serde_json::from_str::<Value>(&error) {
            let code = parsed.get("code")   .and_then(|c| c.as_str()).unwrap_or("FunctionExecutionError").to_string();
            let msg  = parsed.get("message").and_then(|m| m.as_str()).unwrap_or(&error).to_string();
            (code, msg)
        } else {
            ("FunctionExecutionError".to_string(), error)
        };

        tracer.post_lifecycle(
            "error",
            format!("execution_error: {}: {}", err_code, message),
            "end", "error",
            Some(duration_ms),
        );

        let status = if err_code == "INPUT_VALIDATION_ERROR" {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        (status, serde_json::json!({ "error": err_code, "message": message }))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Derive the Postgres schema name for a project.
///
/// Single-tenant: always returns an empty string.
pub fn project_schema_name() -> String {
    String::new()
}

/// Return the list of hosts WASM functions may call via `flux.http_fetch`.
///
/// Configured via `WASM_HTTP_ALLOWED_HOSTS`:
/// - Not set / empty  → deny all (safe default)
/// - `"*"`            → allow all (dev/internal only)
/// - Comma-separated  → e.g. `"api.example.com,hooks.slack.com"`
pub fn allowed_wasm_http_hosts() -> Vec<String> {
    std::env::var("WASM_HTTP_ALLOWED_HOSTS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use async_trait::async_trait;
    use axum::http::StatusCode;
    use serde_json::{json, Value};
    use uuid::Uuid;

    use job_contract::dispatch::{ApiDispatch, DataEngineDispatch, QueueDispatch, ResolvedFunction};
    use crate::engine::executor::PoolDispatchers;
    use crate::engine::pool::IsolatePool;
    use crate::engine::wasm_pool::WasmPool;
    use crate::execute::bundle::ResolvedBundle;
    use crate::execute::types::InvocationCtx;
    use crate::schema::cache::{FunctionSchema, SchemaCache};
    use crate::trace::emitter::TraceEmitter;

    // ── Test doubles ─────────────────────────────────────────────────────────

    /// No-op ApiDispatch — fire-and-forget spans are discarded.
    struct NullApi;

    #[async_trait]
    impl ApiDispatch for NullApi {
        async fn get_bundle(&self, _: &str) -> Result<Value, String> {
            Err("not used in runner tests".into())
        }
        async fn write_log(&self, _: Value) -> Result<(), String> {
            Ok(())
        }
        async fn write_network_call(&self, _: Value) -> Result<(), String> {
            Ok(())
        }
        async fn get_secrets(&self) -> Result<HashMap<String, String>, String> {
            Ok(HashMap::new())
        }
        async fn resolve_function(&self, _: &str) -> Result<ResolvedFunction, String> {
            Err("not used in runner tests".into())
        }
    }

    struct NullQueueDispatch;
    #[async_trait]
    impl QueueDispatch for NullQueueDispatch {
        async fn push_job(&self, _: &str, _: Value, _: Option<u64>, _: Option<String>) -> Result<(), String> {
            Err("not used in runner tests".into())
        }
    }

    struct NullDataEngineDispatch;
    #[async_trait]
    impl DataEngineDispatch for NullDataEngineDispatch {
        async fn execute_sql(&self, _: String, _: Vec<Value>, _: String, _: String) -> Result<Value, String> {
            Err("not used in runner tests".into())
        }
    }

    fn test_dispatchers() -> PoolDispatchers {
        PoolDispatchers {
            api:         Arc::new(NullApi),
            queue:       Arc::new(NullQueueDispatch),
            data_engine: Arc::new(NullDataEngineDispatch),
            runtime:     Arc::new(std::sync::OnceLock::new()),
        }
    }

    fn null_tracer() -> TraceEmitter {
        TraceEmitter::new(Arc::new(NullApi), "test_fn".into(), None, None)
    }

    fn ctx(function_id: &str) -> InvocationCtx {
        InvocationCtx {
            function_id:    function_id.to_string(),
            payload:        json!({"name": "flux"}),
            execution_seed: 1,
            request_id:     None,
            parent_span_id: None,
        }
    }

    fn runner<'a>(
        isolate_pool: &'a IsolatePool,
        wasm_pool:    &'a WasmPool,
        schema_cache: &'a SchemaCache,
        http_client:  &'a reqwest::Client,
        dispatchers:  &'a PoolDispatchers,
    ) -> ExecutionRunner<'a> {
        ExecutionRunner {
            isolate_pool,
            wasm_pool,
            schema_cache,
            http_client,
            wasm_http_hosts: vec![],
            database:        String::new(),
            dispatchers,
        }
    }

    // ── project_schema_name ───────────────────────────────────────────────────

    #[test]
    fn project_schema_name_returns_empty() {
        assert_eq!(project_schema_name(), "");
    }

    // ── allowed_wasm_http_hosts ───────────────────────────────────────────────

    #[test]
    fn wasm_http_hosts_unset_returns_empty() {
        // Guard against env bleed from other tests.
        unsafe { std::env::remove_var("WASM_HTTP_ALLOWED_HOSTS"); }
        assert!(allowed_wasm_http_hosts().is_empty());
    }

    #[test]
    fn wasm_http_hosts_parses_csv() {
        unsafe { std::env::set_var("WASM_HTTP_ALLOWED_HOSTS", "api.example.com, hooks.slack.com , "); }
        let hosts = allowed_wasm_http_hosts();
        assert_eq!(hosts, vec!["api.example.com", "hooks.slack.com"]);
        unsafe { std::env::remove_var("WASM_HTTP_ALLOWED_HOSTS"); }
    }

    #[test]
    fn wasm_http_hosts_wildcard_passthrough() {
        unsafe { std::env::set_var("WASM_HTTP_ALLOWED_HOSTS", "*"); }
        let hosts = allowed_wasm_http_hosts();
        assert_eq!(hosts, vec!["*"]);
        unsafe { std::env::remove_var("WASM_HTTP_ALLOWED_HOSTS"); }
    }

    // ── Schema validation (run() short-circuits before pool is touched) ───────

    #[tokio::test]
    async fn schema_validation_failure_returns_400() {
        let schema_cache = SchemaCache::new(10);
        schema_cache.insert("validate_fn".into(), FunctionSchema {
            input: Some(json!({
                "type": "object",
                "required": ["user_id"],
                "properties": { "user_id": { "type": "string" } }
            })),
            output: None,
        });

        let dispatchers = test_dispatchers();
        let isolate_pool = IsolatePool::new(1, 5, dispatchers.clone());
        let wasm_pool    = WasmPool::new(1, 1_000_000, 5);
        let client = reqwest::Client::new();
        let r = runner(&isolate_pool, &wasm_pool, &schema_cache, &client, &dispatchers);

        let mut c = ctx("validate_fn");
        c.payload = json!({"wrong_key": 123}); // missing required "user_id"

        let (status, body) = r.run(
            ResolvedBundle::Deno { code: "".into() },
            HashMap::new(),
            &c,
            &null_tracer(),
            Instant::now(),
        ).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "INPUT_VALIDATION_ERROR");
        assert!(body["violations"].is_array());
    }

    #[tokio::test]
    async fn no_schema_skips_validation() {
        // Empty cache → validation step is bypassed; pool will error with bad code,
        // but the point is we get a 500 (execution error), not a 400 (schema error).
        let schema_cache = SchemaCache::new(10);
        let dispatchers = test_dispatchers();
        let isolate_pool = IsolatePool::new(1, 5, dispatchers.clone());
        let wasm_pool    = WasmPool::new(1, 1_000_000, 5);
        let client = reqwest::Client::new();
        let r = runner(&isolate_pool, &wasm_pool, &schema_cache, &client, &dispatchers);

        let (status, _body) = r.run(
            ResolvedBundle::Deno { code: "throw new Error('boom')".into() },
            HashMap::new(),
            &ctx("no_schema_fn"),
            &null_tracer(),
            Instant::now(),
        ).await;

        // Not 400 — schema wasn't the problem.
        assert_ne!(status, StatusCode::BAD_REQUEST);
    }

    // ── execution_error: JSON error envelope parsing ──────────────────────────

    #[tokio::test]
    async fn json_error_code_extracted_from_pool_error() {
        // Deno throws structured errors as JSON strings, e.g.:
        //   {"code":"NOT_FOUND","message":"user not found"}
        // The runner must parse code/message and preserve them.
        let schema_cache = SchemaCache::new(10);
        let dispatchers = test_dispatchers();
        let isolate_pool = IsolatePool::new(1, 5, dispatchers.clone());
        let wasm_pool    = WasmPool::new(1, 1_000_000, 5);
        let client = reqwest::Client::new();
        let r = runner(&isolate_pool, &wasm_pool, &schema_cache, &client, &dispatchers);

        // Code that throws a structured JSON error.
        let code = r#"__flux_fn = async (ctx) => {
            const err = {code: "NOT_FOUND", message: "user not found"};
            throw new Error(JSON.stringify(err));
        };"#.to_string();

        let (status, body) = r.run(
            ResolvedBundle::Deno { code },
            HashMap::new(),
            &ctx("json_error_fn"),
            &null_tracer(),
            Instant::now(),
        ).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        // The error field should be present.
        assert!(body.get("error").is_some() || body.get("message").is_some(),
            "expected error body, got: {}", body);
    }

    #[tokio::test]
    async fn input_validation_error_code_in_pool_error_returns_400() {
        // If the pool itself returns INPUT_VALIDATION_ERROR (unlikely but possible),
        // the runner must map it to 400.
        let schema_cache = SchemaCache::new(10);
        let dispatchers = test_dispatchers();
        let isolate_pool = IsolatePool::new(1, 5, dispatchers.clone());
        let wasm_pool    = WasmPool::new(1, 1_000_000, 5);
        let client = reqwest::Client::new();
        let r = runner(&isolate_pool, &wasm_pool, &schema_cache, &client, &dispatchers);

        // Code that throws INPUT_VALIDATION_ERROR — runner must map it to 400.
        let code = r#"__flux_fn = async (ctx) => {
            const err = {code: "INPUT_VALIDATION_ERROR", message: "bad input from user code"};
            throw new Error(JSON.stringify(err));
        };"#.to_string();

        let (status, body) = r.run(
            ResolvedBundle::Deno { code },
            HashMap::new(),
            &ctx("input_val_fn"),
            &null_tracer(),
            Instant::now(),
        ).await;

        assert_eq!(status, StatusCode::BAD_REQUEST,
            "INPUT_VALIDATION_ERROR must map to 400, body: {}", body);
        assert_eq!(body["error"], "INPUT_VALIDATION_ERROR");
    }

    // ── Successful Deno execution round-trip ──────────────────────────────────

    #[tokio::test]
    async fn deno_success_returns_200_with_result() {
        let schema_cache = SchemaCache::new(10);
        let dispatchers = test_dispatchers();
        let isolate_pool = IsolatePool::new(1, 10, dispatchers.clone());
        let wasm_pool    = WasmPool::new(1, 1_000_000, 10);
        let client = reqwest::Client::new();
        let r = runner(&isolate_pool, &wasm_pool, &schema_cache, &client, &dispatchers);

        let code = r#"__flux_fn = async (ctx) => ({ ok: true, echo: ctx.payload.name });"#.to_string();

        let (status, body) = r.run(
            ResolvedBundle::Deno { code },
            HashMap::new(),
            &ctx("echo_fn"),
            &null_tracer(),
            Instant::now(),
        ).await;

        assert_eq!(status, StatusCode::OK, "body: {}", body);
        assert_eq!(body["result"]["ok"], true);
        assert!(body["duration_ms"].is_number());
    }

    // ── duration_ms is present on success ─────────────────────────────────────

    #[tokio::test]
    async fn successful_run_includes_duration_ms() {
        let schema_cache = SchemaCache::new(10);
        let dispatchers = test_dispatchers();
        let isolate_pool = IsolatePool::new(1, 10, dispatchers.clone());
        let wasm_pool    = WasmPool::new(1, 1_000_000, 10);
        let client = reqwest::Client::new();
        let r = runner(&isolate_pool, &wasm_pool, &schema_cache, &client, &dispatchers);

        let (status, body) = r.run(
            ResolvedBundle::Deno { code: r#"__flux_fn = async (ctx) => 42;"#.into() },
            HashMap::new(),
            &ctx("duration_fn"),
            &null_tracer(),
            Instant::now(),
        ).await;

        assert_eq!(status, StatusCode::OK, "body: {}", body);
        assert!(body["duration_ms"].as_u64().is_some(), "duration_ms must be a number");
    }

    // ── run_response wraps into HTTP response ─────────────────────────────────

    #[tokio::test]
    async fn run_response_returns_axum_response() {
        use axum::body::to_bytes;

        let schema_cache = SchemaCache::new(10);
        let dispatchers = test_dispatchers();
        let isolate_pool = IsolatePool::new(1, 10, dispatchers.clone());
        let wasm_pool    = WasmPool::new(1, 1_000_000, 10);
        let client = reqwest::Client::new();
        let r = runner(&isolate_pool, &wasm_pool, &schema_cache, &client, &dispatchers);

        let resp = r.run_response(
            ResolvedBundle::Deno { code: r#"__flux_fn = async (ctx) => ({ v: 1 });"#.into() },
            HashMap::new(),
            &ctx("wrap_fn"),
            &null_tracer(),
            Instant::now(),
        ).await;

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["result"]["v"], 1);
    }
}
