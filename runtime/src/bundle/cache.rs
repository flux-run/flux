use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Caches function bundles to prevent redundant S3/HTTP downloads.
///
/// Two-level cache:
/// - `by_deployment`: deployment_id → code  (LRU capacity, no TTL)
/// - `by_function`:   function_id   → (code, inserted_at)  (TTL 60 s — skips control plane
///                    entirely on warm invocations)
///
/// Warm path (cache hit): 0 network calls.
/// Cold path: control plane → S3/inline → `insert_both` → execute.
#[derive(Clone)]
pub struct BundleCache {
    by_deployment: Arc<Mutex<LruCache<String, String>>>,
    by_function:   Arc<Mutex<LruCache<String, (String, Instant)>>>,
    function_ttl:  Duration,
}

impl BundleCache {
    /// `capacity` — max entries in each sub-cache (LRU eviction).
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

    // ── deployment_id cache ───────────────────────────────────────────────

    pub fn get(&self, deployment_id: &str) -> Option<String> {
        self.by_deployment.lock().unwrap().get(deployment_id).cloned()
    }

    // ── function_id cache ─────────────────────────────────────────────────

    pub fn get_by_function(&self, function_id: &str) -> Option<String> {
        let mut c = self.by_function.lock().unwrap();
        match c.get(function_id) {
            Some((code, inserted_at)) if inserted_at.elapsed() < self.function_ttl => {
                Some(code.clone())
            }
            Some(_) => { c.pop(function_id); None }   // expired
            None    => None,
        }
    }

    // ── Writes ────────────────────────────────────────────────────────────

    /// Cache code under both function_id (TTL) and deployment_id (LRU).
    pub fn insert_both(&self, function_id: String, deployment_id: Option<String>, code: String) {
        self.by_function.lock().unwrap().put(function_id, (code.clone(), Instant::now()));
        if let Some(d_id) = deployment_id {
            self.by_deployment.lock().unwrap().put(d_id, code);
        }
    }

    // ── Invalidation ─────────────────────────────────────────────────────

    pub fn invalidate_function(&self, function_id: &str) {
        self.by_function.lock().unwrap().pop(function_id);
    }

    pub fn invalidate_deployment(&self, deployment_id: &str) {
        self.by_deployment.lock().unwrap().pop(deployment_id);
    }
}
