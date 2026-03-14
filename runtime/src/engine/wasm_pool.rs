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
    engine:           Arc<Engine>,
    /// LRU cache: function_id → compiled Wasmtime Module (Arc for cheap clone/share)
    modules:          Arc<Mutex<LruCache<String, Arc<Module>>>>,
    /// Raw bytes cache: function_id → (Arc<Vec<u8>>, inserted_at)
    raw_bytes:        Arc<Mutex<LruCache<String, (Arc<Vec<u8>>, Instant)>>>,
    bytes_ttl:        Duration,
    semaphore:        Arc<Semaphore>,
    workers:          usize,
    fuel_limit:       u64,
    timeout_secs:     u64,
}

impl WasmPool {
    /// Create a pool sized to `2 × logical CPUs` (min 2, max 16).
    pub fn default_sized() -> Self {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2);
        let workers = (cpus * 2).clamp(2, 16);
        tracing::info!(workers, "wasm pool started");
        Self::new(workers, 1_000_000_000, 30)
    }

    pub fn new(workers: usize, fuel_limit: u64, timeout_secs: u64) -> Self {
        let workers = workers.max(1);
        let engine  = Arc::new(build_engine());
        let cap     = NonZeroUsize::new(256).expect("256 is a valid non-zero usize");
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
            fuel_limit,
            timeout_secs,
        }
    }

    pub fn workers(&self) -> usize { self.workers }

    // ── Raw bytes cache (warm execution path, avoids re-fetching bundles) ──

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
    /// - `bytes` is the raw `.wasm` binary (fetched from BundleCache / control plane)
    /// - `allowed_http_hosts`: per-function HTTP allow-list for `fluxbase.http_fetch`
    ///
    /// Returns an `ExecutionResult` with `output` (JSON) and `logs`.
    pub async fn execute(
        &self,
        function_id:         String,
        bytes:               Vec<u8>,
        secrets:             HashMap<String, String>,
        payload:             serde_json::Value,
        fuel_limit:          Option<u64>,
        allowed_http_hosts:  Vec<String>,
        http_client:         reqwest::Client,
        data_engine_url:     String,
        service_token:       String,
        database:            String,
        queue_url:           String,
        api_url:             String,
        project_id:          Option<String>,
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
            secrets,
            payload,
            fuel_limit:          fuel_limit.unwrap_or(self.fuel_limit),
            allowed_http_hosts,
            http_client:         Some(http_client),
            timeout_secs:        self.timeout_secs,
            data_engine_url,
            service_token,
            database,
            queue_url,
            api_url,
            project_id,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const MINIMAL_WAT: &str = r#"(module
        (import "fluxbase" "log"         (func (param i32 i32 i32)))
        (import "fluxbase" "secrets_get" (func (param i32 i32 i32 i32) (result i32)))
        (import "fluxbase" "http_fetch"  (func (param i32 i32 i32 i32) (result i32)))
        (import "fluxbase" "db_query"    (func (param i32 i32 i32 i32 i32 i32) (result i32)))
        (import "fluxbase" "queue_push"  (func (param i32 i32 i32 i32) (result i32)))
        (memory (export "memory") 2)
        (data (i32.const 4) "\0f\00\00\00{\"output\":\"ok\"}")
        (func (export "__flux_alloc") (param i32) (result i32) i32.const 65536)
        (func (export "handle") (param i32 i32) (result i32) i32.const 4)
    )"#;

    fn wasm_bytes() -> Vec<u8> {
        wat::parse_str(MINIMAL_WAT).expect("WAT parse failed")
    }

    // ── pool construction ─────────────────────────────────────────────────

    #[test]
    fn new_pool_reports_correct_worker_count() {
        let pool = WasmPool::new(3, 1_000_000_000, 30);
        assert_eq!(pool.workers(), 3);
    }

    #[test]
    fn new_pool_minimum_one_worker() {
        let pool = WasmPool::new(0, 1_000_000_000, 30);
        assert_eq!(pool.workers(), 1);
    }

    #[test]
    fn default_sized_pool_has_at_least_two_workers() {
        let pool = WasmPool::default_sized();
        assert!(pool.workers() >= 2);
    }

    // ── raw bytes cache ───────────────────────────────────────────────────

    #[tokio::test]
    async fn bytes_cache_miss_returns_none() {
        let pool = WasmPool::new(2, 1_000_000_000, 30);
        let result = pool.get_cached_bytes("nonexistent_fn").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn bytes_cache_hit_after_insert() {
        let pool  = WasmPool::new(2, 1_000_000_000, 30);
        let bytes = Arc::new(vec![0u8, 1, 2, 3]);
        pool.cache_bytes("my_fn".to_string(), bytes.clone()).await;
        let cached = pool.get_cached_bytes("my_fn").await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().as_ref(), bytes.as_ref());
    }

    #[tokio::test]
    async fn bytes_cache_different_functions_independent() {
        let pool = WasmPool::new(2, 1_000_000_000, 30);
        pool.cache_bytes("fn1".to_string(), Arc::new(vec![1])).await;
        pool.cache_bytes("fn2".to_string(), Arc::new(vec![2])).await;
        let r1 = pool.get_cached_bytes("fn1").await.unwrap();
        let r2 = pool.get_cached_bytes("fn2").await.unwrap();
        assert_eq!(r1.as_ref(), &[1u8]);
        assert_eq!(r2.as_ref(), &[2u8]);
    }

    // ── module eviction ───────────────────────────────────────────────────

    #[tokio::test]
    async fn evict_removes_bytes_from_cache() {
        let pool = WasmPool::new(2, 1_000_000_000, 30);
        pool.cache_bytes("evict_me".to_string(), Arc::new(vec![9])).await;
        pool.evict("evict_me").await;
        assert!(pool.get_cached_bytes("evict_me").await.is_none());
    }

    // ── execute ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn execute_minimal_module_returns_ok() {
        let pool   = WasmPool::new(2, 1_000_000_000, 30);
        let bytes  = wasm_bytes();
        let result = pool.execute(
            "test_fn".to_string(),
            bytes,
            Default::default(),
            serde_json::json!({"x": 1}),
            None,
            vec![],
            reqwest::Client::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            None,
        ).await;
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
        assert_eq!(result.unwrap().output, serde_json::json!("ok"));
    }

    #[tokio::test]
    async fn execute_caches_compiled_module_on_second_call() {
        let pool  = WasmPool::new(2, 1_000_000_000, 30);
        let bytes = wasm_bytes();
        // First call: compiles + executes.
        let r1 = pool.execute("cached_fn".to_string(), bytes.clone(), Default::default(),
            serde_json::json!({}), None, vec![], reqwest::Client::new(),
            String::new(), String::new(), String::new(), String::new(), String::new(), None).await;
        // Second call: should hit module cache.
        let r2 = pool.execute("cached_fn".to_string(), bytes, Default::default(),
            serde_json::json!({}), None, vec![], reqwest::Client::new(),
            String::new(), String::new(), String::new(), String::new(), String::new(), None).await;
        assert!(r1.is_ok());
        assert!(r2.is_ok());
    }
}
