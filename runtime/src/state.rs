//! Shared application state for the runtime service.
//!
//! Extracted from `main.rs` so it can be re-exported by `lib.rs` and
//! consumed by the monolithic `server` binary.

use std::sync::Arc;
use crate::secrets::client::SecretsClient;
use crate::engine::pool::IsolatePool;
use crate::engine::wasm_pool::WasmPool;
use crate::bundle::cache::BundleCache;
use crate::schema::cache::SchemaCache;
use job_contract::dispatch::ApiDispatch;

#[derive(Clone)]
pub struct AppState {
    /// Secrets with built-in LRU cache.
    pub secrets_client: SecretsClient,
    /// HTTP client for user-facing calls (WASM host HTTP, agent LLM, queue op).
    pub http_client:    reqwest::Client,
    /// Control-plane dispatch: bundle fetch, log write, secrets fetch.
    pub api:            Arc<dyn ApiDispatch>,
    /// Raw API base URL — forwarded into V8 `op_queue_push` op context.
    /// Kept alongside `api` until QueueOpState is refactored to use QueueDispatch.
    pub api_url:        String,
    /// Queue service URL — forwarded into V8 `op_queue_push` op.
    /// TODO: replace with `Arc<dyn QueueDispatch>` once the V8 op is refactored.
    pub queue_url:      String,
    pub service_token:  String,
    pub bundle_cache:   BundleCache,
    pub schema_cache:   SchemaCache,
    pub isolate_pool:   IsolatePool,
    pub wasm_pool:      WasmPool,
}
