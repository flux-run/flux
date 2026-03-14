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
    /// Returns the raw JSON object the API endpoint returns:
    /// `{ code, runtime, deployment_id, input_schema, output_schema }`.
    async fn get_bundle(&self, function_id: &str) -> Result<Value, String>;

    /// Ship a structured log/trace entry to the API's log ingestion endpoint.
    async fn write_log(&self, entry: Value) -> Result<(), String>;

    /// Fetch decrypted secrets. Returns a plain `key → value` map.
    async fn get_secrets(&self) -> Result<HashMap<String, String>, String>;

    /// Resolve a function name (or UUID string) to its function_id.
    ///
    /// Used by `ctx.queue.push()` in V8 ops to look up a function before
    /// enqueuing a job.
    async fn resolve_function(
        &self,
        name: &str,
    ) -> Result<ResolvedFunction, String>;
}

/// Result of resolving a function name to its ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedFunction {
    pub function_id: Uuid,
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
        payload:     Value,
        delay_seconds: Option<u64>,
        idempotency_key: Option<String>,
    ) -> Result<(), String>;
}

/// Runtime (V8 op) → Data-Engine boundary.
///
/// Called by `ctx.db.query()` inside JS/WASM functions to execute raw SQL
/// against the project database. In-process dispatch replaces the HTTP POST
/// to `/db/sql`.
#[async_trait]
pub trait DataEngineDispatch: Send + Sync {
    async fn execute_sql(
        &self,
        sql:        String,
        params:     Vec<Value>,
        database:   String,
        request_id: String,
    ) -> Result<Value, String>;
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── ExecuteRequest serde ──────────────────────────────────────────────

    #[test]
    fn execute_request_roundtrip() {
        let req = ExecuteRequest {
            function_id:    "my-fn".to_string(),
            payload:        json!({"key": "value"}),
            execution_seed: Some(42),
            request_id:     Some("req-123".to_string()),
            parent_span_id: None,
            runtime_hint:   Some("javascript".to_string()),
            user_id:        None,
            jwt_claims:     None,
        };
        let json_str = serde_json::to_string(&req).unwrap();
        let back: ExecuteRequest = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back.function_id, "my-fn");
        assert_eq!(back.execution_seed, Some(42));
        assert_eq!(back.request_id, Some("req-123".to_string()));
    }

    #[test]
    fn execute_request_minimal_fields() {
        let json_str = r#"{"function_id":"fn","payload":null}"#;
        let req: ExecuteRequest = serde_json::from_str(json_str).unwrap();
        assert_eq!(req.function_id, "fn");
        assert!(req.execution_seed.is_none());
    }

    #[test]
    fn execute_request_clone() {
        let req = ExecuteRequest {
            function_id:    "fn".to_string(),
            payload:        json!({}),
            execution_seed: None,
            request_id:     None,
            parent_span_id: None,
            runtime_hint:   None,
            user_id:        None,
            jwt_claims:     None,
        };
        let cloned = req.clone();
        assert_eq!(cloned.function_id, req.function_id);
    }

    // ── ExecuteResponse serde ─────────────────────────────────────────────

    #[test]
    fn execute_response_roundtrip() {
        let resp = ExecuteResponse {
            body:        json!({"result": "ok"}),
            status:      200,
            duration_ms: 42,
        };
        let json_str = serde_json::to_string(&resp).unwrap();
        let back: ExecuteResponse = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back.status,      200);
        assert_eq!(back.duration_ms, 42);
        assert_eq!(back.body["result"], "ok");
    }

    #[test]
    fn execute_response_error_status() {
        let resp = ExecuteResponse {
            body:        json!({"error": "not found"}),
            status:      404,
            duration_ms: 5,
        };
        assert_eq!(resp.status, 404);
    }

    // ── Trait object safety ───────────────────────────────────────────────

    #[test]
    fn runtime_dispatch_is_object_safe() {
        fn _check(_: &dyn RuntimeDispatch) {}
    }

    #[test]
    fn api_dispatch_is_object_safe() {
        fn _check(_: &dyn ApiDispatch) {}
    }

    #[test]
    fn queue_dispatch_is_object_safe() {
        fn _check(_: &dyn QueueDispatch) {}
    }

    #[test]
    fn data_engine_dispatch_is_object_safe() {
        fn _check(_: &dyn DataEngineDispatch) {}
    }

    #[test]
    fn resolved_function_roundtrip() {
        let rf = ResolvedFunction { function_id: Uuid::new_v4() };
        let json_str = serde_json::to_string(&rf).unwrap();
        let back: ResolvedFunction = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back.function_id, rf.function_id);
    }
}
