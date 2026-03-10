use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Caches function bundles locally to prevent redundant S3/HTTP downloads.
///
/// Two-level cache:
/// - `by_deployment`: deployment_id → code  (exact match, no TTL, evicted by LRU capacity)
/// - `by_function`:   function_id  → (code, inserted_at)  (TTL-based, skips control plane
///                     entirely on warm invocations)
///
/// Warm execution path (cache hit on function_id):
///   execute_handler → BundleCache::get_by_function → execute  (0 network calls)
///
/// Cold/miss path:
///   execute_handler → control plane → S3/inline → BundleCache::insert_both → execute
#[derive(Clone)]
pub struct BundleCache {
    by_deployment: Arc<Mutex<LruCache<String, String>>>,
    by_function: Arc<Mutex<LruCache<String, (String, Instant)>>>,
    function_ttl: Duration,
}

impl BundleCache {
    /// `capacity`    — max entries in each sub-cache (LRU eviction)
    /// `function_ttl` — how long to trust a function→code mapping before
    ///                  re-validating with the control plane (default: 60s)
    pub fn new(capacity: usize) -> Self {
        Self::with_ttl(capacity, Duration::from_secs(60))
    }

    pub fn with_ttl(capacity: usize, function_ttl: Duration) -> Self {
        let cap = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(100).unwrap());
        Self {
            by_deployment: Arc::new(Mutex::new(LruCache::new(cap))),
            by_function:   Arc::new(Mutex::new(LruCache::new(cap))),
            function_ttl,
        }
    }

    // ── deployment_id cache (legacy, no TTL) ─────────────────────────────

    pub fn get(&self, deployment_id: &str) -> Option<String> {
        let mut c = self.by_deployment.lock().unwrap();
        c.get(deployment_id).cloned()
    }

    pub fn insert(&self, deployment_id: String, code: String) {
        let mut c = self.by_deployment.lock().unwrap();
        c.put(deployment_id, code);
    }

    // ── function_id cache (TTL-based, skips control plane) ───────────────

    /// Returns the cached code for this function if it was inserted within TTL.
    pub fn get_by_function(&self, function_id: &str) -> Option<String> {
        let mut c = self.by_function.lock().unwrap();
        match c.get(function_id) {
            Some((code, inserted_at)) if inserted_at.elapsed() < self.function_ttl => {
                Some(code.clone())
            }
            Some(_) => {
                // Expired — remove so the next fetch replaces it cleanly.
                c.pop(function_id);
                None
            }
            None => None,
        }
    }

    // ── Invalidation ──────────────────────────────────────────────────────

    /// Drop a function-level cache entry immediately.
    /// Called by the control plane after a new deployment is live.
    pub fn invalidate_function(&self, function_id: &str) {
        self.by_function.lock().unwrap().pop(function_id);
    }

    /// Drop a deployment-level cache entry immediately.
    pub fn invalidate_deployment(&self, deployment_id: &str) {
        self.by_deployment.lock().unwrap().pop(deployment_id);
    }

    /// Cache code under both the function_id (TTL) and deployment_id (LRU).
    pub fn insert_both(&self, function_id: String, deployment_id: Option<String>, code: String) {
        {
            let mut c = self.by_function.lock().unwrap();
            c.put(function_id, (code.clone(), Instant::now()));
        }
        if let Some(d_id) = deployment_id {
            let mut c = self.by_deployment.lock().unwrap();
            c.put(d_id, code);
        }
    }
}
