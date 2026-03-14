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

use crate::engine::executor::{DbContext, ExecutionResult, QueueContext};
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
    /// Queue service base URL — forwarded into ctx.queue.push() via QueueOpState.
    pub queue_url:        &'a str,
    /// API service base URL — used by ctx.queue.push() to resolve function names.
    pub api_url:          &'a str,
    /// Internal service token threaded to queue + API calls from user functions.
    pub service_token:    &'a str,
    /// Data-engine base URL — forwarded into ctx.db.query() via task JSON.
    pub data_engine_url:  &'a str,
    /// Postgres schema name for this project — forwarded into ctx.db.query().
    pub database:         String,
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
        let queue_ctx = QueueContext {
            queue_url:     self.queue_url.to_string(),
            api_url:       self.api_url.to_string(),
            service_token: self.service_token.to_string(),
            project_id:    ctx.project_id,
            client:        self.http_client.clone(),
        };
        let db_ctx = DbContext {
            data_engine_url: self.data_engine_url.to_string(),
            service_token:   self.service_token.to_string(),
            database:        self.database.clone(),
            client:          self.http_client.clone(),
        };
        let result = self.isolate_pool.execute(
            code,
            secrets,
            ctx.payload.clone(),
            ctx.execution_seed,
            queue_ctx,
            db_ctx,
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
/// Projects get their own schema in the form `project_<uuid_no_hyphens>`.
/// Returns an empty string if no project_id is available.
pub fn project_schema_name(project_id: Option<uuid::Uuid>) -> String {
    match project_id {
        Some(id) => format!("project_{}", id.as_simple()),
        None => String::new(),
    }
}

/// Return the list of hosts WASM functions may call via `fluxbase.http_fetch`.
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
