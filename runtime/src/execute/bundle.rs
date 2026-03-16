/// BundleResolver — single-responsibility bundle resolution for the execute pipeline.
///
/// Encapsulates the two-path resolution strategy:
///   1. Warm Deno path  — cached code in BundleCache (zero network calls)
///   2. Cold path       — fetch from control plane → inline code → populate cache
///
/// Separated from the HTTP handler and execution runner so each can be
/// tested and reasoned about independently.
use axum::http::StatusCode;
use axum::Json;
use axum::response::{IntoResponse, Response};
use serde_json::Value;

use job_contract::dispatch::ApiDispatch;
use crate::bundle::cache::BundleCache;
use crate::schema::cache::{SchemaCache, FunctionSchema};
use crate::trace::emitter::TraceEmitter;

/// The resolved bundle, ready for execution.
pub enum ResolvedBundle {
    /// JavaScript/TypeScript code (Deno V8 path).
    Deno { code: String },
    /// Compiled WASM bytes.
    Wasm { bytes: Vec<u8> },
}

/// Short fingerprint of a bundle — used as `code_sha` in trace spans for replay.
/// Not cryptographic — just sufficient to identify a unique bundle version.
pub fn bundle_sha(data: &[u8]) -> String {
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// State slice used by the resolver (all immutably shared via Arc in AppState).
pub struct BundleResolver<'a> {
    pub bundle_cache:  &'a BundleCache,
    pub schema_cache:  &'a SchemaCache,
    pub http_client:   &'a reqwest::Client,
    /// Control-plane dispatch — used for the bundle metadata fetch.
    pub api:           &'a dyn ApiDispatch,
}

impl<'a> BundleResolver<'a> {
    /// Attempt the warm Deno path.
    /// Returns `Some(code)` on a cache hit, `None` on a miss.
    pub fn warm_deno(&self, function_id: &str) -> Option<String> {
        self.bundle_cache.get_by_function(function_id)
    }

    /// Cold path: fetch bundle from the control-plane `/internal/bundle` endpoint,
    /// populate caches, and cache any schema data included in the response.
    ///
    /// Returns `Ok(ResolvedBundle)` on success, or an `Err(Response)` with the
    /// appropriate HTTP error status already encoded.
    pub async fn cold_fetch(
        &self,
        function_id: &str,
        _tracer:     &TraceEmitter,
    ) -> Result<ResolvedBundle, Response> {
        // Fetch bundle metadata via the ApiDispatch trait (HTTP in multi-process
        // mode, direct call in single-binary mode).  The dispatch impl already
        // unwraps the outer `{ success, data }` envelope and returns the inner
        // data object.
        let data = self.api.get_bundle(function_id).await.map_err(|e| {
            if e.contains("HTTP 404") {
                not_found("no_bundle_found",
                    "No active deployment found for this function. Deploy it first.")
            } else {
                bad_gateway("BundleFetchError",
                    format!("Failed to reach API service: {}", e))
            }
        })?;

        // Cache schema if the control plane returned it alongside the bundle.
        self.cache_schema(function_id, &data);

        let deployment_id = data.get("deployment_id").and_then(|v| v.as_str()).map(|s| s.to_string());
        let code_opt      = data.get("code")         .and_then(|v| v.as_str()).map(|s| s.to_string());

        // ── Deno cold path ────────────────────────────────────────────────
        // Check deployment-level cache first.
        if let Some(d_id) = &deployment_id {
            if let Some(cached) = self.bundle_cache.get(d_id) {
                tracing::debug!(function_id, deployment_id = %d_id, "bundle cache hit (deployment-level)");
                self.bundle_cache.insert_both(function_id.to_string(), Some(d_id.clone()), cached.clone());
                return Ok(ResolvedBundle::Deno { code: cached });
            }
        }

        let code = code_opt.ok_or_else(|| {
            internal("no_bundle_found",
                "Bundle response contained no code field".to_string())
        })?;
        self.bundle_cache.insert_both(function_id.to_string(), deployment_id, code.clone());
        Ok(ResolvedBundle::Deno { code })
    }

    // ── private helpers ───────────────────────────────────────────────────

    /// `data` is the already-unwrapped inner data object from the bundle response.
    fn cache_schema(&self, function_id: &str, data: &Value) {
        let input  = data.get("input_schema" ).cloned().filter(|v| !v.is_null());
        let output = data.get("output_schema").cloned().filter(|v| !v.is_null());
        if input.is_some() || output.is_some() {
            self.schema_cache.insert(function_id.to_string(), FunctionSchema { input, output });
        }
    }

}

// ── Error response helpers ────────────────────────────────────────────────────

fn bad_gateway(code: &str, message: String) -> Response {
    (StatusCode::BAD_GATEWAY,
     Json(serde_json::json!({ "error": code, "message": message }))).into_response()
}

fn not_found(code: &str, message: &str) -> Response {
    (StatusCode::NOT_FOUND,
     Json(serde_json::json!({ "error": code, "message": message }))).into_response()
}

fn internal(code: &str, message: String) -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR,
     Json(serde_json::json!({ "error": code, "message": message }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── bundle_sha ────────────────────────────────────────────────────────

    #[test]
    fn bundle_sha_is_16_hex_chars() {
        let sha = bundle_sha(b"hello world");
        assert_eq!(sha.len(), 16, "sha must be 16 hex chars, got: {sha}");
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()), "sha must be hex: {sha}");
    }

    #[test]
    fn bundle_sha_same_input_same_output() {
        assert_eq!(bundle_sha(b"stable"), bundle_sha(b"stable"));
    }

    #[test]
    fn bundle_sha_different_inputs_different_outputs() {
        assert_ne!(bundle_sha(b"aaa"), bundle_sha(b"bbb"));
    }

    #[test]
    fn bundle_sha_empty_input_does_not_panic() {
        let sha = bundle_sha(b"");
        assert_eq!(sha.len(), 16);
    }

    #[test]
    fn bundle_sha_large_input() {
        let big = vec![0xABu8; 1_000_000];
        let sha = bundle_sha(&big);
        assert_eq!(sha.len(), 16);
    }
}
