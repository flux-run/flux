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
//!
//! ## AOT disk cache
//!
//! WASM → machine code compilation (Cranelift) is expensive for large interpreter
//! runtimes: PHP 13 MB ≈ 88 s, Python 30 MB ≈ several minutes on a debug build.
//! To avoid re-paying this cost on every process restart, compiled modules are
//! persisted to disk as Wasmtime "precompiled" artifacts (`.cwasm` files).
//!
//! Flow:
//!   1. Cold start, no disk cache → compile WASM → `Module::serialize()` → write
//!      `~/.flux/wasm-cache/<fingerprint>-<engine>.cwasm`
//!   2. Same WASM is seen again (process restart, new deployment of unchanged code)
//!      → `Module::deserialize()` from the `.cwasm` file → sub-100 ms load
//!   3. New deployment (different bytes → different fingerprint) → compile again,
//!      old `.cwasm` is left on disk until `flux wasm-cache prune` cleans it up
//!
//! The `.cwasm` format is platform-specific (ARM64 vs x86-64) and Wasmtime-version-
//! specific.  Files from a different machine or a Wasmtime upgrade will fail to
//! deserialize; the pool detects this and falls back to recompilation automatically.

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lru::LruCache;
use tokio::sync::{Mutex, Semaphore};
use wasmtime::{Engine, Module};

