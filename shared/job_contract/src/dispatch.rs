//! Inter-service dispatch traits.
//!
//! Each trait represents one "service boundary" that can be satisfied
//! either by an HTTP client (multi-process mode) or by a direct Rust call
//! into another crate's library (single-binary / server mode).
//!
//! # Implementations
//! - `HttpRuntimeDispatch` lives in `gateway/src/forward/http_impl.rs`
//! - `HttpApiDispatch` and `HttpQueueDispatch` live in `runtime/src/dispatch/`
//! - In-process impls live in `server/src/dispatch/`

use async_trait::async_trait;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ── ExecuteRequest / ExecuteResponse ─────────────────────────────────────────

/// Everything the Gateway passes to the Runtime when dispatching a function.
///
/// Mirrors the JSON body + headers that the HTTP path currently sends over the
/// wire, but collapsed into a single typed struct for in-process dispatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteRequest {
    pub function_id:    String,
    pub project_id:     Option<Uuid>,
    pub payload:        Value,
    /// Deterministic replay seed (omit for live invocations).
    pub execution_seed: Option<i64>,
    /// Forwarded `x-request-id` header.
    pub request_id:     Option<String>,
    /// Forwarded `x-parent-span-id` header.
    pub parent_span_id: Option<String>,
    /// Value of the `X-Function-Runtime` header, e.g. `"javascript"`.
    pub runtime_hint:   Option<String>,
    /// Value of the `X-User-Id` header (set when authenticated via JWT).
    pub user_id:        Option<String>,
    /// Serialised JWT claims forwarded as `X-JWT-Claims`.
    pub jwt_claims:     Option<Value>,
}

/// The runtime's response to an `ExecuteRequest`, with the HTTP status code
/// detached so callers can reconstruct a proper HTTP response without
/// re-parsing the JSON body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteResponse {
    /// Parsed JSON response body as returned by the function.
    pub body:       Value,
    /// HTTP status code the runtime chose (typically 200, 4xx, or 5xx).
    pub status:     u16,
    /// Wall-clock execution time in milliseconds (informational).
    pub duration_ms: u64,
}

// ── Traits ────────────────────────────────────────────────────────────────────

/// Gateway → Runtime boundary.
///
/// The gateway calls `execute` once per inbound HTTP request.  The runtime
/// may be on the same thread (server crate) or across the network (HTTP impl).
#[async_trait]
pub trait RuntimeDispatch: Send + Sync {
    async fn execute(&self, req: ExecuteRequest) -> Result<ExecuteResponse, String>;
}

/// Runtime → API boundary (bundle fetch, log write, secrets).
///
/// Used by the runtime's bundle resolver, trace emitter, and secrets client
/// to call back into the control-plane API without knowing how to reach it.
#[async_trait]
pub trait ApiDispatch: Send + Sync {
    /// Fetch the active deployment bundle for `function_id`.
    ///
    /// Returns the raw JSON object the API endpoint returns — either
    /// `{ url, runtime, ... }` (presigned S3/Minio path) or
    /// `{ code, runtime, ... }` (inline bundle fallback).
    async fn get_bundle(&self, function_id: &str) -> Result<Value, String>;

    /// Ship a structured log/trace entry to the API's log ingestion endpoint.
    async fn write_log(&self, entry: Value) -> Result<(), String>;

    /// Fetch decrypted secrets for `project_id` (or the default project if
    /// `None`).  Returns a plain `key → value` map.
    async fn get_secrets(
        &self,
        project_id: Option<Uuid>,
    ) -> Result<HashMap<String, String>, String>;
}

/// Runtime (inside user-function V8 op) → Queue boundary.
///
/// Called by `ctx.queue.push()` inside JS functions.  Accepts a function name
/// (resolved in API) and a JSON payload, and enqueues the job.
#[async_trait]
pub trait QueueDispatch: Send + Sync {
    async fn push_job(
        &self,
        function_id: &str,
        project_id:  Option<Uuid>,
        payload:     Value,
        delay_ms:    Option<u64>,
        idempotency_key: Option<String>,
    ) -> Result<(), String>;
}

// ── AgentDispatch ─────────────────────────────────────────────────────────────

/// Caller → Agent boundary.
///
/// Used by:
///   - The API handlers (`POST /agents/{name}/run`) to trigger an agent.
///   - The `op_agent_run` Deno op, so user JS functions can call `ctx.agent.run()`.
///
/// The in-process implementation (`InProcessAgentDispatch` in server) calls
/// `agent::run()` directly.
#[async_trait]
pub trait AgentDispatch: Send + Sync {
    async fn run(
        &self,
        name:       &str,
        input:      Value,
        request_id: &str,
        project_id: Uuid,
        secrets:    HashMap<String, String>,
    ) -> Result<Value, String>;
}
