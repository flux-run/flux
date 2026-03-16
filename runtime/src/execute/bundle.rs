/// BundleResolver — single-responsibility bundle resolution for the execute pipeline.
///
/// Encapsulates the two-path resolution strategy:
///   1. Warm Deno path  — cached code in BundleCache (zero network calls)
///   2. Cold path       — fetch from control plane → inline code → populate cache
///
/// Separated from the HTTP handler and execution runner so each can be
/// tested and reasoned about independently.
use crate::contracts::ApiDispatch;
use crate::bundle::cache::BundleCache;
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
    /// Returns `Ok(ResolvedBundle)` on success, or an `Err(String)`.
    pub async fn cold_fetch(
        &self,
        function_id: &str,
        _tracer:     &TraceEmitter,
    ) -> Result<ResolvedBundle, String> {
        // Fetch bundle metadata via the ApiDispatch trait (HTTP in multi-process
        // mode, direct call in single-binary mode).  The dispatch impl already
        // unwraps the outer `{ success, data }` envelope and returns the inner
        // data object.
        let data = self.api.get_bundle(function_id).await.map_err(|e| {
            if e.contains("HTTP 404") {
                "no_bundle_found: No active deployment found for this function. Deploy it first.".to_string()
            } else {
                format!("BundleFetchError: Failed to reach API service: {}", e)
            }
        })?;

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
            "no_bundle_found: Bundle response contained no code field".to_string()
        })?;
        self.bundle_cache.insert_both(function_id.to_string(), deployment_id, code.clone());
        Ok(ResolvedBundle::Deno { code })
    }

    // ── private helpers ───────────────────────────────────────────────────

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
