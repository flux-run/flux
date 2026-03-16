//! TraceEmitter — single-responsibility span/log emitter for the execution pipeline.
//!
//! Constructed once per invocation with all fixed context (function_id, project_id,
//! request_id, parent_span_id, code_sha).  Each method fires a tokio::spawn so
//! callers are never blocked by log I/O.
//!
//! Delegates the actual log shipping to an `Arc<dyn ApiDispatch>` so it works
//! in both multi-process (HTTP) and single-binary (in-process) modes.
use std::sync::Arc;
use uuid::Uuid;
use serde_json::Value;
use job_contract::dispatch::ApiDispatch;
use crate::engine::executor::LogLine;

#[derive(Clone)]
pub struct TraceEmitter {
    api:            Arc<dyn ApiDispatch>,
    function_id:    Arc<String>,
    project_id:     Arc<String>,
    request_id:     Arc<Option<String>>,
    parent_span_id: Option<String>,
    /// Short fingerprint of the JS/WASM bundle being executed — set after bundle resolve.
    pub code_sha:   Option<String>,
}

impl TraceEmitter {
    pub fn new(
        api:            Arc<dyn ApiDispatch>,
        function_id:    String,
        request_id:     Option<String>,
        parent_span_id: Option<String>,
    ) -> Self {
        Self {
            api,
            function_id:   Arc::new(function_id),
            project_id:    Arc::new(std::env::var("FLUX_PROJECT").unwrap_or_else(|_| "default".to_string())),
            request_id:    Arc::new(request_id),
            parent_span_id,
            code_sha:      None,
        }
    }

    /// Emit a single fire-and-forget event span (debug/info level).
    pub fn post_event(&self, level: &'static str, message: String) {
        self.spawn_log(serde_json::json!({
            "source":      "runtime",
            "resource_id": &*self.function_id,
            "project_id": &*self.project_id,
            "level":       level,
            "message":     message,
            "request_id":  &*self.request_id,
            "span_id":     Uuid::new_v4().to_string(),
            "span_type":   "event",
        }));
    }

    /// Emit an execution lifecycle span: start / completed / error.
    ///
    /// - `state` — "started" | "completed" | "error"
    /// - `span_type` — "start" | "end"
    /// - `duration_ms` — only set on end/error spans
    pub fn post_lifecycle(
        &self,
        level:       &'static str,
        message:     String,
        span_type:   &'static str,
        state:       &'static str,
        duration_ms: Option<u64>,
    ) {
        let parent_span_id = self.parent_span_id.clone();
        let code_sha       = self.code_sha.clone();
        self.spawn_log(serde_json::json!({
            "source":           "runtime",
            "resource_id":      &*self.function_id,
            "project_id":       &*self.project_id,
            "level":            level,
            "message":          message,
            "request_id":       &*self.request_id,
            "span_id":          Uuid::new_v4().to_string(),
            "span_type":        span_type,
            "parent_span_id":   parent_span_id,
            "code_sha":         code_sha,
            "execution_state":  state,
            "duration_ms":      duration_ms,
        }));
    }

    /// Forward all user-emitted `ctx.log()` lines and the final execution_end span.
    ///
    /// Fire-and-forget in a tokio::spawn. Whole batch sent after the response is
    /// already on the wire, so log I/O never adds to gateway-visible latency.
    pub fn emit_logs(&self, logs: Vec<LogLine>, duration_ms: u64) {
        let api           = Arc::clone(&self.api);
        let function_id   = Arc::clone(&self.function_id);
        let project_id    = Arc::clone(&self.project_id);
        let request_id    = Arc::clone(&self.request_id);
        let parent_span_id = self.parent_span_id.clone();
        let code_sha      = self.code_sha.clone();

        tokio::spawn(async move {
            for log in logs {
                let span_type = log.span_type.as_deref().unwrap_or("event");
                let source    = log.source   .as_deref().unwrap_or("function");
                let span_id   = log.span_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
                let payload: Value = serde_json::json!({
                    "source":           source,
                    "resource_id":      &*function_id,
                    "project_id":       &*project_id,
                    "level":            log.level,
                    "message":          log.message,
                    "request_id":       &*request_id,
                    "span_id":          span_id,
                    "span_type":        span_type,
                    "parent_span_id":   &parent_span_id,
                    "code_sha":         &code_sha,
                    "duration_ms":      log.duration_ms,
                    "tool_name":        log.tool_name,
                    "execution_state":  log.execution_state,
                });
                let _ = api.write_log(payload).await;
            }

            // Final lifecycle span — marks the execution tree as complete.
            let end_payload: Value = serde_json::json!({
                "source":           "runtime",
                "resource_id":      &*function_id,
                "project_id":       &*project_id,
                "level":            "info",
                "message":          format!("execution_end ({}ms)", duration_ms),
                "request_id":       &*request_id,
                "span_id":          Uuid::new_v4().to_string(),
                "span_type":        "end",
                "parent_span_id":   &parent_span_id,
                "code_sha":         &code_sha,
                "execution_state":  "completed",
                "duration_ms":      duration_ms,
            });
            let _ = api.write_log(end_payload).await;
        });
    }

    // ── private ───────────────────────────────────────────────────────────

    fn spawn_log(&self, payload: Value) {
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            let _ = api.write_log(payload).await;
        });
    }
}
