use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

/// Inbound request from the Gateway — no tenant concept.
#[derive(Deserialize)]
pub struct ExecuteRequest {
    pub function_id:    String,
    pub project_id:     Option<Uuid>,
    pub payload:        Value,
    /// Deterministic randomness seed.
    /// When provided (replay path) Math.random / crypto.randomUUID / nanoid are
    /// seeded so the same seed produces identical values.
    /// Omit for live invocations — the runtime generates a fresh seed.
    pub execution_seed: Option<i64>,
}

/// Per-request context threaded through every layer of the execute pipeline.
/// Derived once from the HTTP request; never mutated after construction.
#[derive(Clone)]
pub struct InvocationCtx {
    pub function_id:    String,
    pub project_id:     Option<Uuid>,
    pub payload:        Value,
    pub execution_seed: i64,
    /// Forwarded `x-request-id` header — used to correlate spans across services.
    pub request_id:     Option<String>,
    /// Forwarded `x-parent-span-id` header — links to the Gateway span.
    pub parent_span_id: Option<String>,
}
