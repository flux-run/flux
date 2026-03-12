//! `WasmPool` — bounded concurrency pool for Wasmtime WASM module execution.
//!
//! Unlike `IsolatePool` (which pre-spawns dedicated OS threads for V8 which is
//! `!Send`), `WasmPool` is simpler: Wasmtime's `Engine` and `Module` are both
//! `Send + Sync`, so execution can be offloaded to tokio's `spawn_blocking` pool.
//!
//! The pool provides:
//! - A shared `Arc<Engine>` (one Cranelift engine per process)
//! - An LRU cache of compiled `Module`s keyed by `function_id`
//!   (compilation is the expensive step; instantiation is cheap)
//! - A `Semaphore` bounding concurrent WASM executions to `max(2×CPU, 16)`

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lru::LruCache;
use tokio::sync::{Mutex, Semaphore};
use wasmtime::{Engine, Module};

use super::executor::ExecutionResult;
use super::wasm_executor::{build_engine, compile_module, execute_wasm, WasmExecutionParams};

// ─── WasmPool ───────────────────────────────────────────────────────────────

/// A pool that executes WASM function bundles with bounded concurrency and
/// compiled-module caching.
#[derive(Clone)]
pub struct WasmPool {
    engine:    Arc<Engine>,
    /// LRU cache: function_id → compiled Wasmtime Module (Arc for cheap clone/share)
    modules:   Arc<Mutex<LruCache<String, Arc<Module>>>>,
    /// Raw bytes cache: function_id → (Arc<Vec<u8>>, inserted_at)
    /// Warm-path equivalent of BundleCache — avoids re-downloading from S3.
    raw_bytes: Arc<Mutex<LruCache<String, (Arc<Vec<u8>>, Instant)>>>,
    bytes_ttl: Duration,
    semaphore: Arc<Semaphore>,
    workers:   usize,
}

impl WasmPool {
    /// Create a pool sized to `2 × logical CPUs` (min 2, max 16).
    /// Module cache holds up to 256 compiled modules.
    pub fn default_sized() -> Self {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2);
        let workers = (cpus * 2).clamp(2, 16);
        tracing::info!(workers, "wasm pool started");
        Self::new(workers)
    }

    pub fn new(workers: usize) -> Self {
        let workers = workers.max(1);
        let engine  = Arc::new(build_engine());
        let cap     = NonZeroUsize::new(256).unwrap();
        let modules   = Arc::new(Mutex::new(LruCache::new(cap)));
        let raw_bytes = Arc::new(Mutex::new(LruCache::new(cap)));
        let semaphore = Arc::new(Semaphore::new(workers));
        Self {
            engine,
            modules,
            raw_bytes,
            bytes_ttl: Duration::from_secs(60),
            semaphore,
            workers,
        }
    }

    pub fn workers(&self) -> usize { self.workers }

    // ── Raw bytes cache (warm execution path, avoids re-fetching from S3) ──

    /// Return cached bytes for `function_id` if within TTL.
    pub async fn get_cached_bytes(&self, function_id: &str) -> Option<Arc<Vec<u8>>> {
        let mut cache = self.raw_bytes.lock().await;
        match cache.get(function_id) {
            Some((bytes, ts)) if ts.elapsed() < self.bytes_ttl => {
                tracing::debug!(%function_id, "wasm bytes cache hit");
                Some(bytes.clone())
            }
            Some(_) => { cache.pop(function_id); None }
            None    => None,
        }
    }

    /// Store raw bytes for the warm path. Also evicts the compiled module so
    /// the next execution recompiles from the fresh bytes.
    pub async fn cache_bytes(&self, function_id: String, bytes: Arc<Vec<u8>>) {
        {
            let mut cache = self.raw_bytes.lock().await;
            cache.put(function_id.clone(), (bytes, Instant::now()));
        }
        // Don't evict the compiled module — if bytes haven't changed (same
        // deployment), the cached Module is still valid.
    }

    /// Execute a WASM function bundle.
    ///
    /// - `function_id` is the cache key; same value as used in `IsolatePool`
    /// - `bytes` is the raw `.wasm` binary (fetched from BundleCache / S3)
    /// - `allowed_http_hosts`: per-function HTTP allow-list for `fluxbase.http_fetch`
    ///
    /// Returns an `ExecutionResult` with `output` (JSON) and `logs`.
    pub async fn execute(
        &self,
        function_id:         String,
        bytes:               Vec<u8>,
        secrets:             HashMap<String, String>,
        payload:             serde_json::Value,
        tenant_id:           String,
        fuel_limit:          Option<u64>,
        allowed_http_hosts:  Vec<String>,
        http_client:         reqwest::Client,
    ) -> Result<ExecutionResult, String> {
        // ── Acquire concurrency slot ──────────────────────────────────────
        let _permit = self.semaphore
            .acquire()
            .await
            .map_err(|_| "wasm pool is shut down".to_string())?;

        // ── Resolve compiled Module (cache hit → skip compilation) ────────
        let module: Arc<Module> = {
            let mut cache = self.modules.lock().await;
            if let Some(m) = cache.get(&function_id) {
                tracing::debug!(%function_id, "wasm module cache hit");
                m.clone()
            } else {
                tracing::debug!(%function_id, "wasm module cache miss — compiling");
                let engine = self.engine.as_ref();
                let module = compile_module(engine, &bytes)
                    .map_err(|e| e)?;
                let arc = Arc::new(module);
                cache.put(function_id.clone(), arc.clone());
                arc
            }
        };

        // ── Execute on a blocking thread ──────────────────────────────────
        let params = WasmExecutionParams {
            bytes,
            secrets,
            payload,
            tenant_id,
            function_id,
            fuel_limit:          fuel_limit.unwrap_or(1_000_000_000),
            allowed_http_hosts,
            http_client:         Some(http_client),
        };

        execute_wasm(self.engine.as_ref(), module.as_ref(), params).await

        // _permit is dropped here — slot released
    }

    /// Evict a compiled module from the cache (called after a new deployment).
    pub async fn evict(&self, function_id: &str) {
        self.modules.lock().await.pop(function_id);
        self.raw_bytes.lock().await.pop(function_id);
        tracing::debug!(%function_id, "wasm module + bytes evicted from cache");
    }
}
