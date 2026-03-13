/// BundleResolver — single-responsibility bundle resolution for the execute pipeline.
///
/// Encapsulates the three-path resolution strategy:
///   1. Warm WASM path  — cached bytes in WasmPool (zero network calls)
///   2. Warm Deno path  — cached code in BundleCache (zero network calls)
///   3. Cold path       — fetch from control plane → S3/inline → populate both caches
///
/// Separated from the HTTP handler and execution runner so each can be
/// tested and reasoned about independently.
use std::sync::Arc;
use axum::http::StatusCode;
use axum::Json;
use axum::response::{IntoResponse, Response};
use serde_json::Value;

use job_contract::dispatch::ApiDispatch;
use crate::bundle::cache::BundleCache;
use crate::schema::cache::{SchemaCache, FunctionSchema};
use crate::engine::wasm_pool::WasmPool;
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
    pub wasm_pool:     &'a WasmPool,
    /// Used for S3/R2 asset downloads only; API calls go through `api`.
    pub http_client:   &'a reqwest::Client,
    /// Control-plane dispatch — used for the bundle metadata fetch.
    pub api:           &'a dyn ApiDispatch,
}

impl<'a> BundleResolver<'a> {
    /// Attempt the warm WASM path.
    /// Returns `Some(bytes)` on a cache hit, `None` on a miss.
    pub async fn warm_wasm(&self, function_id: &str) -> Option<Arc<Vec<u8>>> {
        self.wasm_pool.get_cached_bytes(function_id).await
    }

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
        tracer:      &TraceEmitter,
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

        let bundle_runtime = data.get("runtime")
            .and_then(|r| r.as_str())
            .unwrap_or("deno")
            .to_string();

        let deployment_id = data.get("deployment_id").and_then(|v| v.as_str()).map(|s| s.to_string());
        let url_opt       = data.get("url")          .and_then(|v| v.as_str()).map(|s| s.to_string());
        let code_opt      = data.get("code")         .and_then(|v| v.as_str()).map(|s| s.to_string());

        if bundle_runtime == "wasm" {
            let bytes = self.fetch_wasm_bytes(
                function_id, url_opt, code_opt, tracer
            ).await?;
            self.wasm_pool.cache_bytes(function_id.to_string(), Arc::new(bytes.clone())).await;
            return Ok(ResolvedBundle::Wasm { bytes });
        }

        // ── Deno cold path ────────────────────────────────────────────────
        // Check deployment-level cache first (avoids re-downloading from S3).
        if let Some(d_id) = &deployment_id {
            if let Some(cached) = self.bundle_cache.get(d_id) {
                tracing::debug!(function_id, deployment_id = %d_id, "bundle cache hit (deployment-level)");
                self.bundle_cache.insert_both(function_id.to_string(), Some(d_id.clone()), cached.clone());
                return Ok(ResolvedBundle::Deno { code: cached });
            }
        }

        let code = self.fetch_js_code(
            function_id, deployment_id.clone(), url_opt, code_opt, tracer
        ).await?;
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

    async fn fetch_wasm_bytes(
        &self,
        _function_id: &str,
        url_opt:     Option<String>,
        code_opt:    Option<String>,
        tracer:      &TraceEmitter,
    ) -> Result<Vec<u8>, Response> {
        if let Some(url) = url_opt {
            let res = self.http_client.get(&url).send().await
                .map_err(|e| internal("S3FetchError",
                    format!("Failed to download wasm bundle: {}", e)))?;
            if !res.status().is_success() {
                let status = res.status().as_u16();
                let body   = res.text().await.unwrap_or_default();
                return Err(internal("S3FetchError",
                    format!("S3 returned HTTP {}: {}", status, body)));
            }
            let elapsed = 0u64; // timing tracked in runner
            tracer.post_event("info", format!("wasm bundle fetched from R2 ({}ms)", elapsed));
            Ok(res.bytes().await.map(|b| b.to_vec()).unwrap_or_default())
        } else if let Some(encoded) = code_opt {
            use base64::Engine as _;
            Ok(base64::engine::general_purpose::STANDARD
                .decode(&encoded)
                .unwrap_or_else(|_| encoded.into_bytes()))
        } else {
            Err(not_found("no_bundle_found", "No wasm bundle found for this function."))
        }
    }

    async fn fetch_js_code(
        &self,
        function_id:   &str,
        deployment_id: Option<String>,
        url_opt:       Option<String>,
        code_opt:      Option<String>,
        tracer:        &TraceEmitter,
    ) -> Result<String, Response> {
        if let Some(url) = url_opt {
            let res = self.http_client.get(&url).send().await
                .map_err(|e| internal("S3FetchError",
                    format!("Failed to download bundle: {}", e)))?;
            if !res.status().is_success() {
                let status = res.status().as_u16();
                let body   = res.text().await.unwrap_or_default();
                return Err(internal("S3FetchError",
                    format!("S3 returned HTTP {}: {}", status, body)));
            }
            let text = res.text().await.unwrap_or_default();
            tracer.post_event("info", "bundle fetched from R2".to_string());
            self.bundle_cache.insert_both(function_id.to_string(), deployment_id, text.clone());
            Ok(text)
        } else if let Some(code) = code_opt {
            // Inline code stored directly in the DB (local dev / small functions).
            self.bundle_cache.insert_both(function_id.to_string(), deployment_id, code.clone());
            Ok(code)
        } else {
            Err(internal("no_bundle_found",
                "Bundle response contained neither url nor code field".to_string()))
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
