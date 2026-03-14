//! Queue span emitter — single responsibility: emit job lifecycle spans to platform_logs.
//!
//! Mirrors `runtime::trace::TraceEmitter` but scoped to queue worker lifecycle events.
//! Depends on `ApiDispatch` so it works in both HTTP (multi-process) and in-process modes.
//!
//! # Design
//!
//! Each call to [`QueueSpanEmitter::emit`] fires a `tokio::spawn` and returns immediately.
//! The spawned task calls `ApiDispatch::write_log`, which writes to `flux.platform_logs`.
//! This fire-and-forget pattern means log I/O never delays job execution or the poller loop.
//!
//! # DIP rationale
//!
//! The emitter depends on the `ApiDispatch` trait, not on any concrete HTTP client or DB
//! pool. In multi-process deployments the queue binary supplies `HttpApiDispatch`; in the
//! monolithic server the same code receives `InProcessApiDispatch` — zero code change.

use std::sync::Arc;
use uuid::Uuid;
use job_contract::dispatch::ApiDispatch;

/// Emits job lifecycle spans to `flux.platform_logs` via `ApiDispatch::write_log`.
///
/// Each span is fire-and-forget (`tokio::spawn`) — never blocks the job execution path.
///
/// Construct one instance per job execution; all fixed fields (`job_id`, `function_id`,
/// `project_id`, `request_id`) are captured at construction time.
#[derive(Clone)]
pub struct QueueSpanEmitter {
    api:         Arc<dyn ApiDispatch>,
    job_id:      Uuid,
    function_id: Uuid,
    request_id:  String,
}

impl QueueSpanEmitter {
    pub fn new(
        api:         Arc<dyn ApiDispatch>,
        job_id:      Uuid,
        function_id: Uuid,
        request_id:  String,
    ) -> Self {
        Self { api, job_id, function_id, request_id }
    }

    /// Emit a job lifecycle span (started, completed, failed, retried, dead_lettered).
    ///
    /// - `level`     — `"info"` | `"warn"` | `"error"`
    /// - `message`   — human-readable description of the event
    /// - `span_type` — `"start"` | `"end"` | `"event"` | `"error"`
    ///
    /// A fresh `span_id` (UUIDv4) is generated for every call so each span is individually
    /// addressable in `flux trace` queries while still being grouped by `request_id`.
    pub fn emit(&self, level: &'static str, message: String, span_type: &'static str) {
        let api        = Arc::clone(&self.api);
        let job_id     = self.job_id;
        let fn_id      = self.function_id.to_string();
        let request_id = self.request_id.clone();

        tokio::spawn(async move {
            let _ = api.write_log(serde_json::json!({
                "source":      "queue",
                "resource_id": fn_id,
                "level":       level,
                "message":     message,
                "request_id":  request_id,
                "span_id":     Uuid::new_v4().to_string(),
                "span_type":   span_type,
                "metadata":    { "job_id": job_id.to_string() },
            })).await;
        });
    }
}