use super::executor::{ExecutionResult, PoolDispatchers};
use super::wasm_executor::{build_engine, build_engine_fast, compile_module, execute_wasm, parse_wasi_args, WasmExecutionParams};

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Compute a deterministic (seed-independent) fingerprint of a byte slice.
///
/// Uses FNV-1a 64-bit, which is:
/// - Deterministic across process restarts (unlike Rust's `DefaultHasher` which
///   randomises its seed per process for DoS protection — that would break the
///   disk AOT cache whose filenames are keyed by fingerprint).
/// - Fast: single-pass, no allocations, O(n).
/// - Collision-resistant enough for identifying unique WASM bundles.
fn bytes_fingerprint(bytes: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 14695981039346656037;
    const FNV_PRIME:  u64 = 1099511628211;
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// Binaries larger than this threshold are compiled with `OptLevel::None` to avoid
/// Cranelift spending minutes on huge interpreter dispatch functions (PHP, Python, Ruby).
const LARGE_WASM_THRESHOLD: usize = 5 * 1024 * 1024; // 5 MB

/// Return the AOT disk-cache directory, creating it if needed.
/// Path: `~/.flux/wasm-cache/`  (or `$FLUX_WASM_CACHE_DIR` if set).
fn cache_dir() -> Option<PathBuf> {
    let dir = std::env::var("FLUX_WASM_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".flux").join("wasm-cache")
        });
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Path for a precompiled `.cwasm` artifact.
/// The filename encodes the content fingerprint and which engine variant was used
/// so that a new deployment or an engine change both produce a cache miss.
fn cwasm_path(fingerprint: u64, is_fast_engine: bool) -> Option<PathBuf> {
    let engine_tag = if is_fast_engine { "fast" } else { "speed" };
    Some(cache_dir()?.join(format!("{:016x}-{}.cwasm", fingerprint, engine_tag)))
}

/// Try to load a precompiled module from the disk cache.
/// Returns `None` on any error (missing file, wrong Wasmtime version, wrong arch).
fn load_from_disk(engine: &Engine, fingerprint: u64, is_fast: bool) -> Option<Module> {
    let path = cwasm_path(fingerprint, is_fast)?;
    match unsafe { Module::deserialize_file(engine, &path) } {
        Ok(m) => {
            tracing::info!(?path, "wasm AOT disk cache hit");
            Some(m)
        }
        Err(e) => {
            // Stale / incompatible artifact — remove it so it doesn't consume disk
            tracing::warn!(?path, err = %e, "wasm AOT disk cache miss (stale/incompatible), recompiling");
            let _ = std::fs::remove_file(&path);
            None
        }
    }
}

/// Serialize a compiled module to disk for future process restarts.
/// Errors are logged and silently ignored — the disk cache is best-effort.
fn save_to_disk(module: &Module, fingerprint: u64, is_fast: bool) {
    let Some(path) = cwasm_path(fingerprint, is_fast) else { return };
    match module.serialize() {
        Ok(bytes) => {
            // Write to a temp file first, then rename for atomicity.
            let tmp = path.with_extension("cwasm.tmp");
            if let Err(e) = std::fs::write(&tmp, &bytes) {
                tracing::warn!(?tmp, err = %e, "failed to write AOT cache (tmp)");
                return;
            }
            if let Err(e) = std::fs::rename(&tmp, &path) {
                tracing::warn!(?path, err = %e, "failed to rename AOT cache file");
                let _ = std::fs::remove_file(&tmp);
            } else {
                tracing::info!(?path, bytes = bytes.len(), "wasm AOT compiled and cached");
            }
        }
        Err(e) => tracing::warn!(err = %e, "failed to serialize compiled WASM module"),
    }
}

// ─── WasmPool ───────────────────────────────────────────────────────────────

/// A pool that executes WASM function bundles with bounded concurrency and
/// compiled-module caching.
#[derive(Clone)]
pub struct WasmPool {
    engine:           Arc<Engine>,
    /// `OptLevel::None` engine for interpreter WASM (PHP, Python, Ruby).
    /// Shared via `Arc` so the pool is cheap to clone.
    fast_engine:      Arc<Engine>,
    /// LRU cache: function_id → (compiled Module, bytes fingerprint, engine Arc).
    /// `engine` is stored so that execution uses the same engine the module was compiled with.
    /// The fingerprint detects new deployments — different bytes = cache miss = recompile.
    modules:          Arc<Mutex<LruCache<String, (Arc<Module>, u64, Arc<Engine>)>>>,
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
        // 10 billion fuel: Python/Ruby WASM interpreters consume far more VM
        // instructions than hand-written WASM (Rust, Go, AssemblyScript).
        // 120 s timeout: large slow-start WASM (py2wasm, rbwasm) may take 60 s.
        Self::new(workers, 10_000_000_000, 120)
    }

    pub fn new(workers: usize, fuel_limit: u64, timeout_secs: u64) -> Self {
        let workers   = workers.max(1);
        let engine    = Arc::new(build_engine());
        let fast_engine = Arc::new(build_engine_fast());
        let cap       = NonZeroUsize::new(256).expect("256 is a valid non-zero usize");
        let modules   = Arc::new(Mutex::new(LruCache::new(cap)));
        let raw_bytes = Arc::new(Mutex::new(LruCache::new(cap)));
        let semaphore = Arc::new(Semaphore::new(workers));
        Self {
            engine,
            fast_engine,
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

    /// Store raw bytes for the warm path.
    pub async fn cache_bytes(&self, function_id: String, bytes: Arc<Vec<u8>>) {
        let mut cache = self.raw_bytes.lock().await;
        cache.put(function_id.clone(), (bytes, Instant::now()));
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
        database:            String,
        dispatchers:         PoolDispatchers,
    ) -> Result<ExecutionResult, String> {
        // ── Acquire concurrency slot ──────────────────────────────────────
        let _permit = self.semaphore
            .acquire()
            .await
            .map_err(|_| "wasm pool is shut down".to_string())?;

        // ── Resolve compiled Module (content-addressed cache) ─────────────
        //
        // The cache key is (function_id, bytes_fingerprint).  When a new
        // deployment writes different bytes, the fingerprint changes → cache
        // miss → recompile.  This prevents stale compiled modules (e.g. a
        // module compiled from a WASI-linked binary) from surviving a redeploy.
        //
        // Compilation is done on a spawn_blocking thread so that large WASM
        // modules (Python 26 MB, Ruby 47 MB) do not block tokio worker threads
        // for the full Cranelift JIT compilation duration (up to several minutes).
        // The cache lock is held only for the fast lookup and insert — not during
        // the compile itself.
        //
        // Three-level cache (fastest → slowest):
        //   1. In-memory LRU  — Arc<Module>, instant
        //   2. Disk AOT cache — ~/.flux/wasm-cache/<fp>-<engine>.cwasm, ~50 ms
        //   3. Cranelift JIT  — compile from WASM bytecode, seconds–minutes
        let fingerprint = bytes_fingerprint(&bytes);
        let is_large    = bytes.len() > LARGE_WASM_THRESHOLD;

        let (module, exec_engine): (Arc<Module>, Arc<Engine>) = {
            // Level 1: in-memory LRU (microseconds).
            let cached = {
                let mut cache = self.modules.lock().await;
                match cache.get(&function_id) {
                    Some((m, fp, eng)) if *fp == fingerprint => {
                        tracing::debug!(%function_id, "wasm module cache hit (memory)");
                        Some((m.clone(), eng.clone()))
                    }
                    _ => None,
                }
            };

            if let Some(pair) = cached {
                pair
            } else {
                // Choose engine based on binary size — large interpreter runtimes
                // (PHP, Python, Ruby) use OptLevel::None to avoid minutes-long
                // Cranelift compilations caused by their huge dispatch functions.
                let engine_arc = if is_large {
                    tracing::info!(%function_id, bytes = bytes.len(), "using fast engine (OptLevel::None) for large WASM");
                    self.fast_engine.clone()
                } else {
                    self.engine.clone()
                };

                // Level 2: disk AOT cache (~50 ms load vs seconds of Cranelift).
                let disk_module = {
                    let eng = engine_arc.clone();
                    tokio::task::spawn_blocking(move || {
                        load_from_disk(eng.as_ref(), fingerprint, is_large)
                    }).await.unwrap_or(None)
                };

                let compiled = if let Some(m) = disk_module {
                    m
                } else {
                    // Level 3: Cranelift JIT — compile from WASM bytecode.
                    tracing::info!(%function_id, bytes = bytes.len(), "wasm module cache miss — compiling (Cranelift)");
                    let bytes_clone = bytes.clone();
                    let eng2 = engine_arc.clone();
                    let fp   = fingerprint;
                    let fast = is_large;
                    tokio::task::spawn_blocking(move || {
                        let m = compile_module(eng2.as_ref(), &bytes_clone)?;
                        // Persist to disk so future restarts skip recompilation.
                        save_to_disk(&m, fp, fast);
                        Ok::<Module, String>(m)
                    })
                    .await
                    .map_err(|e| format!("compile task panicked: {}", e))??
                };

                let arc = Arc::new(compiled);
                // Re-acquire lock to insert — another request may have compiled
                // concurrently; prefer whichever finished first.
                let mut cache = self.modules.lock().await;
                match cache.get(&function_id) {
                    Some((m, fp, eng)) if *fp == fingerprint => (m.clone(), eng.clone()),
                    _ => {
                        cache.put(function_id.clone(), (arc.clone(), fingerprint, engine_arc.clone()));
                        (arc, engine_arc)
                    }
                }
            }
        };

        // ── Execute on a blocking thread ──────────────────────────────────
        // Parse the `flux.wasi-args` custom section from the module bytes.
        // For interpreter WASM (PHP: ["php", "-r", "<code>"]) this provides the
        // argv needed by the interpreter to know which script to run.
        // For self-contained binaries (Go, Rust, AssemblyScript) returns empty vec.
        let wasi_argv = parse_wasi_args(&bytes);

        let params = WasmExecutionParams {
            secrets,
            wasi_argv,
            payload,
            fuel_limit:          fuel_limit.unwrap_or(self.fuel_limit),
            allowed_http_hosts,
            http_client:         Some(http_client),
            timeout_secs:        self.timeout_secs,
            database,
            dispatchers,
        };

        execute_wasm(exec_engine.as_ref(), module.as_ref(), params).await

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
    use std::collections::HashMap;
    use async_trait::async_trait;
    use job_contract::dispatch::{ApiDispatch, DataEngineDispatch, QueueDispatch};
    use crate::engine::executor::PoolDispatchers;

    struct MockApi;
    #[async_trait]
    impl ApiDispatch for MockApi {
        async fn get_bundle(&self, _: &str) -> Result<serde_json::Value, String> { Err("mock".into()) }
        async fn write_log(&self, _: serde_json::Value) -> Result<(), String> { Ok(()) }
        async fn get_secrets(&self) -> Result<HashMap<String, String>, String> { Ok(Default::default()) }
        async fn resolve_function(&self, _: &str) -> Result<job_contract::dispatch::ResolvedFunction, String> { Err("mock".into()) }
    }
    struct MockQueue;
    #[async_trait]
    impl QueueDispatch for MockQueue {
        async fn push_job(&self, _: &str, _: serde_json::Value, _: Option<u64>, _: Option<String>) -> Result<(), String> { Ok(()) }
    }
    struct MockDataEngine;
    #[async_trait]
    impl DataEngineDispatch for MockDataEngine {
        async fn execute_sql(&self, _: String, _: Vec<serde_json::Value>, _: String, _: String) -> Result<serde_json::Value, String> { Ok(serde_json::json!({})) }
    }
    fn test_dispatchers() -> PoolDispatchers {
        PoolDispatchers {
            api: Arc::new(MockApi),
            queue: Arc::new(MockQueue),
            data_engine: Arc::new(MockDataEngine),
            runtime: Arc::new(std::sync::OnceLock::new()),
        }
    }

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
            test_dispatchers(),
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
            String::new(), test_dispatchers()).await;
        // Second call: should hit module cache.
        let r2 = pool.execute("cached_fn".to_string(), bytes, Default::default(),
            serde_json::json!({}), None, vec![], reqwest::Client::new(),
            String::new(), test_dispatchers()).await;
        assert!(r1.is_ok());
        assert!(r2.is_ok());
    }
}
