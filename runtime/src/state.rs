//! Shared application state for the runtime service.
//!
//! ## Fields
//!
//! - **`secrets_client`** — Secrets client with built-in LRU + TTL cache (50 entries,
//!   30 s TTL). Backed by `ApiDispatch::get_secrets` — avoids ~5 ms control-plane RTT
//!   on warm invocations.
//!
//! - **`http_client`** — Shared reqwest client for user-facing outbound calls.
//!   Connection pooling is critical here — user functions can be invoked at high
//!   concurrency and each must not open a new TCP connection.
//!
//! - **`api`** — Control-plane dispatch: bundle fetch, span write, secrets fetch.
//!   In multi-process mode this is `HttpApiDispatch`; in server mode it is
//!   `InProcessApiDispatch`. The runtime never knows which — DIP satisfied.
//!
//! - **`queue`** — Queue dispatch: in-process job enqueue via `QueueDispatch`.
//!
//! - **`data_engine`** — Data-engine dispatch: in-process SQL execution via
//!   `DataEngineDispatch`.
//!
//! - **`service_token`** — Internal service token threaded to queue and API calls
//!   originating from inside user functions.
//!
//! - **`bundle_cache`** — Two-level LRU + TTL bundle cache. `by_function` (60 s TTL)
//!   skips the control plane entirely on warm invocations. `by_deployment` (LRU only)
//!   handles explicit deployment-id lookups.
//!
//! - **`schema_cache`** — Per-function input JSON Schema cache. Used by
//!   `ExecutionRunner` to validate the `payload` before dispatching to V8.
//!
//! - **`isolate_pool`** — Fixed pool of OS threads each owning a warm `JsRuntime`
//!   (V8 heap + Flux extension loaded once). Eliminates per-request V8 init
//!   overhead (~3–5 ms). Sized by `ISOLATE_WORKERS` env var.

use std::sync::Arc;
use crate::secrets::client::SecretsClient;
use crate::engine::executor::PoolDispatchers;
use crate::engine::pool::IsolatePool;
use crate::bundle::cache::BundleCache;
use crate::schema::cache::SchemaCache;
use job_contract::dispatch::{ApiDispatch, DataEngineDispatch, QueueDispatch};

#[derive(Clone)]
pub struct AppState {
    /// Secrets with built-in LRU cache.
    pub secrets_client: SecretsClient,
    /// HTTP client for user-facing calls.
    pub http_client:    reqwest::Client,
    /// Control-plane dispatch: bundle fetch, log write, secrets fetch.
    pub api:            Arc<dyn ApiDispatch>,
    /// Queue dispatch: enqueue jobs from V8 ops.
    pub queue:          Arc<dyn QueueDispatch>,
    /// Data-engine dispatch: execute SQL from V8 ops.
    pub data_engine:    Arc<dyn DataEngineDispatch>,
    pub service_token:  String,
    pub bundle_cache:   BundleCache,
    pub schema_cache:   SchemaCache,
    pub isolate_pool:   IsolatePool,
    /// Dispatch traits shared with V8 ops.
    pub dispatchers:    PoolDispatchers,
}
